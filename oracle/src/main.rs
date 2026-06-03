//! Real database-engine reference oracle.
//!
//! For each reference dialect this brings up the actual engine in Docker, labels
//! every corpus statement valid/invalid by the schema-free rule (a syntax/parse
//! error means invalid; no error or a schema/semantic error means it parsed, so
//! valid), and writes a committed cache under `oracle/labels/{dir}.tsv.zst` that
//! `sql_ast_benchmark::oracle_cache` reads. Run locally with Docker:
//!
//!   cargo run --release -p oracle              # all implemented dialects
//!   cargo run --release -p oracle -- sqlite    # one or more by dir name
//!
//! Server engines (PostgreSQL) connect over a mapped port via testcontainers.
//! CLI engines (SQLite) are driven by a one-shot container reading a script on
//! stdin, since they have no network protocol.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use sql_ast_benchmark::datasets::{ensure_corpus, Dialect};
use sql_ast_benchmark::oracle_cache::{statement_hash, LABELS_DIR};
use sql_ast_benchmark::report::load_dialect_from;

/// Dialects with an adapter implemented so far.
const IMPLEMENTED: &[&str] = &["postgresql", "sqlite", "mysql", "clickhouse"];

#[tokio::main]
async fn main() -> Result<()> {
    ensure_corpus().context("dataset corpus")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let wanted: Vec<String> = if args.is_empty() {
        IMPLEMENTED.iter().map(|s| (*s).to_string()).collect()
    } else {
        args
    };
    std::fs::create_dir_all(LABELS_DIR)?;

    for name in &wanted {
        let Some(dialect) = Dialect::from_dir_name(name) else {
            eprintln!("unknown dialect: {name}");
            continue;
        };
        let stmts = load_dialect_from(Path::new("datasets"), dialect);
        if stmts.is_empty() {
            eprintln!("no corpus for {name}, skipping");
            continue;
        }
        eprintln!("labeling {name}: {} statements", stmts.len());
        let valid = match dialect {
            Dialect::Postgresql => label_postgresql(&stmts).await?,
            Dialect::Sqlite => label_sqlite(&stmts)?,
            Dialect::Mysql => label_mysql(&stmts).await?,
            Dialect::Clickhouse => label_clickhouse(&stmts).await?,
            _ => {
                eprintln!("{name}: no adapter yet, skipping");
                continue;
            }
        };
        write_cache(dialect, &stmts, &valid)?;
    }
    Ok(())
}

/// Write the per-dialect validity cache: header line = corpus statement count,
/// then `hash\t0|1` for each unique statement.
fn write_cache(dialect: Dialect, stmts: &[String], valid: &[bool]) -> Result<()> {
    let mut map: HashMap<u64, bool> = HashMap::new();
    for (s, &v) in stmts.iter().zip(valid) {
        map.entry(statement_hash(s)).or_insert(v);
    }
    let mut body = format!("{}\n", stmts.len());
    for (h, v) in &map {
        body.push_str(&format!("{h}\t{}\n", u8::from(*v)));
    }
    let path = format!("{LABELS_DIR}/{}.tsv.zst", dialect.dir_name());
    let raw = std::fs::File::create(&path).with_context(|| format!("create {path}"))?;
    let mut enc = zstd::stream::Encoder::new(raw, 19)?;
    enc.write_all(body.as_bytes())?;
    enc.finish()?;
    let n_valid = map.values().filter(|v| **v).count();
    eprintln!(
        "wrote {path}: {} unique statements, {n_valid} valid, {} invalid",
        map.len(),
        map.len() - n_valid
    );
    Ok(())
}

/// PostgreSQL: real server in a container. Each statement runs inside a rolled
/// back transaction (PG has transactional DDL); invalid iff the SQLSTATE is
/// `42601` (syntax_error). Schema errors (42P01, 42703) and "cannot run in a
/// transaction" (25xxx) are not syntax, so they count as valid (parsed fine).
async fn label_postgresql(stmts: &[String]) -> Result<Vec<bool>> {
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;
    use tokio_postgres::error::SqlState;
    use tokio_postgres::NoTls;

    let node = Postgres::default()
        .start()
        .await
        .context("start postgres container")?;
    let host = node.get_host().await?;
    let port = node.get_host_port_ipv4(5432).await?;
    let conn_str =
        format!("host={host} port={port} user=postgres password=postgres dbname=postgres");
    let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
        .await
        .context("connect postgres")?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let mut valid = Vec::with_capacity(stmts.len());
    for (i, s) in stmts.iter().enumerate() {
        // Make sure no aborted transaction is left from a prior error.
        let _ = client.batch_execute("ROLLBACK").await;
        let _ = client.batch_execute("BEGIN").await;
        let res = client.batch_execute(s).await;
        let _ = client.batch_execute("ROLLBACK").await;
        let v = match res {
            Ok(()) => true,
            Err(e) => e.code() != Some(&SqlState::SYNTAX_ERROR),
        };
        valid.push(v);
        if i % 2000 == 0 {
            eprintln!("  postgresql {i}/{}", stmts.len());
        }
    }
    Ok(valid)
}

/// MySQL: real server in a container. We use `PREPARE`, MySQL's parse-only path:
/// it parses (and name-resolves) without executing, so there are no side effects
/// and nothing blocks. Invalid iff `PREPARE` fails with error 1064
/// (ER_PARSE_ERROR); a missing table/column (1146/1054) or "unsupported in the
/// prepared-statement protocol" (1295) means it parsed, so it is valid.
async fn label_mysql(stmts: &[String]) -> Result<Vec<bool>> {
    use mysql_async::prelude::Queryable;
    use testcontainers_modules::mysql::Mysql;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;

    let node = Mysql::default()
        .start()
        .await
        .context("start mysql container")?;
    let host = node.get_host().await?;
    let port = node.get_host_port_ipv4(3306).await?;
    let url = format!("mysql://root@{host}:{port}/test");
    let pool = mysql_async::Pool::new(url.as_str());
    let mut conn = pool.get_conn().await.context("connect mysql")?;

    let mut valid = Vec::with_capacity(stmts.len());
    for (i, s) in stmts.iter().enumerate() {
        let stmt = s.trim().trim_end_matches(';');
        // Bind the statement text as a parameter (no injection), then PREPARE it.
        let v = match conn.exec_drop("SET @q = ?", (stmt,)).await {
            Ok(()) => match conn.query_drop("PREPARE _ck FROM @q").await {
                Ok(()) => {
                    let _ = conn.query_drop("DEALLOCATE PREPARE _ck").await;
                    true
                }
                Err(mysql_async::Error::Server(e)) => e.code != 1064,
                Err(_) => true,
            },
            Err(_) => true,
        };
        valid.push(v);
        if i % 2000 == 0 {
            eprintln!("  mysql {i}/{}", stmts.len());
        }
    }
    drop(conn);
    let _ = pool.disconnect().await;
    Ok(valid)
}

/// ClickHouse: real server in a container, queried over HTTP. `EXPLAIN AST`
/// parses only (no execution, no tables needed). Invalid iff the exception code
/// is 62 (SYNTAX_ERROR); any other code (unknown table/identifier, not
/// implemented) means it parsed, so it is valid.
async fn label_clickhouse(stmts: &[String]) -> Result<Vec<bool>> {
    use testcontainers_modules::clickhouse::ClickHouse;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;

    let node = ClickHouse::default()
        .start()
        .await
        .context("start clickhouse container")?;
    let host = node.get_host().await?;
    let port = node.get_host_port_ipv4(8123).await?;
    let url = format!("http://{host}:{port}/");
    let client = reqwest::Client::new();

    let mut valid = Vec::with_capacity(stmts.len());
    for (i, s) in stmts.iter().enumerate() {
        let query = format!("EXPLAIN AST {}", s.trim().trim_end_matches(';'));
        let v = match client.post(&url).body(query).send().await {
            Ok(resp) if resp.status().is_success() => true,
            Ok(resp) => {
                let code = resp
                    .headers()
                    .get("x-clickhouse-exception-code")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<i32>().ok());
                match code {
                    Some(62) => false,
                    Some(_) => true,
                    None => !resp.text().await.unwrap_or_default().contains("Code: 62."),
                }
            }
            Err(_) => true,
        };
        valid.push(v);
        if i % 5000 == 0 {
            eprintln!("  clickhouse {i}/{}", stmts.len());
        }
    }
    Ok(valid)
}

/// SQLite: real engine via the `sqlite3` CLI in a one-shot container. We feed a
/// script of `EXPLAIN <stmt>;` (compiles, does not execute, so no side effects)
/// and read stderr. `EXPLAIN` resolves names, so "no such table/column" surfaces
/// as a non-syntax error (valid); only a syntax error makes a statement invalid.
fn label_sqlite(stmts: &[String]) -> Result<Vec<bool>> {
    // Script line 1 is `.bail off`; statement i is on line i + 2.
    let mut script = String::from(".bail off\n");
    for s in stmts {
        script.push_str("EXPLAIN ");
        script.push_str(s.trim().trim_end_matches(';'));
        script.push_str(";\n");
    }

    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "--entrypoint",
            "sqlite3",
            "keinos/sqlite3",
            ":memory:",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("docker run sqlite3 (is the keinos/sqlite3 image pullable?)")?;

    // Write stdin from a thread so a full stderr pipe cannot deadlock us.
    let mut stdin = child.stdin.take().expect("piped stdin");
    let bytes = script.into_bytes();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&bytes);
    });
    let out = child.wait_with_output().context("sqlite3 wait")?;
    let _ = writer.join();
    let stderr = String::from_utf8_lossy(&out.stderr);

    let mut valid = vec![true; stmts.len()];
    for line in stderr.lines() {
        if let Some((lineno, msg)) = parse_sqlite_err(line) {
            if lineno >= 2 {
                let idx = lineno - 2;
                if idx < stmts.len() && is_sqlite_syntax(msg) {
                    valid[idx] = false;
                }
            }
        }
    }
    Ok(valid)
}

/// Parse a sqlite3 CLI error line like `Parse error near line 3: <msg>` into
/// `(line number, message)`.
fn parse_sqlite_err(line: &str) -> Option<(usize, &str)> {
    let rest = line.split_once("near line ")?.1;
    let (num, msg) = rest.split_once(':')?;
    Some((num.trim().parse().ok()?, msg.trim()))
}

/// Whether a sqlite3 error message denotes a syntax (parse) failure rather than
/// a semantic one (a missing table or column means it parsed fine).
fn is_sqlite_syntax(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("syntax error") || m.contains("incomplete input") || m.contains("unrecognized token")
}

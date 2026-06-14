//! Real database-engine reference oracle.
//!
//! For each reference dialect this brings up the actual engine in Docker, labels
//! every corpus statement valid/invalid by the schema-free rule (a syntax/parse
//! error means invalid. No error or a schema/semantic error means it parsed, so
//! valid), and writes a committed cache under `oracle/labels/{dir}.tsv.zst` that
//! `sql_ast_benchmark::oracle_cache` reads. Run locally with Docker:
//!
//!   cargo run --release -p oracle              # all implemented dialects
//!   cargo run --release -p oracle -- sqlite    # one or more by dir name
//!
//! Server engines (PostgreSQL, MySQL, ClickHouse, SQL Server) run in
//! testcontainers and connect over a mapped port. SQLite runs as the `sqlite3`
//! CLI in a one-shot container. DuckDB, which has no server and whose CLI errors
//! carry no line numbers, links the real `libduckdb` in-process via the `duckdb`
//! crate. Each uses the engine's parse-only path where one exists.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use sql_ast_benchmark::datasets::{ensure_corpus, Dialect};
use sql_ast_benchmark::oracle_cache::{statement_hash, LABELS_DIR};
use sql_ast_benchmark::report::load_dialect_from;

/// Dialects with an adapter implemented so far.
const IMPLEMENTED: &[&str] = &[
    "postgresql",
    "sqlite",
    "mysql",
    "clickhouse",
    "tsql",
    "duckdb",
];

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
            Dialect::Tsql => label_tsql(&stmts).await?,
            Dialect::Duckdb => label_duckdb(&stmts)?,
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
/// back transaction (PG has transactional DDL). Invalid iff the SQLSTATE is
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

    let mut valid = Vec::with_capacity(stmts.len());
    let mut reconnects = 0usize;
    // A statement that terminates the backend twice at the same index is a
    // confirmed "poison" (some pg_regress statements crash/kill the connection);
    // it is marked invalid and skipped, mirroring the ClickHouse handling.
    let mut death_idx: Option<usize> = None;
    let mut death_count = 0usize;

    'session: while valid.len() < stmts.len() {
        let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
            .await
            .context("connect postgres")?;
        tokio::spawn(async move {
            let _ = connection.await;
        });

        let mut unreachable_streak = 0usize;
        while valid.len() < stmts.len() {
            let i = valid.len();
            // Make sure no aborted transaction is left from a prior error.
            let _ = client.batch_execute("ROLLBACK").await;
            let _ = client.batch_execute("BEGIN").await;
            let res = client.batch_execute(&stmts[i]).await;
            let _ = client.batch_execute("ROLLBACK").await;
            // A verdict only counts if the server actually answered: an error with
            // a SQLSTATE is a real result (syntax -> invalid, anything else parsed
            // -> valid). An error with no code is a transport/connection failure,
            // which must never be recorded as "valid".
            let verdict = match res {
                Ok(()) => Some(true),
                Err(e) => e.code().map(|code| code != &SqlState::SYNTAX_ERROR),
            };
            match verdict {
                Some(v) => {
                    unreachable_streak = 0;
                    valid.push(v);
                    if i.is_multiple_of(2000) {
                        eprintln!("  postgresql {i}/{}", stmts.len());
                    }
                }
                None if is_copy_to_stdout(&stmts[i]) => {
                    // `COPY ... TO STDOUT` parses fine but then streams rows over
                    // the COPY sub-protocol, which `batch_execute` cannot consume,
                    // so it breaks the connection with no SQLSTATE. Reaching that
                    // stage proves it parsed (a syntax error would carry code
                    // 42601 and be a real verdict above), so it is valid. Record it
                    // and reconnect to replace the now-broken connection.
                    valid.push(true);
                    death_idx = None;
                    death_count = 0;
                    reconnects += 1;
                    anyhow::ensure!(
                        reconnects <= 50,
                        "postgres reconnected {reconnects} times (last at statement {i}); aborting without writing a label cache"
                    );
                    continue 'session;
                }
                None if unreachable_streak + 1 < 6 => {
                    unreachable_streak += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                None => {
                    // Connection is gone: the backend died (often killed by the
                    // statement itself). Reconnect and resume; if the same index
                    // kills it twice, treat that statement as poison.
                    if death_idx == Some(i) {
                        death_count += 1;
                    } else {
                        death_idx = Some(i);
                        death_count = 1;
                    }
                    if death_count >= 2 {
                        eprintln!(
                            "  postgresql: statement {i} repeatedly kills the backend; marking invalid and skipping: {}",
                            stmts[i].chars().take(120).collect::<String>()
                        );
                        valid.push(false);
                        death_idx = None;
                        death_count = 0;
                    } else {
                        eprintln!(
                            "  postgresql backend died at {i}/{}; reconnecting",
                            stmts.len()
                        );
                    }
                    reconnects += 1;
                    anyhow::ensure!(
                        reconnects <= 50,
                        "postgres backend crashed {reconnects} times (last at statement {i}); aborting without writing a label cache"
                    );
                    continue 'session;
                }
            }
        }
    }
    Ok(valid)
}

/// Whether a statement is `COPY ... TO STDOUT`: valid SQL whose result is streamed
/// over the COPY sub-protocol, which the simple-query probe cannot consume (it
/// breaks the connection with no SQLSTATE). A syntactically invalid COPY instead
/// returns a real syntax error, so this only matches genuinely-valid ones.
fn is_copy_to_stdout(stmt: &str) -> bool {
    let up = stmt.trim_start().to_ascii_uppercase();
    up.starts_with("COPY") && up.contains("TO STDOUT")
}

/// MySQL: real server in a container. We use `PREPARE`, MySQL's parse-only path:
/// it parses (and name-resolves) without executing, so there are no side effects
/// and nothing blocks. Invalid iff `PREPARE` fails with error 1064
/// (ER_PARSE_ERROR). A missing table/column (1146/1054) or "unsupported in the
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
    let mut unreachable_streak = 0usize;
    let mut i = 0;
    while i < stmts.len() {
        let stmt = stmts[i].trim().trim_end_matches(';');
        // Bind the statement text as a parameter (no injection), then PREPARE it.
        // Only a `Server` response is a real verdict (error 1064 = syntax ->
        // invalid, any other server error parsed -> valid). A non-server error is
        // a transport/connection failure and must never be recorded as "valid".
        let verdict = match conn.exec_drop("SET @q = ?", (stmt,)).await {
            Ok(()) => match conn.query_drop("PREPARE _ck FROM @q").await {
                Ok(()) => {
                    let _ = conn.query_drop("DEALLOCATE PREPARE _ck").await;
                    Some(true)
                }
                Err(mysql_async::Error::Server(e)) => Some(e.code != 1064),
                Err(_) => None,
            },
            Err(mysql_async::Error::Server(e)) => Some(e.code != 1064),
            Err(_) => None,
        };
        match verdict {
            Some(v) => {
                unreachable_streak = 0;
                valid.push(v);
                i += 1;
                if i.is_multiple_of(2000) {
                    eprintln!("  mysql {i}/{}", stmts.len());
                }
            }
            None => {
                unreachable_streak += 1;
                anyhow::ensure!(
                    unreachable_streak < 10,
                    "mysql became unreachable at statement {i}; aborting without writing a label cache"
                );
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    drop(conn);
    let _ = pool.disconnect().await;
    Ok(valid)
}

/// ClickHouse: real server in a container, queried over HTTP. `EXPLAIN AST`
/// parses only (no execution, no tables needed). Invalid iff the exception code
/// is 62 (SYNTAX_ERROR). Any other code (unknown table/identifier, not
/// implemented) means it parsed, so it is valid.
///
/// Hardened on two fronts:
///
///  * Correctness: every response body is fully consumed before the connection is
///    reused (the undrained error body was what desynced later responses and
///    silently mislabeled statements valid), transport blips are retried, and a
///    result that cannot be classified is never assumed valid.
///  * Resilience: the pinned ClickHouse image segfaults nondeterministically under
///    the sustained full-corpus load. When the engine stops responding the
///    container is restarted and labeling resumes from the same statement (each
///    `EXPLAIN AST` is independent, so a fresh engine yields identical verdicts).
///    A restart cap stops an unrecoverable engine from looping forever.
async fn label_clickhouse(stmts: &[String]) -> Result<Vec<bool>> {
    use testcontainers_modules::clickhouse::ClickHouse;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;

    let mut valid: Vec<bool> = Vec::with_capacity(stmts.len());
    let mut restarts = 0usize;
    let mut poisoned: Vec<usize> = Vec::new();
    // Track repeated deaths at one index: a statement that crashes the engine twice
    // in a row is a confirmed parser-crash ("poison") and is skipped as invalid.
    let mut death_idx: Option<usize> = None;
    let mut death_count = 0usize;

    'engine: while valid.len() < stmts.len() {
        let node = ClickHouse::default()
            .start()
            .await
            .context("start clickhouse container")?;
        let host = node.get_host().await?;
        let port = node.get_host_port_ipv4(8123).await?;
        let url = format!("http://{host}:{port}/");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("build clickhouse http client")?;

        let mut consecutive_unreachable = 0usize;
        while valid.len() < stmts.len() {
            let i = valid.len();
            let query = format!("EXPLAIN AST {}", stmts[i].trim().trim_end_matches(';'));
            match clickhouse_classify(&client, &url, &query).await {
                Some(v) => {
                    consecutive_unreachable = 0;
                    valid.push(v);
                    if i.is_multiple_of(5000) {
                        eprintln!("  clickhouse {i}/{}", stmts.len());
                    }
                }
                None if consecutive_unreachable + 1 < 6 => {
                    // A transient blip: wait and retry the SAME statement (do not
                    // advance, do not guess a verdict).
                    consecutive_unreachable += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                None => {
                    // Engine unreachable: it has crashed. Was the crash provoked by
                    // this exact statement (it died here last restart too)?
                    if death_idx == Some(i) {
                        death_count += 1;
                    } else {
                        death_idx = Some(i);
                        death_count = 1;
                    }
                    drop(node);
                    if death_count >= 2 {
                        eprintln!(
                            "  clickhouse: statement {i} repeatedly crashes the engine; marking invalid and skipping: {}",
                            stmts[i].chars().take(120).collect::<String>()
                        );
                        valid.push(false);
                        poisoned.push(i);
                        death_idx = None;
                        death_count = 0;
                    } else {
                        eprintln!(
                            "  clickhouse unreachable at {i}/{}; restarting engine",
                            stmts.len()
                        );
                    }
                    restarts += 1;
                    anyhow::ensure!(
                        restarts <= 50,
                        "ClickHouse crashed {restarts} times (last at statement {i}); aborting without writing a label cache"
                    );
                    continue 'engine;
                }
            }
        }
    }
    if !poisoned.is_empty() {
        eprintln!(
            "  clickhouse: {} statement(s) crashed the engine and were marked invalid (indices: {:?})",
            poisoned.len(),
            poisoned
        );
    }
    Ok(valid)
}

/// Classify one ClickHouse `EXPLAIN AST` request, retrying transient transport
/// failures. The response body is always fully read before returning, so a
/// connection is never left mid-stream (the bug that desynced reused connections).
/// `Some(true)` if the request succeeded (2xx) or failed with a non-syntax
/// exception code (the statement parsed, the engine just could not resolve or
/// execute it); `Some(false)` for exception code 62 (`SYNTAX_ERROR`) or an
/// unclassifiable response; `None` if the engine was unreachable after retries.
async fn clickhouse_classify(client: &reqwest::Client, url: &str, query: &str) -> Option<bool> {
    for attempt in 0..3 {
        match client.post(url).body(query.to_string()).send().await {
            Ok(resp) => {
                let success = resp.status().is_success();
                let header_code = resp
                    .headers()
                    .get("x-clickhouse-exception-code")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<i32>().ok());
                let body = resp.text().await.unwrap_or_default();
                if success {
                    return Some(true);
                }
                return Some(match header_code.or_else(|| parse_clickhouse_code(&body)) {
                    Some(62) => false,
                    Some(_) => true,
                    None => false,
                });
            }
            Err(_) if attempt < 2 => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(_) => return None,
        }
    }
    None
}

/// Parse the leading exception code from a ClickHouse error body, e.g.
/// `"Code: 62. DB::Exception: ..."` -> `Some(62)`.
fn parse_clickhouse_code(body: &str) -> Option<i32> {
    let digits: String = body
        .trim_start()
        .strip_prefix("Code:")?
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// SQL Server (T-SQL): real server in a container. `SET PARSEONLY ON` parses
/// every batch without compiling or executing it (and without resolving object
/// names), so the only errors that can surface are syntax errors. A statement is
/// therefore valid iff it runs without error.
async fn label_tsql(stmts: &[String]) -> Result<Vec<bool>> {
    use testcontainers_modules::mssql_server::MssqlServer;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;
    use tiberius::{AuthMethod, Client, Config};
    use tokio_util::compat::TokioAsyncWriteCompatExt;

    let node = MssqlServer::default()
        .with_accept_eula()
        .start()
        .await
        .context("start sql server container")?;
    let host = node.get_host().await?;
    let port = node.get_host_port_ipv4(1433).await?;

    let mut config = Config::new();
    config.host(host.to_string());
    config.port(port);
    config.authentication(AuthMethod::sql_server(
        "sa",
        MssqlServer::DEFAULT_SA_PASSWORD,
    ));
    config.trust_cert();
    let tcp = tokio::net::TcpStream::connect(config.get_addr())
        .await
        .context("tcp connect sql server")?;
    tcp.set_nodelay(true)?;
    let mut client = Client::connect(config, tcp.compat_write())
        .await
        .context("connect sql server")?;
    // Parse-only for the rest of the session: only syntax errors will surface.
    client
        .simple_query("SET PARSEONLY ON")
        .await?
        .into_results()
        .await?;

    let mut valid = Vec::with_capacity(stmts.len());
    let mut unreachable_streak = 0usize;
    let mut i = 0;
    while i < stmts.len() {
        // Under PARSEONLY the only `Server` error is a syntax error (invalid). A
        // non-server error is a transport/connection failure: never record it as a
        // verdict, retry, and abort if the engine stays unreachable.
        let verdict = match client.simple_query(stmts[i].as_str()).await {
            Ok(stream) => match stream.into_results().await {
                Ok(_) => Some(true),
                Err(tiberius::error::Error::Server(_)) => Some(false),
                Err(_) => None,
            },
            Err(tiberius::error::Error::Server(_)) => Some(false),
            Err(_) => None,
        };
        match verdict {
            Some(v) => {
                unreachable_streak = 0;
                valid.push(v);
                i += 1;
                if i.is_multiple_of(2000) {
                    eprintln!("  tsql {i}/{}", stmts.len());
                }
            }
            None => {
                unreachable_streak += 1;
                anyhow::ensure!(
                    unreachable_streak < 10,
                    "sql server became unreachable at statement {i}; aborting without writing a label cache"
                );
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    Ok(valid)
}

/// DuckDB: real engine via the in-process `duckdb` crate (the actual libduckdb).
/// DuckDB has no server, and its CLI errors carry no line numbers (so the
/// container batch-correlation used for SQLite is unreliable), so we link the
/// real engine directly. `prepare` parses and binds without executing. A
/// "Parser Error" is a syntax error (invalid), while a "Binder"/"Catalog Error"
/// (unknown table or column) means it parsed, so it is valid.
fn label_duckdb(stmts: &[String]) -> Result<Vec<bool>> {
    let conn = duckdb::Connection::open_in_memory().context("open duckdb")?;
    let mut valid = Vec::with_capacity(stmts.len());
    for (i, s) in stmts.iter().enumerate() {
        let stmt = s.trim().trim_end_matches(';');
        let v = match conn.prepare(stmt) {
            Ok(_) => true,
            Err(e) => !e.to_string().contains("Parser Error"),
        };
        valid.push(v);
        if i % 5000 == 0 {
            eprintln!("  duckdb {i}/{}", stmts.len());
        }
    }
    Ok(valid)
}

/// SQLite: real engine via the `sqlite3` CLI in a one-shot container. We feed a
/// script of `EXPLAIN <stmt>;` (compiles, does not execute, so no side effects)
/// and read stderr. `EXPLAIN` resolves names, so "no such table/column" surfaces
/// as a non-syntax error (valid). Only a syntax error makes a statement invalid.
fn label_sqlite(stmts: &[String]) -> Result<Vec<bool>> {
    // Script line 1 is `.bail off`. Statement i is on line i + 2.
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
            // Pinned by digest for reproducible labels: this is keinos/sqlite3
            // latest at label time, SQLite 3.53.0. The repo publishes no version
            // tag for it, and a floating `latest` would silently change the SQLite
            // version (and thus which statements are valid). Bump deliberately,
            // then regenerate the SQLite cache.
            "keinos/sqlite3@sha256:252363ef3cbbe11f1100dcbc734b89969b264df99a49008b34ca4578f503ff2a",
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

    // sqlite3 normally exits 0 (clean) or 1 (some statement errored, `.bail off`
    // keeps going). A crash or container failure surfaces as the container exit
    // code >= 128 (128 + signal, e.g. 139 = SIGSEGV) or a docker error (125-127),
    // or no code at all. In those cases the script stopped early, so the unscanned
    // tail would silently default to "valid" -- abort instead of writing garbage.
    match out.status.code() {
        Some(0 | 1) => {}
        other => anyhow::bail!(
            "sqlite3 ended abnormally (exit {other:?}); a statement likely crashed the CLI. Aborting without writing a label cache"
        ),
    }
    let stderr = String::from_utf8_lossy(&out.stderr);

    let mut valid = vec![true; stmts.len()];
    for line in stderr.lines() {
        if let Some((lineno, msg)) = parse_sqlite_err(line) {
            if lineno >= 2 {
                let idx = lineno - 2;
                if idx < stmts.len() && is_sqlite_invalid(msg) {
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

/// Whether a sqlite3 prepare error marks the statement invalid.
///
/// `EXPLAIN` resolves names, so the only errors that mean "it parsed fine, it
/// just references objects we did not create" are missing-object and binding
/// errors (no such table/column/function/..., ambiguous column). Every other
/// prepare error is a real rejection: not only "syntax error" but the
/// grammar/semantic errors SQLite reports regardless of schema, such as
/// "ORDER BY clause should come after INTERSECT not before" (gwenn/lemon-rs#102)
/// or "RIGHT and FULL OUTER JOINs are not currently supported". The previous
/// allow-list of a few syntax phrases mislabeled all of those as valid.
fn is_sqlite_invalid(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    // Missing-object / binding errors mean the statement parsed and only
    // references objects we did not create (a table, column, function,
    // collation, module, ..., or an attached database alias). Everything else
    // that errors is a real parse or grammar rejection.
    !(m.contains("no such") || m.contains("ambiguous column") || m.contains("unknown database"))
}

#[cfg(test)]
mod tests {
    use super::{is_copy_to_stdout, is_sqlite_invalid, parse_clickhouse_code, parse_sqlite_err};

    #[test]
    fn copy_to_stdout_is_recognized() {
        assert!(is_copy_to_stdout("COPY (SELECT 1) TO STDOUT"));
        assert!(is_copy_to_stdout("copy (select 1) to stdout"));
        assert!(is_copy_to_stdout("COPY (SELECT 1) TO STDOUT WITH CSV"));
        assert!(is_copy_to_stdout("  COPY t TO STDOUT"));
        // Not COPY-to-stdout: a real syntax verdict handles these, or they differ.
        assert!(!is_copy_to_stdout("SELECT 'COPY x TO STDOUT'"));
        assert!(!is_copy_to_stdout("COPY t FROM STDIN"));
        assert!(!is_copy_to_stdout("SELECT 1"));
    }

    #[test]
    fn parse_clickhouse_code_reads_leading_code() {
        assert_eq!(
            parse_clickhouse_code("Code: 62. DB::Exception: Syntax error: ..."),
            Some(62)
        );
        assert_eq!(
            parse_clickhouse_code("Code: 47. DB::Exception: Unknown identifier"),
            Some(47)
        );
        assert_eq!(parse_clickhouse_code("  Code: 999. foo"), Some(999));
        // No parseable code -> None, which the caller treats as invalid.
        assert_eq!(parse_clickhouse_code("totally unexpected body"), None);
        assert_eq!(parse_clickhouse_code(""), None);
    }

    #[test]
    fn missing_object_errors_are_valid() {
        // The statement parsed. It only references objects we did not create.
        assert!(!is_sqlite_invalid("no such table: documents"));
        assert!(!is_sqlite_invalid("no such column: x"));
        assert!(!is_sqlite_invalid("no such function: my_udf"));
        assert!(!is_sqlite_invalid("ambiguous column name: id"));
        // An attached-database alias that is not attached (e.g. CREATE TABLE
        // db2.t(x) without ATTACH) parsed fine, the alias is just absent.
        assert!(!is_sqlite_invalid("unknown database db2"));
    }

    #[test]
    fn parse_and_grammar_errors_are_invalid() {
        assert!(is_sqlite_invalid("near \"FROM\": syntax error"));
        assert!(is_sqlite_invalid("incomplete input"));
        // The errors the old allow-list missed (gwenn/lemon-rs#102).
        assert!(is_sqlite_invalid(
            "ORDER BY clause should come after INTERSECT not before"
        ));
        assert!(is_sqlite_invalid(
            "RIGHT and FULL OUTER JOINs are not currently supported"
        ));
    }

    #[test]
    fn parse_sqlite_err_extracts_line_and_message() {
        assert_eq!(
            parse_sqlite_err("Parse error near line 3: no such table: t"),
            Some((3, "no such table: t"))
        );
        assert_eq!(parse_sqlite_err("just some output"), None);
    }
}

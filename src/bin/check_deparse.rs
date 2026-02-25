#![allow(
    clippy::bool_to_int_with_if,
    clippy::doc_markdown,
    clippy::uninlined_format_args
)]

/// Identifies SQL statements that cause pg_query::deparse() to abort at the C level.
///
/// The libpg_query deparser contains C-level Assert() calls that invoke abort() on
/// unsupported AST node types in type modifiers (postgres_deparse.c:4480). This crash
/// cannot be caught with std::panic::catch_unwind — it is a SIGABRT, not a Rust panic.
///
/// This binary tests each statement from the scraped test corpus by running deparse()
/// in an isolated subprocess. The parent inspects the child's exit status:
///   - exit 0   → deparse succeeded
///   - exit 1   → parse or deparse returned Err (clean failure)
///   - signal 6 → SIGABRT: the C assert fired; statement is recorded as problematic
///
/// Usage:
///   cargo run --bin check_deparse
///
/// Requires: run `cargo run --bin scrape_tests` first.
use std::env;
#[cfg(feature = "pg_query_parser")]
use std::io::{self, Read, Write};
#[cfg(feature = "pg_query_parser")]
use std::process::{Command, Stdio};

#[cfg(all(unix, feature = "pg_query_parser"))]
use std::os::unix::process::ExitStatusExt;

// ── Subprocess worker mode ────────────────────────────────────────────────────

#[cfg(feature = "pg_query_parser")]
fn worker_mode() -> ! {
    // Read SQL from stdin, attempt parse + deparse, exit 0/1 cleanly.
    // If the C deparser aborts, the process is killed by SIGABRT — the parent detects that.
    let mut sql = String::new();
    io::stdin().read_to_string(&mut sql).expect("read stdin");
    let sql = sql.trim();

    let ok = pg_query::parse(sql)
        .ok()
        .and_then(|r| r.deparse().ok())
        .is_some();

    std::process::exit(if ok { 0 } else { 1 });
}

// ── Scanner mode (default) ────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("--worker") {
        #[cfg(feature = "pg_query_parser")]
        worker_mode();
        #[cfg(not(feature = "pg_query_parser"))]
        std::process::exit(1);
    }

    #[cfg(not(feature = "pg_query_parser"))]
    {
        eprintln!("ERROR: pg_query_parser feature required. Run: cargo run --bin check_deparse");
        std::process::exit(1);
    }

    #[cfg(feature = "pg_query_parser")]
    {
        let exe = env::current_exe().expect("cannot locate current executable");

        let test_files: &[(&str, &str)] = &[
            ("sqlparser_test_postgres.txt", "PostgreSQL-specific"),
            ("sqlparser_test_common.txt", "Common"),
            ("sqlparser_test_regression.txt", "Regression / TPC-H"),
        ];

        let mut total = 0usize;
        let mut deparse_failed = 0usize;
        let mut aborted: Vec<(String, String)> = Vec::new(); // (file_label, sql)

        for (file, label) in test_files {
            let path = std::path::Path::new(file);
            if !path.exists() {
                eprintln!("[skip] {file} not found — run `cargo run --bin scrape_tests` first");
                continue;
            }
            let content = std::fs::read_to_string(path).expect("read file");
            let stmts: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

            eprint!("Testing {label} ({} stmts)...", stmts.len());
            let mut file_aborts = 0usize;

            for sql in &stmts {
                total += 1;

                let mut child = Command::new(&exe)
                    .arg("--worker")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn worker");

                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(sql.as_bytes())
                    .expect("write stdin");
                drop(child.stdin.take());

                let status = child.wait().expect("wait child");

                #[cfg(unix)]
                {
                    if status.signal().is_some() {
                        // Killed by a signal (SIGABRT from C assert)
                        aborted.push((label.to_string(), sql.to_string()));
                        file_aborts += 1;
                    } else if status.code() == Some(0) {
                        // deparse succeeded
                    } else {
                        // parse rejected or deparse returned Err
                        deparse_failed += 1;
                    }
                }

                #[cfg(not(unix))]
                {
                    if !status.success() {
                        deparse_failed += 1;
                    }
                }
            }

            eprintln!(" {} aborts", file_aborts);
        }

        // Count pg_query-rejected statements (those where parse itself fails)
        // This is a subset of deparse_failed but we don't distinguish here.

        println!("\n══════════════════════════════════════════════════════");
        println!("  pg_query deparse() abort report");
        println!("══════════════════════════════════════════════════════");
        println!("  Total statements tested : {total}");
        println!("  Aborted (SIGABRT)       : {}", aborted.len());
        println!("  Failed cleanly          : {deparse_failed}");
        println!("══════════════════════════════════════════════════════");

        if aborted.is_empty() {
            println!("\n  No statements caused a C-level abort.");
        } else {
            println!("\n  Statements that trigger C Assert(false) in deparseTypeName:");
            for (label, sql) in &aborted {
                println!("\n  [{label}]");
                println!("    {sql}");
            }
        }
    }
}

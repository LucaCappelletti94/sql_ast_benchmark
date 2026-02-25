#![allow(clippy::doc_markdown)]

/// Shows the specific SQL statements where sqlparser-rs and pg_query.rs disagree,
/// using pg_query.rs (libpg_query) as the PostgreSQL ground truth.
///
/// Two categories are printed per test file:
///
///   sqlparser-rs accepts, pg_query rejects:
///     sqlparser-rs is over-permissive — it accepts syntax that real PostgreSQL
///     would not. These are sqlparser-rs false positives / PostgreSQL extensions.
///
///   pg_query accepts, sqlparser-rs rejects:
///     sqlparser-rs is missing coverage — real PostgreSQL accepts this SQL but
///     sqlparser-rs (PostgreSQL dialect) cannot parse it.
///
/// Requires the default `pg_query_parser` feature:
///   cargo run --bin divergence
///
/// Run scrape_tests first:
///   cargo run --bin scrape_tests
use sql_ast_benchmark::{is_valid_pg_query, is_valid_sqlparser};
use std::fs;
use std::path::Path;

fn analyze_file(path: &Path, label: &str) {
    let Ok(content) = fs::read_to_string(path) else {
        eprintln!(
            "  [skip] {} — run `cargo run --bin scrape_tests` first",
            path.display()
        );
        return;
    };

    let all: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if all.is_empty() {
        println!("\n[{label}] — empty");
        return;
    }

    let mut sqlparser_only: Vec<&str> = Vec::new(); // sqlparser accepts, pg_query rejects
    let mut pg_query_only: Vec<&str> = Vec::new(); // pg_query accepts, sqlparser rejects

    for sql in &all {
        let sp = is_valid_sqlparser(sql);
        let pg = is_valid_pg_query(sql);
        match (sp, pg) {
            (true, false) => sqlparser_only.push(sql),
            (false, true) => pg_query_only.push(sql),
            _ => {}
        }
    }

    println!("\n╔══ {label} ({} statements total) ══", all.len());

    // ── sqlparser-rs accepts, pg_query rejects ────────────────────────────────
    println!("║");
    println!(
        "║  sqlparser-rs ACCEPTS but pg_query REJECTS ({} cases)",
        sqlparser_only.len()
    );
    println!("║  (sqlparser-rs is more permissive than real PostgreSQL here)");
    println!("║");
    if sqlparser_only.is_empty() {
        println!("║  (none)");
    } else {
        for (i, sql) in sqlparser_only.iter().enumerate() {
            println!("║  [{:>3}] {}", i + 1, sql);
        }
    }

    // ── pg_query accepts, sqlparser-rs rejects ────────────────────────────────
    println!("║");
    println!(
        "║  pg_query ACCEPTS but sqlparser-rs REJECTS ({} cases)",
        pg_query_only.len()
    );
    println!("║  (sqlparser-rs is missing PostgreSQL coverage here)");
    println!("║");
    if pg_query_only.is_empty() {
        println!("║  (none)");
    } else {
        for (i, sql) in pg_query_only.iter().enumerate() {
            println!("║  [{:>3}] {}", i + 1, sql);
        }
    }

    println!("╚══");
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));

    println!("sqlparser-rs  ↔  pg_query.rs (libpg_query) divergence report");
    println!("Source: sqlparser-rs test suite (run scrape_tests to refresh)\n");

    let files: &[(&str, &str)] = &[
        ("sqlparser_test_postgres.txt", "PostgreSQL-specific tests"),
        ("sqlparser_test_common.txt", "Common (all-dialect) tests"),
        ("sqlparser_test_regression.txt", "Regression / TPC-H tests"),
        ("spider_select.txt", "Spider dataset (real-world SELECT)"),
        ("gretel_select.txt", "Gretel dataset (synthetic SELECT)"),
        ("gretel_insert.txt", "Gretel dataset (INSERT)"),
        ("gretel_update.txt", "Gretel dataset (UPDATE)"),
        ("gretel_delete.txt", "Gretel dataset (DELETE)"),
    ];

    for (file, label) in files {
        analyze_file(Path::new(file), label);
    }
}

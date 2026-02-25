#![allow(
    clippy::doc_markdown,
    clippy::manual_checked_ops,
    clippy::print_literal,
    clippy::redundant_closure
)]

/// Correctness benchmark: tests each SQL parser against SQL statements
/// extracted from the sqlparser-rs test suite (produced by `scrape_tests`).
///
/// Ground truth: pg_query.rs (libpg_query) — the actual PostgreSQL parser.
///   Valid   = accepted by pg_query.rs
///   Invalid = rejected by pg_query.rs
///
/// Three metrics per parser:
///
///   Recall (true-positive rate):
///     Of SQL that pg_query.rs accepts, how many does this parser also accept?
///     Higher is better.
///
///   False-positive rate:
///     Of SQL that pg_query.rs rejects, how many does this parser wrongly accept?
///     Lower is better.
///
///   Round-trip stability:
///     Of valid SQL that this parser accepts, does parse → print → re-parse → re-print
///     produce stable output? (N/A for parsers without a pretty-printer)
///     Higher is better.
///
/// Requires the default `pg_query_parser` feature:
///   cargo run --bin correctness
///
/// Run scrape_tests first:
///   cargo run --bin scrape_tests
use sql_ast_benchmark::{
    databend_roundtrip, is_valid_databend, is_valid_polyglot, is_valid_sql_parse,
    is_valid_sqlparser, polyglot_roundtrip, sqlparser_roundtrip,
};
use std::fs;
use std::path::Path;

#[cfg(feature = "pg_query_parser")]
use sql_ast_benchmark::{
    databend_fidelity, is_valid_pg_query, is_valid_pg_query_summary, pg_query_roundtrip,
    polyglot_fidelity, sqlparser_fidelity,
};

// ── Per-parser counts ─────────────────────────────────────────────────────────

struct ParserCounts {
    /// Correctly accepted (true positives): parser accepts pg_query-valid SQL
    accepted: usize,
    /// Wrongly accepted (false positives): parser accepts pg_query-invalid SQL
    false_pos: usize,
    /// Of `accepted`, how many also round-trip stably; None = no pretty-printer
    roundtrip: Option<usize>,
    /// Of `accepted`, how many produce output with the same pg_query canonical
    /// form as the original; None = no pretty-printer or fidelity check N/A
    fidelity: Option<usize>,
}

struct Counts {
    extracted: usize,
    /// Accepted by pg_query.rs — the PostgreSQL ground truth
    valid_total: usize,
    /// Rejected by pg_query.rs
    invalid_total: usize,
    /// pg_query baseline round-trip count
    #[cfg(feature = "pg_query_parser")]
    pg_query_rt: usize,

    sqlparser: ParserCounts,
    polyglot: ParserCounts,
    databend: ParserCounts,
    sql_parse: ParserCounts,
    #[cfg(feature = "pg_query_parser")]
    pg_query_summary: ParserCounts,
}

fn count_for(accept: impl Fn(&str) -> bool, valid: &[&str], invalid: &[&str]) -> ParserCounts {
    ParserCounts {
        accepted: valid.iter().filter(|s| accept(s)).count(),
        false_pos: invalid.iter().filter(|s| accept(s)).count(),
        roundtrip: None,
        fidelity: None,
    }
}

fn with_roundtrip(mut pc: ParserCounts, rt: impl Fn(&str) -> bool, valid: &[&str]) -> ParserCounts {
    pc.roundtrip = Some(valid.iter().filter(|s| rt(s)).count());
    pc
}

fn with_fidelity(mut pc: ParserCounts, fi: impl Fn(&str) -> bool, valid: &[&str]) -> ParserCounts {
    pc.fidelity = Some(valid.iter().filter(|s| fi(s)).count());
    pc
}

#[cfg(feature = "pg_query_parser")]
fn check_file(path: &Path) -> Option<Counts> {
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    let all: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let extracted = all.len();
    if extracted == 0 {
        return None;
    }

    // Ground truth: split by pg_query.rs (libpg_query = the actual PostgreSQL parser)
    let valid: Vec<&str> = all
        .iter()
        .copied()
        .filter(|s| is_valid_pg_query(s))
        .collect();
    let invalid: Vec<&str> = all
        .iter()
        .copied()
        .filter(|s| !is_valid_pg_query(s))
        .collect();

    let pg_query_rt = valid.iter().filter(|s| pg_query_roundtrip(s)).count();

    Some(Counts {
        extracted,
        valid_total: valid.len(),
        invalid_total: invalid.len(),
        pg_query_rt,
        sqlparser: with_fidelity(
            with_roundtrip(
                count_for(|s| is_valid_sqlparser(s), &valid, &invalid),
                |s| sqlparser_roundtrip(s),
                &valid,
            ),
            |s| sqlparser_fidelity(s),
            &valid,
        ),
        polyglot: with_fidelity(
            with_roundtrip(
                count_for(|s| is_valid_polyglot(s), &valid, &invalid),
                |s| polyglot_roundtrip(s),
                &valid,
            ),
            |s| polyglot_fidelity(s),
            &valid,
        ),
        databend: with_fidelity(
            with_roundtrip(
                count_for(|s| is_valid_databend(s), &valid, &invalid),
                |s| databend_roundtrip(s),
                &valid,
            ),
            |s| databend_fidelity(s),
            &valid,
        ),
        sql_parse: count_for(|s| is_valid_sql_parse(s), &valid, &invalid),
        pg_query_summary: count_for(|s| is_valid_pg_query_summary(s), &valid, &invalid),
    })
}

// ── Formatting ────────────────────────────────────────────────────────────────

fn pct(n: usize, base: usize) -> f64 {
    if base == 0 {
        0.0
    } else {
        100.0 * n as f64 / base as f64
    }
}

fn bar(n: usize, base: usize, width: usize) -> String {
    let filled = if base == 0 {
        0
    } else {
        (n * width / base).min(width)
    };
    format!("[{}{}]", "█".repeat(filled), "░".repeat(width - filled))
}

fn print_recall_row(label: &str, accepted: usize, base: usize) {
    println!(
        "│  {:<24} {:>6}/{:<6}  {:>6.1}%  {}",
        label,
        accepted,
        base,
        pct(accepted, base),
        bar(accepted, base, 30)
    );
}

fn print_fp_row(label: &str, pc: &ParserCounts, base: usize) {
    println!(
        "│  {:<24} {:>6}/{:<6}  {:>6.1}%  {}",
        label,
        pc.false_pos,
        base,
        pct(pc.false_pos, base),
        bar(pc.false_pos, base, 30)
    );
}

fn print_rt_row(label: &str, pc: &ParserCounts) {
    match pc.roundtrip {
        Some(rt) => println!(
            "│  {:<24} {:>6}/{:<6}  {:>6.1}%  {}",
            label,
            rt,
            pc.accepted,
            pct(rt, pc.accepted),
            bar(rt, pc.accepted, 30)
        ),
        None => println!("│  {:<24}  {:>38}", label, "N/A (no pretty-printer)"),
    }
}

fn print_fidelity_row(label: &str, pc: &ParserCounts) {
    match pc.fidelity {
        Some(fi) => println!(
            "│  {:<24} {:>6}/{:<6}  {:>6.1}%  {}",
            label,
            fi,
            pc.accepted,
            pct(fi, pc.accepted),
            bar(fi, pc.accepted, 30)
        ),
        None => println!("│  {:<24}  {:>38}", label, "N/A (no pretty-printer)"),
    }
}

fn print_table_header(col2: &str) {
    println!(
        "│  {:<24} {:>13}  {:>7}  {}",
        "Parser", col2, "Score", "Visual (30 cols)"
    );
    println!(
        "│  {:<24} {:>13}  {:>7}  {}",
        "──────────────────────", "─────────────", "───────", "──────────────────────────────"
    );
}

#[cfg(feature = "pg_query_parser")]
fn print_section(label: &str, c: &Counts) {
    println!("\n┌─ {label}");
    println!(
        "│  Extracted: {}  │  Valid (pg_query accepts): {}  │  Invalid (pg_query rejects): {}",
        c.extracted, c.valid_total, c.invalid_total
    );

    // ── Recall ───────────────────────────────────────────────────────────────
    println!("│");
    println!(
        "│  RECALL — accepted out of {} valid stmts  (↑ higher is better)",
        c.valid_total
    );
    print_table_header("Accepted");
    print_recall_row("pg_query.rs (baseline)", c.valid_total, c.valid_total);
    print_recall_row(
        "pg_query (summary)",
        c.pg_query_summary.accepted,
        c.valid_total,
    );
    print_recall_row("sqlparser-rs", c.sqlparser.accepted, c.valid_total);
    print_recall_row("polyglot-sql", c.polyglot.accepted, c.valid_total);
    print_recall_row("databend-common-ast", c.databend.accepted, c.valid_total);
    print_recall_row("sql-parse", c.sql_parse.accepted, c.valid_total);

    // ── False positives ───────────────────────────────────────────────────────
    if c.invalid_total > 0 {
        println!("│");
        println!(
            "│  FALSE POSITIVES — wrongly accepted out of {} invalid stmts  (↓ lower is better)",
            c.invalid_total
        );
        print_table_header("Wrong+");
        print_recall_row("pg_query.rs (baseline)", 0, c.invalid_total);
        print_fp_row("pg_query (summary)", &c.pg_query_summary, c.invalid_total);
        print_fp_row("sqlparser-rs", &c.sqlparser, c.invalid_total);
        print_fp_row("polyglot-sql", &c.polyglot, c.invalid_total);
        print_fp_row("databend-common-ast", &c.databend, c.invalid_total);
        print_fp_row("sql-parse", &c.sql_parse, c.invalid_total);
    }

    // ── Round-trip ────────────────────────────────────────────────────────────
    println!("│");
    println!(
        "│  ROUND-TRIP — parse→print→reparse→reprint stable, of accepted  (↑ higher is better)"
    );
    print_table_header("Stable");
    println!(
        "│  {:<24} {:>6}/{:<6}  {:>6.1}%  {}",
        "pg_query.rs (baseline)",
        c.pg_query_rt,
        c.valid_total,
        pct(c.pg_query_rt, c.valid_total),
        bar(c.pg_query_rt, c.valid_total, 30)
    );
    println!(
        "│  {:<24}  {:>38}",
        "pg_query (summary)", "N/A (summary, not full parse)"
    );
    print_rt_row("sqlparser-rs", &c.sqlparser);
    print_rt_row("polyglot-sql", &c.polyglot);
    print_rt_row("databend-common-ast", &c.databend);
    println!("│  {:<24}  {:>38}", "sql-parse", "N/A (no pretty-printer)");

    // ── Fidelity ──────────────────────────────────────────────────────────────
    println!("│");
    println!("│  FIDELITY — pg_query canonical of output matches canonical of input, of accepted  (↑ higher is better)");
    print_table_header("Faithful");
    println!(
        "│  {:<24}  {:>38}",
        "pg_query.rs (baseline)", "100% by definition"
    );
    println!(
        "│  {:<24}  {:>38}",
        "pg_query (summary)", "N/A (summary, not full parse)"
    );
    print_fidelity_row("sqlparser-rs", &c.sqlparser);
    print_fidelity_row("polyglot-sql", &c.polyglot);
    print_fidelity_row("databend-common-ast", &c.databend);
    println!("│  {:<24}  {:>38}", "sql-parse", "N/A (no pretty-printer)");

    println!("└─");
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    // Some parsers panic on edge-case SQL instead of returning Err.
    // The is_valid_* and *_roundtrip functions use catch_unwind; suppress noise.
    std::panic::set_hook(Box::new(|_| {}));

    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║           SQL Parser Correctness Report                             ║");
    println!("║  Ground truth: pg_query.rs (libpg_query = real PostgreSQL)         ║");
    println!("║  Metrics: Recall · False-positive rate · Round-trip stability      ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    #[cfg(not(feature = "pg_query_parser"))]
    {
        eprintln!("ERROR: pg_query_parser feature is required for the correctness benchmark.");
        eprintln!("Run: cargo run --bin correctness   (pg_query_parser is enabled by default)");
        std::process::exit(1);
    }

    #[cfg(feature = "pg_query_parser")]
    {
        let files: &[(&str, &str)] = &[
            ("sqlparser_test_postgres.txt", "PostgreSQL-specific tests"),
            ("sqlparser_test_common.txt", "Common (all-dialect) tests"),
            ("sqlparser_test_regression.txt", "Regression / TPC-H tests"),
        ];

        let mut any = false;
        for (file, label) in files {
            match check_file(Path::new(file)) {
                Some(counts) => {
                    print_section(label, &counts);
                    any = true;
                }
                None => eprintln!("\n  [skip] {file} — run `cargo run --bin scrape_tests` first"),
            }
        }

        if !any {
            eprintln!("\nNo test files found. Run `cargo run --bin scrape_tests` first.");
            std::process::exit(1);
        }
    }
}

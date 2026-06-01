#![allow(
    clippy::too_many_lines,
    clippy::print_literal,
    clippy::needless_collect
)]

//! `sqlbench`: the multi-dialect SQL parser benchmark CLI.
//!
//! Subcommands:
//!   correctness [--per-file]   grade parsers over `datasets/` (reference where one
//!                              exists, acceptance rate otherwise). `--per-file`
//!                              prints the per-dataset acceptance matrix instead
//!                              of per-dialect reference metrics.
//!   export                     write `web/assets/bench.json` for the explorer.
//!
//! The grading logic lives in the library (`report`). This binary is argument
//! dispatch plus table formatting.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::report::{self, DialectReport};
use sql_ast_benchmark::{export, BenchParser};

/// Reference-backed dialects first, then the provenance dialects.
const ORDER: [Dialect; 13] = [
    Dialect::Postgresql,
    Dialect::Sqlite,
    Dialect::Mysql,
    Dialect::Clickhouse,
    Dialect::Duckdb,
    Dialect::Hive,
    Dialect::SparkSql,
    Dialect::Trino,
    Dialect::Tsql,
    Dialect::Oracle,
    Dialect::Bigquery,
    Dialect::Redshift,
    Dialect::Multi,
];

fn pct(n: usize, base: usize) -> f64 {
    if base == 0 {
        0.0
    } else {
        100.0 * n as f64 / base as f64
    }
}

fn cell(v: f64) -> String {
    format!("{v:>6.1}%")
}
const NA: &str = "   N/A";

// correctness (per-dialect reference + provenance).

fn print_report(r: &DialectReport) {
    println!("\n=== {} ===", r.dialect.dir_name());
    let nw = r
        .parsers
        .iter()
        .map(|p| p.name().len())
        .max()
        .unwrap_or(22)
        .max(22);

    if r.has_reference {
        let reference = if r.dialect == Dialect::Sqlite {
            "sqlite3-parser (lemon-rs)"
        } else {
            "pg_query (libpg_query)"
        };
        println!(
            "Reference: {}   valid: {}   invalid: {}",
            reference, r.valid_total, r.invalid_total
        );
        println!(
            "{:<nw$}  {:>7}  {:>7}  {:>7}  {:>8}",
            "parser", "Recall", "FalseP", "RTrip", "Fidelity"
        );
        println!("{}", "-".repeat(nw + 2 + 7 + 2 + 7 + 2 + 7 + 2 + 8));
        for (p, a) in r.parsers.iter().zip(r.stats.iter()) {
            let recall = cell(pct(a.accepted_valid, r.valid_total));
            let fp = if r.invalid_total > 0 {
                cell(pct(a.accepted_invalid, r.invalid_total))
            } else {
                NA.to_string()
            };
            let rt = if a.can_reprint {
                cell(pct(a.roundtrip_ok, a.accepted_valid))
            } else {
                NA.to_string()
            };
            let fid = if a.can_reprint {
                cell(pct(a.fidelity_ok, a.accepted_valid))
            } else {
                NA.to_string()
            };
            println!("{:<nw$}  {recall:>7}  {fp:>7}  {rt:>7}  {fid:>8}", p.name());
        }
    } else {
        println!(
            "No reference (provenance corpus). Total statements: {}",
            r.valid_total
        );
        println!("{:<nw$}  {:>8}  {:>7}", "parser", "Accept", "RTrip");
        println!("{}", "-".repeat(nw + 2 + 8 + 2 + 7));
        for (p, a) in r.parsers.iter().zip(r.stats.iter()) {
            let acc = cell(pct(a.accepted_valid, r.valid_total));
            let rt = if a.can_reprint {
                cell(pct(a.roundtrip_ok, a.accepted_valid))
            } else {
                NA.to_string()
            };
            println!("{:<nw$}  {acc:>8}  {rt:>7}", p.name());
        }
    }
}

fn run_correctness() {
    println!("Multi-dialect SQL parser correctness");
    println!("Reference-graded (PostgreSQL=pg_query, SQLite=lemon-rs), acceptance-rate elsewhere.");
    println!("Each parser run in its best-matching dialect.");

    let all = BenchParser::all();
    for dialect in ORDER {
        eprintln!("processing {}...", dialect.dir_name());
        if let Some(r) = report::grade_dialect(dialect, &all) {
            print_report(&r);
        }
    }
    println!();
}

// coverage (per-file acceptance matrix).

fn truncate(s: &str, w: usize) -> String {
    if s.len() <= w {
        s.to_owned()
    } else {
        s.chars().take(w).collect()
    }
}

fn run_coverage() {
    println!("\nPer-file acceptance rate per parser (parser run in matching dialect)");

    let all = BenchParser::all();
    for dialect in ORDER {
        let (parsers, stats) = report::coverage_dialect(dialect, &all);
        if stats.is_empty() {
            continue;
        }

        let name_w = stats.iter().map(|s| s.name.len()).max().unwrap_or(8).max(8);
        let col_w = 10usize;
        println!("\n=== {} ===", dialect.dir_name());
        print!("{:<name_w$}  {:>8}", "dataset", "total");
        for p in &parsers {
            print!("  {:>col_w$}", truncate(p.name(), col_w));
        }
        println!();
        let line = "-".repeat(name_w + 10 + (col_w + 2) * parsers.len());
        println!("{line}");

        let mut dia_total = 0usize;
        let mut dia_acc = vec![0usize; parsers.len()];
        for s in &stats {
            print!("{:<name_w$}  {:>8}", s.name, s.total);
            for (i, n) in s.accepted.iter().enumerate() {
                print!("  {:>col_w$}", format!("{:.1}%", pct(*n, s.total)));
                dia_acc[i] += *n;
            }
            println!();
            dia_total += s.total;
        }
        println!("{line}");
        print!("{:<name_w$}  {:>8}", "[subtotal]", dia_total);
        for acc in &dia_acc {
            print!("  {:>col_w$}", format!("{:.1}%", pct(*acc, dia_total)));
        }
        println!();
    }
    println!("\n(pg_query for PostgreSQL and sqlite3-parser for SQLite are the reference for those dialects.)");
}

fn usage() -> ! {
    eprintln!("usage: sqlbench <subcommand>");
    eprintln!("  correctness [--per-file]   grade parsers over datasets/");
    eprintln!("  export                     write web/assets/bench.json for the site");
    std::process::exit(2);
}

fn main() {
    // Several parsers panic on edge-case SQL, and the is_valid_*/accepts paths use
    // catch_unwind, so suppress the default hook's noise.
    std::panic::set_hook(Box::new(|_| {}));

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("correctness") | None => {
            if let Err(e) = sql_ast_benchmark::datasets::ensure_corpus() {
                eprintln!("ERROR: could not prepare datasets/: {e}");
                std::process::exit(1);
            }
            if args.iter().any(|a| a == "--per-file") {
                run_coverage();
            } else {
                run_correctness();
            }
        }
        Some("export") => {
            if let Err(e) = sql_ast_benchmark::datasets::ensure_corpus() {
                eprintln!("ERROR: could not prepare datasets/: {e}");
                std::process::exit(1);
            }
            if let Err(e) = export::run() {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        Some("-h" | "--help" | "help") => usage(),
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            usage();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pct, truncate};

    #[test]
    fn pct_handles_zero_base() {
        assert!((pct(1, 4) - 25.0).abs() < f64::EPSILON);
        assert!((pct(0, 0) - 0.0).abs() < f64::EPSILON);
        assert!((pct(3, 3) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn truncate_clips_long_names() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("verylongname", 4), "very");
    }
}

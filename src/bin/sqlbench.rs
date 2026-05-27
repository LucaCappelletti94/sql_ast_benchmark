#![allow(
    clippy::too_many_lines,
    clippy::print_literal,
    clippy::needless_collect
)]

//! `sqlbench`: the multi-dialect SQL parser benchmark CLI.
//!
//! Subcommands:
//!   correctness [--per-file]   grade parsers over `datasets/` (oracle where one
//!                              exists, acceptance rate otherwise). `--per-file`
//!                              prints the per-dataset acceptance matrix instead
//!                              of per-dialect oracle metrics.
//!   plot                       render `benchmark_results*.svg` from the data
//!                              `cargo bench` wrote to `target/bench_dist/`.
//!
//! The grading and chart logic live in the library (`report`, `plot`); this
//! binary is argument dispatch plus table formatting.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::report::{self, DialectReport};
use sql_ast_benchmark::{plot, BenchParser};
use std::fs;
use std::path::{Path, PathBuf};

/// Large worker stack: deeply nested SQL overflows recursive-descent parsers
/// and stack overflow aborts the process (uncatchable), so give headroom.
const WORKER_STACK: usize = 512 * 1024 * 1024;

/// Oracle-backed dialects first, then the provenance dialects.
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

// correctness (per-dialect oracle + provenance).

/// Grade one dialect, parallelising over statement chunks on large stacks.
fn process_dialect(dialect: Dialect, all_parsers: &[BenchParser]) -> Option<DialectReport> {
    let stmts = report::load_dialect(dialect);
    if stmts.is_empty() {
        return None;
    }
    let parsers: Vec<BenchParser> = all_parsers
        .iter()
        .copied()
        .filter(|p| p.supports(dialect))
        .collect();

    let n_threads = std::thread::available_parallelism()
        .map_or(8, std::num::NonZeroUsize::get)
        .min(32);
    let chunk = stmts.len().div_ceil(n_threads).max(1);

    let merged = std::thread::scope(|scope| {
        let handles: Vec<_> = stmts
            .chunks(chunk)
            .map(|c| {
                let parsers = &parsers;
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, move || report::grade_chunk(c, dialect, parsers))
                    .expect("spawn worker")
            })
            .collect();
        let mut acc = DialectReport::empty(dialect, &parsers);
        for h in handles {
            acc.merge(&h.join().expect("grade thread panicked"));
        }
        acc
    });
    Some(merged)
}

fn print_report(r: &DialectReport) {
    println!("\n=== {} ===", r.dialect.dir_name());
    let nw = r
        .parsers
        .iter()
        .map(|p| p.name().len())
        .max()
        .unwrap_or(22)
        .max(22);

    if r.has_oracle {
        let oracle = if r.dialect == Dialect::Sqlite {
            "sqlite3-parser (lemon-rs)"
        } else {
            "pg_query (libpg_query)"
        };
        println!(
            "Oracle: {}   valid: {}   invalid: {}",
            oracle, r.valid_total, r.invalid_total
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
            "No oracle (provenance corpus). Total statements: {}",
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
    println!("Oracle-graded (PostgreSQL=pg_query, SQLite=lemon-rs), acceptance-rate elsewhere.");
    println!("Each parser run in its best-matching dialect.");

    let all = BenchParser::all();
    for dialect in ORDER {
        eprintln!("processing {}...", dialect.dir_name());
        if let Some(r) = process_dialect(dialect, &all) {
            print_report(&r);
        }
    }
    println!();
}

// coverage (per-file acceptance matrix).

struct FileStat {
    name: String,
    total: usize,
    accepted: Vec<usize>,
}

fn eval_file(path: &Path, dialect: Dialect, parsers: &[BenchParser]) -> Option<FileStat> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let content = fs::read_to_string(path).ok()?;
    let stmts: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if stmts.is_empty() {
        return None;
    }
    let accepted = parsers
        .iter()
        .map(|&p| report::count_accepted(&stmts, dialect, p))
        .collect();
    Some(FileStat {
        name,
        total: stmts.len(),
        accepted,
    })
}

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
    let mut dirs: Vec<_> = fs::read_dir("datasets")
        .expect("read datasets/")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .collect();
    dirs.sort_by_key(std::fs::DirEntry::file_name);

    for dir in dirs {
        let dir_name = dir.file_name().to_string_lossy().into_owned();
        let Some(dialect) = Dialect::from_dir_name(&dir_name) else {
            continue;
        };
        let parsers: Vec<BenchParser> = all
            .iter()
            .copied()
            .filter(|p| p.supports(dialect))
            .collect();
        let mut files: Vec<PathBuf> = fs::read_dir(dir.path())
            .expect("read dialect dir")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "txt"))
            .collect();
        files.sort();
        if files.is_empty() {
            continue;
        }

        let stats: Vec<FileStat> = std::thread::scope(|scope| {
            let handles: Vec<_> = files
                .iter()
                .map(|path| {
                    let parsers = &parsers;
                    std::thread::Builder::new()
                        .stack_size(WORKER_STACK)
                        .spawn_scoped(scope, move || eval_file(path, dialect, parsers))
                        .expect("spawn worker")
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok().flatten())
                .collect()
        });
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
    println!("\n(pg_query for PostgreSQL and sqlite3-parser for SQLite are the oracles for those dialects.)");
}

fn usage() -> ! {
    eprintln!("usage: sqlbench <subcommand>");
    eprintln!("  correctness [--per-file]   grade parsers over datasets/");
    eprintln!("  plot                       render benchmark_results*.svg");
    std::process::exit(2);
}

fn main() {
    // Several parsers panic on edge-case SQL; the is_valid_*/accepts paths use
    // catch_unwind, so suppress the default hook's noise.
    std::panic::set_hook(Box::new(|_| {}));

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("plot") => {
            if let Err(e) = plot::render() {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        Some("correctness") | None => {
            if !Path::new("datasets").exists() {
                eprintln!(
                    "ERROR: datasets/ not found. Run `tar --zstd -xf datasets.tar.zst` first."
                );
                std::process::exit(1);
            }
            if args.iter().any(|a| a == "--per-file") {
                run_coverage();
            } else {
                run_correctness();
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
    use super::{eval_file, pct, truncate};
    use sql_ast_benchmark::datasets::Dialect;
    use sql_ast_benchmark::BenchParser;
    use std::fs;

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

    #[test]
    fn eval_file_counts_nonblank_and_acceptance() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("sqlbench_eval_{}_{nanos}.txt", std::process::id()));
        fs::write(&path, "SELECT 1\n\nSELECT 1 FROM\n").unwrap();

        let stat = eval_file(&path, Dialect::Postgresql, &[BenchParser::Sqlparser]).unwrap();
        assert_eq!(stat.total, 2); // two non-blank lines
        assert_eq!(stat.accepted[0], 1); // sqlparser accepts "SELECT 1", rejects truncated
        let _ = fs::remove_file(&path);
    }
}

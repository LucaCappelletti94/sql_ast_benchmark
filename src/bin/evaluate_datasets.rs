#![allow(
    clippy::doc_markdown,
    clippy::print_literal,
    clippy::too_many_lines,
    clippy::needless_collect
)]

//! Multi-dialect dataset coverage evaluation.
//!
//! Walks `datasets/{dialect}/{name}.txt`, infers each file's dialect from its
//! directory, and runs every parser that models that dialect on it. Reports,
//! per dialect, the acceptance rate (% of the corpus each parser accepts) with
//! the parser run in its best-matching dialect. Parsers that do not model a
//! dialect are omitted from that dialect's table.
//!
//!   cargo run --bin evaluate_datasets

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::BenchParser;
use std::fs;
use std::path::{Path, PathBuf};

/// Large worker stack: deeply nested SQL overflows recursive-descent parsers
/// and stack overflow aborts the process, so give workers plenty of headroom.
const WORKER_STACK: usize = 512 * 1024 * 1024;

/// Per-file, per-parser acceptance counts.
struct FileStats {
    name: String,
    total: usize,
    /// Parser -> accepted count (only parsers that model the dialect).
    accepted: Vec<(BenchParser, usize)>,
}

fn evaluate_file(dialect: Dialect, path: &Path, parsers: &[BenchParser]) -> Option<FileStats> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let content = fs::read_to_string(path).ok()?;
    let stmts: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let total = stmts.len();
    if total == 0 {
        return None;
    }

    let accepted = parsers
        .iter()
        .map(|&p| {
            let n = stmts
                .iter()
                .filter(|s| p.accepts(s, dialect) == Some(true))
                .count();
            (p, n)
        })
        .collect();

    Some(FileStats {
        name,
        total,
        accepted,
    })
}

fn pct(n: usize, base: usize) -> f64 {
    if base == 0 {
        0.0
    } else {
        100.0 * n as f64 / base as f64
    }
}

/// (dialect, sorted .txt file paths) for every populated `datasets/` subdir.
fn discover() -> Vec<(Dialect, Vec<PathBuf>)> {
    let root = Path::new("datasets");
    let mut dirs: Vec<_> = fs::read_dir(root)
        .expect("cannot read datasets/")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .collect();
    dirs.sort_by_key(std::fs::DirEntry::file_name);

    let mut out = Vec::new();
    for dir in dirs {
        let dir_name = dir.file_name().to_string_lossy().into_owned();
        let Some(dialect) = Dialect::from_dir_name(&dir_name) else {
            eprintln!("  [skip] unknown dialect dir: {dir_name}");
            continue;
        };
        let mut files: Vec<PathBuf> = fs::read_dir(dir.path())
            .expect("cannot read dialect dir")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "txt"))
            .collect();
        files.sort();
        if !files.is_empty() {
            out.push((dialect, files));
        }
    }
    out
}

fn main() {
    // Several parsers panic on edge-case SQL; is_valid_* use catch_unwind.
    std::panic::set_hook(Box::new(|_| {}));

    if !Path::new("datasets").exists() {
        eprintln!("ERROR: `datasets/` directory not found.");
        eprintln!("Run `cargo run --bin download_datasets` first.");
        std::process::exit(1);
    }

    let groups = discover();
    if groups.is_empty() {
        eprintln!("No dataset files found. Run `cargo run --bin download_datasets` first.");
        std::process::exit(1);
    }

    let all_parsers = BenchParser::all();

    println!("\n╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  Multi-dialect dataset coverage — acceptance rate per parser         ║");
    println!("║  Each parser run in its best-matching dialect for the corpus.        ║");
    println!("║  Oracle column (ground truth) marked with *.                         ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    // Grand totals per parser across all dialects.
    let mut grand_total = 0usize;

    for (dialect, files) in &groups {
        // Parsers that model this dialect.
        let parsers: Vec<BenchParser> = all_parsers
            .iter()
            .copied()
            .filter(|p| p.supports(*dialect))
            .collect();

        // Compute file stats in parallel (one thread per file). Use a large
        // stack: some recursive-descent parsers blow the default stack on
        // deeply nested SQL, and stack overflow aborts the process (it is not
        // catchable via catch_unwind).
        let stats: Vec<FileStats> = std::thread::scope(|scope| {
            let handles: Vec<_> = files
                .iter()
                .map(|path| {
                    let parsers = &parsers;
                    let handle = std::thread::Builder::new()
                        .stack_size(WORKER_STACK)
                        .spawn_scoped(scope, move || evaluate_file(*dialect, path, parsers))
                        .expect("spawn worker");
                    (path, handle)
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|(path, h)| {
                    h.join().unwrap_or_else(|_| {
                        eprintln!(
                            "  [warn] worker panicked on {}, file skipped",
                            path.display()
                        );
                        None
                    })
                })
                .collect()
        });
        if stats.is_empty() {
            continue;
        }

        // Layout.
        let name_w = stats.iter().map(|s| s.name.len()).max().unwrap_or(7).max(8);
        let col_w = 10usize;

        println!("\n═══ {} ═══", dialect.dir_name());
        // Header.
        print!("{:<name_w$}  {:>8}", "dataset", "total", name_w = name_w);
        for p in &parsers {
            let mark = if sql_ast_benchmark::has_oracle(*dialect) && is_oracle(*p, *dialect) {
                "*"
            } else {
                ""
            };
            print!(
                "  {:>col_w$}",
                truncate(&format!("{}{}", p.name(), mark), col_w),
                col_w = col_w
            );
        }
        println!();
        let line = "─".repeat(name_w + 10 + (col_w + 2) * parsers.len());
        println!("{line}");

        let mut dia_total = 0usize;
        let mut dia_acc = vec![0usize; parsers.len()];

        for s in &stats {
            print!("{:<name_w$}  {:>8}", s.name, s.total, name_w = name_w);
            for (i, (_p, n)) in s.accepted.iter().enumerate() {
                print!(
                    "  {:>col_w$}",
                    format!("{:.1}%", pct(*n, s.total)),
                    col_w = col_w
                );
                dia_acc[i] += *n;
            }
            println!();
            dia_total += s.total;
        }

        println!("{line}");
        print!(
            "{:<name_w$}  {:>8}",
            "[subtotal]",
            dia_total,
            name_w = name_w
        );
        for acc in &dia_acc {
            print!(
                "  {:>col_w$}",
                format!("{:.1}%", pct(*acc, dia_total)),
                col_w = col_w
            );
        }
        println!();
        grand_total += dia_total;
    }

    println!("\nTotal statements evaluated: {grand_total}");
    println!("(* = oracle / ground truth for that dialect: pg_query for PostgreSQL, sqlite3-parser for SQLite)");
}

/// Is this parser the oracle for the dialect?
fn is_oracle(p: BenchParser, dialect: Dialect) -> bool {
    match dialect {
        #[cfg(feature = "pg_query_parser")]
        Dialect::Postgresql => p == BenchParser::PgQuery,
        Dialect::Sqlite => p == BenchParser::Sqlite3,
        _ => false,
    }
}

fn truncate(s: &str, w: usize) -> String {
    if s.len() <= w {
        s.to_string()
    } else {
        s.chars().take(w).collect()
    }
}

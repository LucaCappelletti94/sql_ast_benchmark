//! Multi-dialect BATCH (multi-statement script) parse benchmark over the full
//! `datasets/` corpus.
//!
//! Companion to `benches/parsing.rs`. Where `parsing` times each statement in
//! isolation, this draws random fixed-size batches of statements a parser can
//! individually digest, joins each into one script, and parses it in a single
//! call. It reports two things per (parser, dialect): batch accuracy, the share
//! of batches that reparse to exactly the expected statement count, and the
//! per-statement parse time averaged over the batches that did. Sampling instead
//! of concatenating the whole accepted set keeps one statement that mishandles
//! the terminator (a real but narrow bug) from voiding the entire measurement
//! under the all-or-nothing `parse_sql`.
//!
//! The sampling, joining, and accuracy live in `sql_ast_benchmark::batch` so the
//! memory bench (`membench -- batch`) and the time machine sample identically.
//! Only parsers with a multi-statement entry point take part (`can_batch`).
//!
//! Output (`target/batch_dist/summary.csv`): per pair the eligible count, the
//! number of batches, how many were correct, the accuracy percent, and the
//! per-statement time over correct batches.
//!
//! Full run:        `cargo bench --bench batch_parsing`
//! Smoke (default): `cargo test` or `cargo bench --bench batch_parsing -- --test`

use sql_ast_benchmark::batch::{evaluate_batches, reports_statement_count, BATCH_K, BATCH_M};
use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::report::load_dialect;
use sql_ast_benchmark::BenchParser;
use std::fs;
use std::hint::black_box;
use std::io::Write as _;
use std::panic::AssertUnwindSafe;
use std::time::Instant;

/// Deep statements can exhaust the default stack inside recursive-descent
/// parsers, and a stack overflow aborts the process, so run on a large stack.
const WORKER_STACK: usize = 1024 * 1024 * 1024;

const OUT_DIR: &str = "target/batch_dist";

const DIALECTS: &[Dialect] = &[
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

/// Whole-sweep parse time (ns): adaptive iteration count so a short sweep still
/// accumulates enough work per round, best (min) of `ROUNDS` rounds.
fn time_sweep(mut f: impl FnMut() -> usize) -> f64 {
    const TARGET_NS: u128 = 2_000_000;
    const ROUNDS: usize = 5;

    black_box(f());
    let probe = Instant::now();
    black_box(f());
    let single = probe.elapsed().as_nanos().max(1);
    let iters = u64::try_from((TARGET_NS / single).clamp(1, 1_000)).unwrap_or(1);

    let mut best = f64::MAX;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        for _ in 0..iters {
            black_box(f());
        }
        let per = start.elapsed().as_nanos() as f64 / iters as f64;
        best = best.min(per);
    }
    best
}

/// Parse one script to a statement count, treating a caught panic as 0 so a
/// single pathological input does not abort the whole (parser, dialect) pair.
fn safe_count(parser: BenchParser, sql: &str, dialect: Dialect) -> usize {
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        parser.parse_batch(sql, dialect).unwrap_or(0)
    }))
    .unwrap_or(0)
}

struct Row {
    dialect: &'static str,
    parser: &'static str,
    n_eligible: usize,
    k: usize,
    n_correct: usize,
    accuracy_pct: Option<f64>,
    /// Per-statement parse time over the correct batches (ns), `None` when none.
    ns_per_stmt: Option<f64>,
}

/// Evaluate one (parser, dialect) pair: build the eligible set, sample batches,
/// measure accuracy, and time the batches that parsed correctly.
fn run_pair(parser: BenchParser, dialect: Dialect, stmts: &[String]) -> Row {
    // Eligible = accepted, parses to exactly one statement alone, and safe to
    // batch (not COPY ... FROM STDIN). The single==1 check makes the expected
    // per-batch count exactly the batch size.
    let eligible: Vec<&str> = stmts
        .iter()
        .filter(|s| {
            parser.accepts(s, dialect) == Some(true)
                && sql_ast_benchmark::batch::batch_eligible(s)
                && safe_count(parser, s, dialect) == 1
        })
        .map(String::as_str)
        .collect();

    let label = format!("{}/{}", dialect.dir_name(), parser.name());
    let eval = evaluate_batches(&eligible, &label, |s| safe_count(parser, s, dialect));

    let ns_per_stmt = if eval.n_correct == 0 {
        None
    } else {
        let denom = (eval.n_correct * eval.effective_m) as f64;
        let sweep = time_sweep(|| {
            eval.correct_scripts
                .iter()
                .map(|s| safe_count(parser, s, dialect))
                .sum()
        });
        Some(sweep / denom)
    };

    Row {
        dialect: dialect.dir_name(),
        parser: parser.name(),
        n_eligible: eval.n_eligible,
        k: eval.k,
        n_correct: eval.n_correct,
        accuracy_pct: eval.accuracy_pct(),
        ns_per_stmt,
    }
}

/// Quick smoke check used by `cargo test`: every batch-capable parser parses a
/// tiny multi-statement script per supported dialect without panicking.
fn smoke() {
    std::panic::set_hook(Box::new(|_| {}));
    let script = "SELECT 1\n;\nSELECT 2\n;\nSELECT 3";
    for &dialect in DIALECTS {
        for parser in BenchParser::all() {
            if !parser.can_batch() || !parser.supports(dialect) {
                continue;
            }
            black_box(parser.parse_batch(script, dialect));
        }
    }
    println!("smoke ok");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let full_run = args.iter().any(|a| a == "--bench") && !args.iter().any(|a| a == "--test");
    if !full_run {
        smoke();
        return;
    }

    std::panic::set_hook(Box::new(|_| {}));

    if let Err(e) = sql_ast_benchmark::datasets::ensure_corpus() {
        eprintln!("ERROR: could not prepare datasets/: {e}");
        std::process::exit(1);
    }
    fs::create_dir_all(OUT_DIR).expect("create out dir");

    let mut summary = fs::File::create(format!("{OUT_DIR}/summary.csv")).expect("summary.csv");
    writeln!(
        summary,
        "dialect,parser,n_eligible,k,n_correct,accuracy_pct,ns_per_stmt"
    )
    .unwrap();

    let parsers = BenchParser::all();
    let start_all = Instant::now();
    println!("batch sampling: m={BATCH_M} statements, k={BATCH_K} batches per pair");

    for &dialect in DIALECTS {
        let stmts = load_dialect(dialect);
        if stmts.is_empty() {
            continue;
        }
        for parser in &parsers {
            let parser = *parser;
            if !parser.can_batch() || !parser.supports(dialect) {
                continue;
            }
            // Skip parsers whose batch entry point does not report a true
            // statement count (e.g. pg_query summary returns distinct types).
            if !reports_statement_count(|s| safe_count(parser, s, dialect)) {
                continue;
            }
            let job_start = Instant::now();
            let result = std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, || run_pair(parser, dialect, &stmts))
                    .expect("spawn worker")
                    .join()
            });
            let Ok(row) = result else {
                eprintln!(
                    "  [warn] {}/{} panicked, skipping pair",
                    dialect.dir_name(),
                    parser.name()
                );
                continue;
            };

            let acc = row
                .accuracy_pct
                .map_or_else(String::new, |a| format!("{a:.3}"));
            let ns = row
                .ns_per_stmt
                .map_or_else(String::new, |n| format!("{n:.1}"));
            writeln!(
                summary,
                "{},{},{},{},{},{acc},{ns}",
                row.dialect, row.parser, row.n_eligible, row.k, row.n_correct,
            )
            .unwrap();
            summary.flush().unwrap();

            println!(
                "{:<11} {:<24} elig={:>6} ok={:>3}/{:<3} acc={:>6} batch={:>9}ns/stmt  ({:.1}s)",
                row.dialect,
                row.parser,
                row.n_eligible,
                row.n_correct,
                row.k,
                row.accuracy_pct
                    .map_or_else(|| "n/a".to_string(), |a| format!("{a:.1}%")),
                row.ns_per_stmt
                    .map_or_else(|| "n/a".to_string(), |n| format!("{n:.0}")),
                job_start.elapsed().as_secs_f64(),
            );
        }
    }

    println!(
        "\nDone in {:.1}s. summary.csv in {OUT_DIR}/",
        start_all.elapsed().as_secs_f64()
    );
}

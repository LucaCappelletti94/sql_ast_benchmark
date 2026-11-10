//! Per-statement memory benchmark.
//!
//! Installs the counting global allocator from `sql_ast_benchmark::mem`, then,
//! for every (parser, dialect) pair, measures for each accepted statement the
//! peak live bytes, the retained (AST) bytes, and the allocation pressure
//! (allocator calls and bytes requested). Results are written one value per
//! line to `target/mem_dist/`, consumed by `sqlbench export` to build the
//! memory section of `web/assets/bench.json.zst`.
//!
//! Measurement is single-threaded by design: the allocator counters are
//! process-wide, so concurrent allocations from other threads would corrupt a
//! window. The libpg_query bindings parse in C and report `None` (their memory
//! is invisible to the Rust allocator).
//!
//! A `batch` subcommand measures whole-script memory instead: per (parser,
//! dialect) it concatenates the accepted set into one script, parses it holding
//! every AST live, and records the same four figures normalized per statement
//! to `target/batch_mem_dist/summary.csv`. Databend has no batch entry point
//! and is skipped there.
//!
//! Run locally: `cargo run --release -p membench`            (per-statement)
//!              `cargo run --release -p membench -- batch`    (whole-script)

use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::Path;

use sql_ast_benchmark::batch::{batch_eligible, evaluate_batches, reports_statement_count};
use sql_ast_benchmark::datasets::{ensure_corpus, Dialect};
use sql_ast_benchmark::mem::CountingAlloc;
use sql_ast_benchmark::stats::slug;
use sql_ast_benchmark::BenchParser;

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

const OUT_DIR: &str = "target/mem_dist";
const BATCH_OUT_DIR: &str = "target/batch_mem_dist";

/// Deep statements can overflow the stack in recursive-descent parsers, so run
/// the whole measurement on a large stack (and a single thread).
const WORKER_STACK: usize = 1024 * 1024 * 1024;

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

fn load_dialect(dialect: Dialect) -> Vec<String> {
    let dir = Path::new("datasets").join(dialect.dir_name());
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        if let Ok(content) = fs::read_to_string(&f) {
            out.extend(
                content
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(String::from),
            );
        }
    }
    out
}

fn write_raw(dialect: &str, parser: &str, kind: &str, values: &[usize]) {
    let path = format!("{OUT_DIR}/{dialect}__{}.{kind}.txt", slug(parser));
    let mut buf = String::with_capacity(values.len() * 6);
    for v in values {
        let _ = writeln!(buf, "{v}");
    }
    if let Ok(mut file) = fs::File::create(&path) {
        let _ = file.write_all(buf.as_bytes());
    }
}

fn run() {
    fs::create_dir_all(OUT_DIR).expect("create mem_dist dir");
    for &dialect in DIALECTS {
        let stmts = load_dialect(dialect);
        if stmts.is_empty() {
            continue;
        }
        for parser in BenchParser::all() {
            if !parser.supports(dialect) {
                continue;
            }
            let accepted: Vec<&str> = stmts
                .iter()
                .filter(|s| parser.accepts(s, dialect) == Some(true))
                .map(String::as_str)
                .collect();
            if accepted.is_empty() {
                continue;
            }
            // Parsers whose memory the Rust allocator cannot see report None.
            if parser.measure_mem(accepted[0], dialect).is_none() {
                continue;
            }
            // Warm up: let one-time caches/lazy statics allocate before we start,
            // so they raise the baseline rather than the first statement.
            let _ = parser.measure_mem(accepted[0], dialect);

            let mut peaks = Vec::with_capacity(accepted.len());
            let mut retained = Vec::with_capacity(accepted.len());
            let mut allocs = Vec::with_capacity(accepted.len());
            let mut allocated = Vec::with_capacity(accepted.len());
            for s in &accepted {
                if let Some(m) = parser.measure_mem(s, dialect) {
                    peaks.push(m.peak);
                    retained.push(m.retained);
                    allocs.push(m.allocs);
                    allocated.push(m.allocated);
                }
            }
            let (dir, name) = (dialect.dir_name(), parser.name());
            write_raw(dir, name, "peak", &peaks);
            write_raw(dir, name, "retained", &retained);
            write_raw(dir, name, "allocs", &allocs);
            write_raw(dir, name, "allocated", &allocated);
            eprintln!(
                "mem {} {}: n={}",
                dialect.dir_name(),
                parser.name(),
                peaks.len()
            );
        }
    }
}

/// Parse one script to a statement count under panic protection, so a single
/// pathological input cannot abort the whole batch run.
fn safe_count(parser: BenchParser, sql: &str, dialect: Dialect) -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parser.parse_batch(sql, dialect).unwrap_or(0)
    }))
    .unwrap_or(0)
}

/// Batch memory: per (parser, dialect) it samples the same random batches as the
/// time bench (deterministic seed), measures peak, retained, and allocation
/// pressure over the batches that reparse correctly, and records them normalized
/// per statement to `target/batch_mem_dist/summary.csv`. Only parsers whose
/// memory is visible to the Rust allocator take part (the libpg_query bindings
/// report `None`).
fn run_batch() {
    fs::create_dir_all(BATCH_OUT_DIR).expect("create batch_mem_dist dir");
    let mut summary =
        fs::File::create(format!("{BATCH_OUT_DIR}/summary.csv")).expect("create summary.csv");
    writeln!(
        summary,
        "dialect,parser,n_eligible,k,n_correct,accuracy_pct,peak_per_stmt,retained_per_stmt,allocs_per_stmt,allocated_per_stmt"
    )
    .expect("write header");

    for &dialect in DIALECTS {
        let stmts = load_dialect(dialect);
        if stmts.is_empty() {
            continue;
        }
        for parser in BenchParser::all() {
            if !parser.can_batch() || !parser.supports(dialect) {
                continue;
            }
            // Skip parsers whose memory is invisible to the Rust allocator (the
            // libpg_query bindings parse in C and report None).
            if parser.measure_mem_batch("SELECT 1", dialect).is_none() {
                continue;
            }
            // Skip parsers whose batch entry point does not report a true count.
            if !reports_statement_count(|s| safe_count(parser, s, dialect)) {
                continue;
            }
            let eligible: Vec<&str> = stmts
                .iter()
                .filter(|s| {
                    parser.accepts(s, dialect) == Some(true)
                        && batch_eligible(s)
                        && safe_count(parser, s, dialect) == 1
                })
                .map(String::as_str)
                .collect();
            let label = format!("{}/{}", dialect.dir_name(), parser.name());
            let eval = evaluate_batches(&eligible, &label, |s| safe_count(parser, s, dialect));

            let [peak_per_stmt, retained_per_stmt, allocs_per_stmt, allocated_per_stmt]: [String;
                4] = if eval.n_correct == 0 {
                Default::default()
            } else {
                let mut sums = [0usize; 4];
                for s in &eval.correct_scripts {
                    if let Some(m) = parser.measure_mem_batch(s, dialect) {
                        let sample = [m.peak, m.retained, m.allocs, m.allocated];
                        for (sum, v) in sums.iter_mut().zip(sample) {
                            *sum = sum
                                .checked_add(v)
                                .expect("batch memory sum overflows usize");
                        }
                    }
                }
                // Precision loss above 2^53 is irrelevant for a per-statement mean.
                let denom = (eval.n_correct * eval.effective_m) as f64;
                sums.map(|sum| format!("{:.1}", sum as f64 / denom))
            };

            let acc = eval
                .accuracy_pct()
                .map_or_else(String::new, |a| format!("{a:.3}"));
            writeln!(
                summary,
                "{},{},{},{},{},{acc},{peak_per_stmt},{retained_per_stmt},{allocs_per_stmt},{allocated_per_stmt}",
                dialect.dir_name(),
                parser.name(),
                eval.n_eligible,
                eval.k,
                eval.n_correct,
            )
            .expect("write row");
            summary.flush().expect("flush summary");
            eprintln!(
                "batch-mem {} {}: elig={} ok={}/{} peak/stmt={peak_per_stmt} ret/stmt={retained_per_stmt} allocs/stmt={allocs_per_stmt}",
                dialect.dir_name(),
                parser.name(),
                eval.n_eligible,
                eval.n_correct,
                eval.k,
            );
        }
    }
}

fn main() {
    ensure_corpus().expect("dataset corpus");
    let batch = std::env::args().any(|a| a == "batch");
    std::thread::Builder::new()
        .stack_size(WORKER_STACK)
        .spawn(move || if batch { run_batch() } else { run() })
        .expect("spawn worker")
        .join()
        .expect("measurement thread panicked");
}

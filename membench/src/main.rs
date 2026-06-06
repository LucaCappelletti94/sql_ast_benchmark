//! Per-statement memory benchmark.
//!
//! Installs a counting global allocator that feeds the safe counters in
//! `sql_ast_benchmark::mem`, then, for every (parser, dialect) pair, measures
//! the peak live bytes and the retained (AST) bytes for each accepted statement.
//! Results are written one value per line to `target/mem_dist/`, consumed by
//! `sqlbench export` to build the memory section of `web/assets/bench.json`.
//!
//! Measurement is single-threaded by design: the allocator counters are
//! process-wide, so concurrent allocations from other threads would corrupt a
//! window. The libpg_query bindings parse in C and report `None` (their memory
//! is invisible to the Rust allocator).
//!
//! A `batch` subcommand measures whole-script memory instead: per (parser,
//! dialect) it concatenates the accepted set into one script, parses it holding
//! every AST live, and records peak/retained bytes normalized per statement to
//! `target/batch_mem_dist/summary.csv`. Databend has no batch entry point and
//! is skipped there.
//!
//! Run locally: `cargo run --release -p membench`            (per-statement)
//!              `cargo run --release -p membench -- batch`    (whole-script)

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::Path;

use sql_ast_benchmark::batch::join_batch;
use sql_ast_benchmark::datasets::{ensure_corpus, Dialect};
use sql_ast_benchmark::stats::slug;
use sql_ast_benchmark::BenchParser;

/// System allocator that records every allocation into `sql_ast_benchmark::mem`.
struct Counting;

// SAFETY: a thin pass-through to the system allocator that only adds atomic
// bookkeeping (no allocation of its own) around each call.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            sql_ast_benchmark::mem::record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        sql_ast_benchmark::mem::record_dealloc(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                sql_ast_benchmark::mem::record_alloc(new_size - layout.size());
            } else {
                sql_ast_benchmark::mem::record_dealloc(layout.size() - new_size);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

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
            for s in &accepted {
                if let Some((pk, rt)) = parser.measure_mem(s, dialect) {
                    peaks.push(pk);
                    retained.push(rt);
                }
            }
            write_raw(dialect.dir_name(), parser.name(), "peak", &peaks);
            write_raw(dialect.dir_name(), parser.name(), "retained", &retained);
            eprintln!(
                "mem {} {}: n={}",
                dialect.dir_name(),
                parser.name(),
                peaks.len()
            );
        }
    }
}

/// Whole-script (batch) memory: one (peak, retained) pair per (parser, dialect),
/// normalized per statement, written to a single summary file. Only parsers with
/// a batch entry point whose memory is visible to the Rust allocator take part.
fn run_batch() {
    fs::create_dir_all(BATCH_OUT_DIR).expect("create batch_mem_dist dir");
    let mut summary =
        fs::File::create(format!("{BATCH_OUT_DIR}/summary.csv")).expect("create summary.csv");
    writeln!(
        summary,
        "dialect,parser,n_accepted,n_parsed,peak_bytes,retained_bytes,peak_per_stmt,retained_per_stmt"
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
            let accepted: Vec<&str> = stmts
                .iter()
                .filter(|s| parser.accepts(s, dialect) == Some(true))
                .map(String::as_str)
                .collect();
            if accepted.is_empty() {
                continue;
            }
            let batch = join_batch(&accepted);
            // Warm up: let one-time caches/lazy statics allocate first, so they
            // raise the baseline rather than this measurement. Also skips
            // parsers whose memory is invisible to the Rust allocator (None).
            if parser.measure_mem_batch(&batch, dialect).is_none() {
                continue;
            }
            let Some((peak, retained)) = parser.measure_mem_batch(&batch, dialect) else {
                continue;
            };
            // Statements the parser actually consumed from the script, so the
            // export can drop a pair whose batch parse bailed out early.
            let n_parsed = parser.parse_batch(&batch, dialect).unwrap_or(0);

            let n = accepted.len() as f64;
            writeln!(
                summary,
                "{},{},{},{n_parsed},{peak},{retained},{:.1},{:.1}",
                dialect.dir_name(),
                parser.name(),
                accepted.len(),
                peak as f64 / n,
                retained as f64 / n,
            )
            .expect("write row");
            summary.flush().expect("flush summary");
            let coverage = 100.0 * n_parsed as f64 / n;
            eprintln!(
                "batch-mem {} {}: n={} seen={n_parsed} ({coverage:.0}%) peak={peak} retained={retained}",
                dialect.dir_name(),
                parser.name(),
                accepted.len(),
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

//! Multi-dialect parse-time benchmark over the FULL `datasets/` corpus.
//!
//! For every (parser, dialect) pair, this:
//!   1. builds the parser's accepted set (statements it parses in that dialect),
//!   2. times each accepted statement individually to produce a per-statement
//!      time distribution, and
//!   3. times the whole accepted body concatenated, normalized by n.
//!
//! Keying on the accepted set means the concatenated parse never stops early on
//! a statement the parser would reject. Timing uses `parse_once` (no
//! `catch_unwind`) for overhead-free, fair measurement; accepted statements are
//! known not to panic.
//!
//! Outputs (under `target/bench_dist/`):
//!   - `{dialect}__{parser}.txt` : raw per-statement times (ns, one per line),
//!     so any plot can be regenerated without re-running.
//!   - `summary.csv`             : per-pair percentiles + normalized concat time.
//!
//! Full benchmark (long; intended for a dedicated run):  cargo bench
//! Quick smoke check (used by the pre-commit hook):       cargo bench -- --test
//!
//! Requires `tar --zstd -xf datasets.tar.zst` to have populated `datasets/`.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::stats::{quantile, slug};
use sql_ast_benchmark::BenchParser;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::io::Write as _;
use std::path::Path;
use std::time::Instant;

/// Deep statements can exhaust the default stack inside recursive-descent
/// parsers; a stack overflow aborts the process, so time on a large stack.
const WORKER_STACK: usize = 512 * 1024 * 1024;
const OUT_DIR: &str = "target/bench_dist";

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

/// Per-statement time (ns/parse): adaptive iteration count to accumulate at
/// least `TARGET_NS` per round, best (min) of `ROUNDS` rounds.
fn time_stmt(mut f: impl FnMut() -> bool) -> f64 {
    const TARGET_NS: u128 = 100_000;
    const ROUNDS: usize = 5;

    black_box(f()); // warm up
    let probe = Instant::now();
    black_box(f());
    let single = probe.elapsed().as_nanos().max(1);
    let iters = u64::try_from((TARGET_NS / single).clamp(3, 1_000_000)).unwrap_or(3);

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

/// Total ns for one parse of a (large) input: best of 3 single timed runs.
fn time_once(mut f: impl FnMut() -> bool) -> f64 {
    black_box(f()); // warm up
    let mut best = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        black_box(f());
        best = best.min(start.elapsed().as_nanos() as f64);
    }
    best
}

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

struct Row {
    dialect: &'static str,
    parser: &'static str,
    n_total: usize,
    n_accepted: usize,
    min: f64,
    p10: f64,
    p25: f64,
    median: f64,
    p75: f64,
    p90: f64,
    p99: f64,
    max: f64,
    mean: f64,
    concat_per_stmt: f64,
}

/// Time one (parser, dialect) pair: accepted set, per-statement distribution
/// (written raw to disk), and normalized concatenated parse.
fn run_pair(parser: BenchParser, dialect: Dialect, stmts: &[String]) -> Row {
    let accepted: Vec<&str> = stmts
        .iter()
        .filter(|s| parser.accepts(s, dialect) == Some(true))
        .map(String::as_str)
        .collect();

    let mut row = Row {
        dialect: dialect.dir_name(),
        parser: parser.name(),
        n_total: stmts.len(),
        n_accepted: accepted.len(),
        min: 0.0,
        p10: 0.0,
        p25: 0.0,
        median: 0.0,
        p75: 0.0,
        p90: 0.0,
        p99: 0.0,
        max: 0.0,
        mean: 0.0,
        concat_per_stmt: 0.0,
    };
    if accepted.is_empty() {
        return row;
    }

    // Per-statement distribution.
    let mut times: Vec<f64> = Vec::with_capacity(accepted.len());
    for s in &accepted {
        times.push(time_stmt(|| parser.parse_once(s, dialect)));
    }

    // Persist raw times for re-plotting.
    let raw_path = format!(
        "{OUT_DIR}/{}__{}.txt",
        dialect.dir_name(),
        slug(parser.name())
    );
    if let Ok(mut file) = fs::File::create(&raw_path) {
        let mut buf = String::with_capacity(times.len() * 8);
        for t in &times {
            let _ = writeln!(buf, "{t:.1}");
        }
        let _ = file.write_all(buf.as_bytes());
    }

    // Normalized concatenated parse.
    let joined = accepted.join("; ");
    let concat_total = time_once(|| parser.parse_once(&joined, dialect));
    row.concat_per_stmt = concat_total / accepted.len() as f64;

    // Distribution stats.
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    row.min = sorted[0];
    row.max = sorted[sorted.len() - 1];
    row.mean = times.iter().sum::<f64>() / times.len() as f64;
    row.p10 = quantile(&sorted, 0.10);
    row.p25 = quantile(&sorted, 0.25);
    row.median = quantile(&sorted, 0.50);
    row.p75 = quantile(&sorted, 0.75);
    row.p90 = quantile(&sorted, 0.90);
    row.p99 = quantile(&sorted, 0.99);
    row
}

/// Quick smoke check used by the pre-commit hook: every parser parses one of
/// its accepted statements per dialect without panicking. Fast.
fn smoke() {
    std::panic::set_hook(Box::new(|_| {}));
    for &dialect in DIALECTS {
        let stmts = load_dialect(dialect);
        if stmts.is_empty() {
            continue;
        }
        for parser in BenchParser::all() {
            if !parser.supports(dialect) {
                continue;
            }
            if let Some(s) = stmts
                .iter()
                .find(|s| parser.accepts(s, dialect) == Some(true))
            {
                black_box(parser.parse_once(s, dialect));
            }
        }
    }
    println!("smoke ok");
}

fn main() {
    if std::env::args().any(|a| a == "--test") {
        smoke();
        return;
    }

    if !Path::new("datasets").exists() {
        eprintln!("ERROR: datasets/ not found. Run `tar --zstd -xf datasets.tar.zst` first.");
        std::process::exit(1);
    }
    fs::create_dir_all(OUT_DIR).expect("create out dir");

    let mut summary = fs::File::create(format!("{OUT_DIR}/summary.csv")).expect("summary.csv");
    writeln!(
        summary,
        "dialect,parser,n_total,n_accepted,min_ns,p10_ns,p25_ns,median_ns,p75_ns,p90_ns,p99_ns,max_ns,mean_ns,concat_ns_per_stmt"
    )
    .unwrap();

    let parsers = BenchParser::all();
    let start_all = Instant::now();

    for &dialect in DIALECTS {
        let stmts = load_dialect(dialect);
        if stmts.is_empty() {
            continue;
        }
        for parser in &parsers {
            if !parser.supports(dialect) {
                continue;
            }
            let parser = *parser;
            let job_start = Instant::now();
            // Run on a large stack: deeply nested accepted statements can
            // otherwise overflow the default stack and abort the process.
            let row = std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, || run_pair(parser, dialect, &stmts))
                    .expect("spawn worker")
                    .join()
                    .expect("pair thread panicked")
            });

            writeln!(
                summary,
                "{},{},{},{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}",
                row.dialect,
                row.parser,
                row.n_total,
                row.n_accepted,
                row.min,
                row.p10,
                row.p25,
                row.median,
                row.p75,
                row.p90,
                row.p99,
                row.max,
                row.mean,
                row.concat_per_stmt,
            )
            .unwrap();
            summary.flush().unwrap();

            println!(
                "{:<11} {:<24} n={:>6}/{:<6} median={:>8.0}ns p90={:>9.0}ns concat/n={:>8.0}ns  ({:.1}s)",
                row.dialect,
                row.parser,
                row.n_accepted,
                row.n_total,
                row.median,
                row.p90,
                row.concat_per_stmt,
                job_start.elapsed().as_secs_f64(),
            );
        }
    }

    println!(
        "\nDone in {:.1}s. Raw distributions + summary.csv in {OUT_DIR}/",
        start_all.elapsed().as_secs_f64()
    );
}

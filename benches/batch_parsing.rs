//! Multi-dialect BATCH (whole-script) parse-time benchmark over the full
//! `datasets/` corpus.
//!
//! Companion to `benches/parsing.rs`. Where `parsing` times each statement in
//! isolation, this concatenates every statement a parser accepts in a dialect
//! into one script and times parsing that whole script in a single call, then
//! divides by the statement count to get a normalized per-statement cost. The
//! contrast between this and the per-statement median isolates what a batch API
//! pays or amortizes, the effect raised in issue #15: `Parser::parse_sql` grows
//! a `Vec` of large `Statement` values, so bulk parsing can behave differently
//! from many single-statement calls.
//!
//! Both axes are measured over the SAME accepted set (statements the parser
//! parses in that dialect), so the two numbers are directly comparable.
//!
//! Only parsers with a multi-statement entry point take part (see
//! `BenchParser::can_batch`); `databend-common-ast` parses one statement per
//! call and is simply skipped here.
//!
//! Output (under `target/batch_dist/`), self-contained for now (not yet wired
//! into the web export):
//!   - `summary.csv` : per-pair statement count, statements the parser saw,
//!     batch size in bytes, whole-script time, and time normalized per
//!     statement.
//!
//! Full run:        `cargo bench --bench batch_parsing`
//! Smoke (default): `cargo test` or `cargo bench --bench batch_parsing -- --test`
//!
//! The full run unpacks `datasets.tar.zst` automatically if `datasets/` is
//! missing. The smoke path needs no corpus, so `cargo test` stays fast.

use sql_ast_benchmark::batch::join_batch;
use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::report::load_dialect;
use sql_ast_benchmark::BenchParser;
use std::fs;
use std::hint::black_box;
use std::io::Write as _;
use std::time::Instant;

/// Deep statements can exhaust the default stack inside recursive-descent
/// parsers, and a stack overflow aborts the process, so time on a large stack.
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

/// Whole-script parse time (ns/batch): adaptive iteration count so a short
/// script still accumulates enough work per round, capped low because one batch
/// call already does a lot. Best (min) of `ROUNDS` rounds.
fn time_batch(mut f: impl FnMut() -> usize) -> f64 {
    const TARGET_NS: u128 = 2_000_000; // aim for ~2 ms of work per round
    const ROUNDS: usize = 5;

    black_box(f()); // warm up
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

struct Row {
    dialect: &'static str,
    parser: &'static str,
    /// Statements fed into the batch (the parser's accepted set).
    n_accepted: usize,
    /// Statements the parser reported parsing from the batch (coverage).
    n_parsed: usize,
    batch_bytes: usize,
    /// Whole-script parse time (ns).
    batch_ns: f64,
    /// `batch_ns / n_accepted`: time per statement in batch context.
    ns_per_stmt: f64,
}

/// Time one (parser, dialect) pair: build the accepted set, concatenate it into
/// one script, time the whole-script parse, and normalize per statement.
fn run_pair(parser: BenchParser, dialect: Dialect, stmts: &[String]) -> Row {
    let accepted: Vec<&str> = stmts
        .iter()
        .filter(|s| parser.accepts(s, dialect) == Some(true))
        .map(String::as_str)
        .collect();

    let mut row = Row {
        dialect: dialect.dir_name(),
        parser: parser.name(),
        n_accepted: accepted.len(),
        n_parsed: 0,
        batch_bytes: 0,
        batch_ns: 0.0,
        ns_per_stmt: 0.0,
    };
    if accepted.is_empty() {
        return row;
    }

    let batch = join_batch(&accepted);
    row.batch_bytes = batch.len();
    row.n_parsed = parser.parse_batch(&batch, dialect).unwrap_or(0);
    row.batch_ns = time_batch(|| parser.parse_batch(&batch, dialect).unwrap_or(0));
    row.ns_per_stmt = row.batch_ns / accepted.len() as f64;
    row
}

/// Quick smoke check used by `cargo test`: every batch-capable parser parses a
/// tiny multi-statement script per supported dialect without panicking. Needs
/// no corpus, so it stays instant.
fn smoke() {
    std::panic::set_hook(Box::new(|_| {}));
    let script = "SELECT 1;\nSELECT 2;\nSELECT 3";
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
    // Match `benches/parsing.rs`: only an explicit `cargo bench` (which passes
    // `--bench` and not `--test`) does the full, datasets-backed run. `cargo
    // test` and a bare run take the fast smoke path, which needs no corpus.
    let args: Vec<String> = std::env::args().collect();
    let full_run = args.iter().any(|a| a == "--bench") && !args.iter().any(|a| a == "--test");
    if !full_run {
        smoke();
        return;
    }

    // Acceptance checks are panic-guarded; suppress the default panic message so
    // a caught panic does not spam stderr.
    std::panic::set_hook(Box::new(|_| {}));

    if let Err(e) = sql_ast_benchmark::datasets::ensure_corpus() {
        eprintln!("ERROR: could not prepare datasets/: {e}");
        std::process::exit(1);
    }
    fs::create_dir_all(OUT_DIR).expect("create out dir");

    let mut summary = fs::File::create(format!("{OUT_DIR}/summary.csv")).expect("summary.csv");
    writeln!(
        summary,
        "dialect,parser,n_accepted,n_parsed,batch_bytes,batch_ns,ns_per_stmt"
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
            let parser = *parser;
            if !parser.can_batch() || !parser.supports(dialect) {
                continue;
            }
            let job_start = Instant::now();
            // Run on a large stack: deeply nested accepted statements can
            // otherwise overflow the default stack and abort the process.
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

            writeln!(
                summary,
                "{},{},{},{},{},{:.1},{:.1}",
                row.dialect,
                row.parser,
                row.n_accepted,
                row.n_parsed,
                row.batch_bytes,
                row.batch_ns,
                row.ns_per_stmt,
            )
            .unwrap();
            summary.flush().unwrap();

            let coverage = if row.n_accepted == 0 {
                0.0
            } else {
                100.0 * row.n_parsed as f64 / row.n_accepted as f64
            };
            println!(
                "{:<11} {:<24} n={:>6} seen={:>6} ({:>3.0}%) batch={:>9.0}ns/stmt  ({:.1}s)",
                row.dialect,
                row.parser,
                row.n_accepted,
                row.n_parsed,
                coverage,
                row.ns_per_stmt,
                job_start.elapsed().as_secs_f64(),
            );
        }
    }

    println!(
        "\nDone in {:.1}s. summary.csv in {OUT_DIR}/",
        start_all.elapsed().as_secs_f64()
    );
}

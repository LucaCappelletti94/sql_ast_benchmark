//! Multi-dialect parse-throughput benchmark over the `datasets/` corpus.
//!
//! For every dialect that has a downloaded corpus, each parser that models that
//! dialect is timed parsing concatenated batches of 1/10/100/1000 statements,
//! run in its best-matching dialect. Parsers that do not model a dialect are
//! skipped for it.
//!
//!   cargo bench
//!
//! Requires `cargo run --bin download_datasets` to have populated `datasets/`.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode};
use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{concatenate_statements, BenchParser};
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::Duration;

/// Dialects benchmarked, in report order.
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

/// Load up to `cap` statements for a dialect from its `datasets/` subdir.
fn load_dialect(dialect: Dialect, cap: usize) -> Vec<String> {
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
    let mut stmts = Vec::new();
    for f in files {
        if let Ok(content) = fs::read_to_string(&f) {
            for line in content.lines() {
                // Skip blank and pathologically long statements: criterion runs
                // on the default 8 MiB stack, and a deeply nested statement can
                // overflow a recursive-descent parser (uncatchable abort).
                if !line.trim().is_empty() && line.len() <= 50_000 {
                    stmts.push(line.to_string());
                    if stmts.len() >= cap {
                        return stmts;
                    }
                }
            }
        }
    }
    stmts
}

fn bench_dialect(c: &mut Criterion, dialect: Dialect) {
    let statements = load_dialect(dialect, 1000);
    if statements.is_empty() {
        return;
    }
    let parsers: Vec<BenchParser> = BenchParser::all()
        .into_iter()
        .filter(|p| p.supports(dialect))
        .collect();

    let sizes: Vec<usize> = [1, 10, 100, 1000]
        .into_iter()
        .filter(|&s| s <= statements.len())
        .collect();

    let mut group = c.benchmark_group(dialect.dir_name());
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(2));
    group.warm_up_time(Duration::from_millis(500));

    for size in sizes {
        let subset: Vec<String> = statements.iter().take(size).cloned().collect();
        let sql = concatenate_statements(&subset);
        for &parser in &parsers {
            group.bench_with_input(BenchmarkId::new(parser.name(), size), &sql, |b, sql| {
                b.iter(|| black_box(parser.accepts(black_box(sql), dialect)));
            });
        }
    }
    group.finish();
}

fn all_dialects(c: &mut Criterion) {
    // Several parsers panic on edge-case SQL; accepts() catches it. Silence the
    // default hook so benchmark output stays clean.
    std::panic::set_hook(Box::new(|_| {}));
    for &dialect in DIALECTS {
        bench_dialect(c, dialect);
    }
}

criterion_group!(benches, all_dialects);
criterion_main!(benches);

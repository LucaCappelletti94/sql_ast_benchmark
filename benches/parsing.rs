use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode};
use sql_ast_benchmark::{
    concatenate_statements, load_delete_statements, load_dml_statements, load_insert_statements,
    load_select_statements, load_update_statements,
};
use sql_parse::{parse_statements, Issues, ParseOptions, SQLDialect};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::hint::black_box;
use std::time::Duration;

fn bench_sqlparser(sql: &str) {
    let dialect = PostgreSqlDialect {};
    let _ = black_box(Parser::parse_sql(&dialect, sql));
}

#[cfg(feature = "pg_query_parser")]
fn bench_pg_query(sql: &str) {
    let _ = black_box(pg_query::parse(sql));
}

#[cfg(feature = "pg_query_parser")]
fn bench_pg_query_summary(sql: &str) {
    let _ = black_box(pg_query::summary(sql, -1));
}

#[cfg(feature = "pg_parse_parser")]
fn bench_pg_parse(sql: &str) {
    let _ = black_box(pg_parse::parse(sql));
}

fn bench_sql_parse(sql: &str) {
    let options = ParseOptions::new().dialect(SQLDialect::PostgreSQL);
    let mut issues = Issues::new(sql);
    let _ = black_box(parse_statements(sql, &mut issues, &options));
}

fn run_benchmark_group(c: &mut Criterion, group_name: &str, statements: &[String]) {
    if statements.is_empty() {
        eprintln!("Warning: No statements for group '{group_name}', skipping");
        return;
    }

    // Build sizes list, adding max count if between 500 and 1000
    let base_sizes = [1, 10, 50, 100, 500, 1000];
    let max_count = statements.len();
    let sizes: Vec<usize> = if max_count > 500 && max_count < 1000 {
        base_sizes
            .iter()
            .copied()
            .filter(|&s| s <= max_count)
            .chain(std::iter::once(max_count))
            .collect()
    } else {
        base_sizes.to_vec()
    };
    let mut group = c.benchmark_group(group_name);

    // Reduce sample size for faster benchmarks with large datasets
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(50);
    group.measurement_time(Duration::from_secs(3));

    for size in &sizes {
        let size = *size;

        let subset: Vec<String> = statements.iter().take(size).cloned().collect();
        let concatenated = concatenate_statements(&subset);

        group.bench_with_input(
            BenchmarkId::new("sqlparser", size),
            &concatenated,
            |b, sql| b.iter(|| bench_sqlparser(sql)),
        );

        #[cfg(feature = "pg_query_parser")]
        group.bench_with_input(
            BenchmarkId::new("pg_query", size),
            &concatenated,
            |b, sql| b.iter(|| bench_pg_query(sql)),
        );

        #[cfg(feature = "pg_query_parser")]
        group.bench_with_input(
            BenchmarkId::new("pg_query_summary", size),
            &concatenated,
            |b, sql| b.iter(|| bench_pg_query_summary(sql)),
        );

        #[cfg(feature = "pg_parse_parser")]
        group.bench_with_input(
            BenchmarkId::new("pg_parse", size),
            &concatenated,
            |b, sql| b.iter(|| bench_pg_parse(sql)),
        );

        group.bench_with_input(
            BenchmarkId::new("sql_parse", size),
            &concatenated,
            |b, sql| b.iter(|| bench_sql_parse(sql)),
        );
    }

    group.finish();
}

fn select_benchmark(c: &mut Criterion) {
    let statements = load_select_statements();
    run_benchmark_group(c, "select", &statements);
}

fn insert_benchmark(c: &mut Criterion) {
    let statements = load_insert_statements();
    run_benchmark_group(c, "insert", &statements);
}

fn update_benchmark(c: &mut Criterion) {
    let statements = load_update_statements();
    run_benchmark_group(c, "update", &statements);
}

fn delete_benchmark(c: &mut Criterion) {
    let statements = load_delete_statements();
    run_benchmark_group(c, "delete", &statements);
}

fn dml_benchmark(c: &mut Criterion) {
    let statements = load_dml_statements();
    run_benchmark_group(c, "dml", &statements);
}

criterion_group!(
    benches,
    select_benchmark,
    insert_benchmark,
    update_benchmark,
    delete_benchmark,
    dml_benchmark
);
criterion_main!(benches);

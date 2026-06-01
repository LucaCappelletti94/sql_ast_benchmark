# SQL Parser Benchmark

[![CI](https://github.com/LucaCappelletti94/sql_ast_benchmark/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/sql_ast_benchmark/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/LucaCappelletti94/sql_ast_benchmark/graph/badge.svg)](https://codecov.io/gh/LucaCappelletti94/sql_ast_benchmark)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021_edition-orange.svg)](https://www.rust-lang.org)

Benchmarking Rust SQL parsers on a real-world corpus of 311,594 statements across 13 SQL dialects. Each parser runs in its best-matching dialect, and correctness is graded against a real reference parser where one exists.

An interactive explorer (every dialect's distribution charts, correctness, and per-file coverage) is published at <https://sql-ast-benchmark.luca.phd>.

See [CHANGELOG.md](CHANGELOG.md) for the project history.

## Abstract

Choosing a SQL parser for a Rust project means weighing dialect coverage, correctness, and speed, yet those trade-offs are seldom measured on realistic input. This project benchmarks the actively maintained Rust SQL parsers on a large, multi-dialect corpus of real-world statements so the choice can rest on evidence rather than on each library's own claims.

Eight parser libraries are evaluated: [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) (Apache DataFusion), [pg_query.rs](https://github.com/pganalyze/pg_query.rs) and its faster summary mode (Rust bindings to [libpg_query](https://github.com/pganalyze/libpg_query), PostgreSQL's own parser), [databend-common-ast](https://crates.io/crates/databend-common-ast), [polyglot-sql](https://github.com/tobilg/polyglot), [sqlglot-rust](https://crates.io/crates/sqlglot-rust), [qusql-parse](https://crates.io/crates/qusql-parse), and [sqlite3-parser](https://crates.io/crates/sqlite3-parser) (lemon-rs), with [orql](https://codeberg.org/xitep/orql) added on Oracle. The corpus holds 311,594 statements across 13 dialects, drawn from each engine's own regression suites and official samples and committed compressed for reproducibility.

Every parser runs in the dialect that matches the corpus under test. Where a ground-truth parser exists, [libpg_query](https://github.com/pganalyze/libpg_query) for PostgreSQL and [lemon-rs](https://github.com/gwenn/lemon-rs) for SQLite, it labels each statement valid or invalid, and parsers are scored on recall (valid statements accepted), false positives (invalid statements wrongly accepted), display round-trip stability, and canonical-form fidelity. The remaining dialects have no reference, so statements count as provenance-valid and the metric is acceptance rate. Speed is reported as a per-statement parse-time distribution over every accepted statement.

The reference bindings are exact on their home dialect by construction. Among the pure-Rust parsers, [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) is the most broadly capable, while permissive parsers such as [polyglot-sql](https://github.com/tobilg/polyglot) accept the most statements but at a high false-positive rate, and stricter parsers trade coverage for precision. Speed varies by more than an order of magnitude: median parse times run from well under a microsecond for the fastest parsers to the low single-digit microseconds for most, with [polyglot-sql](https://github.com/tobilg/polyglot) a clear outlier at around fifteen microseconds per statement. Coverage, false-positive behavior, and speed together separate the parsers.

## Parsers Under Test

| Parser | Version | Source | Implementation | Dialects |
| --- | --- | --- | --- | --- |
| **[sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs)** | 0.62.0 | git [`182eae8`](https://github.com/sqlparser-rs/sqlparser-rs/commit/182eae8191962985d3e668895c66841e420d6258) | Pure Rust, handwritten recursive descent | 14 dedicated dialects |
| **[sqlglot-rust](https://crates.io/crates/sqlglot-rust)** | 0.9.37 | crates.io | Pure Rust, standalone port of Python sqlglot | 30 (parser currently dialect-agnostic) |
| **[polyglot-sql](https://github.com/tobilg/polyglot)** | 0.4.1 | git [`dbdead6`](https://github.com/tobilg/polyglot/commit/dbdead65405449825923b3834a09bfc0d2c8bc4e) | Pure Rust, transpiler | 32 |
| **[pg_query.rs](https://github.com/pganalyze/pg_query.rs)** | 6.1.1 | git [`7e189a9`](https://github.com/pganalyze/pg_query.rs/commit/7e189a9dd1d4e441a2d44e6655c793f101bba3fa) | Rust FFI to C (libpg_query) | PostgreSQL (the PostgreSQL reference) |
| **[qusql-parse](https://crates.io/crates/qusql-parse)** | 0.8.0 | crates.io | Pure Rust, zero-copy | PostgreSQL, MariaDB/MySQL, SQLite |
| **[databend-common-ast](https://github.com/datafuselabs/databend)** | 0.2.5 | crates.io | Pure Rust, zero-copy, Pratt | PostgreSQL, MySQL, Hive |
| **[sqlite3-parser](https://crates.io/crates/sqlite3-parser)** (lemon-rs) | 0.16.0 | crates.io | Generated from SQLite's Lemon grammar | SQLite (the SQLite reference) |
| **[orql](https://codeberg.org/xitep/orql)** | 0.1.0 | git [`6a5391b`](https://codeberg.org/xitep/orql/commit/6a5391b1b11f5771ab15e4ba519bdf00fdacc021) | Pure Rust, early-stage | Oracle (SELECT only) |

### Parser notes

sqlparser-rs is the most widely adopted Rust SQL parser (part of Apache DataFusion), with dedicated dialects for most engines. Here Trino maps to its Generic dialect and Spark SQL to Databricks.

sqlglot-rust is a from-scratch Rust port of Python's sqlglot with a full AST, SQL generator and optimizer. Its parser is dialect-agnostic in this version (the dialect only affects generation), so acceptance does not vary by dialect.

polyglot-sql is a parsing/formatting/transpilation library covering 32 dialects. It is very permissive, so its high acceptance comes with a high false-positive rate on reference-backed dialects, and the two should be read together.

pg_query.rs provides Rust bindings to libpg_query, which embeds PostgreSQL's actual parser, making it the reference for the PostgreSQL corpus. Its summary mode (`pg_query::summary()`) extracts metadata without deserializing the full AST, giving the same accept/reject decisions as a full parse but much faster.

qusql-parse is a zero-copy hand-coded parser for MySQL/MariaDB, PostgreSQL and (since 0.8) SQLite. It can `todo!()`-panic on unimplemented paths, which the benchmark treats as a parse failure.

databend-common-ast is a custom parser built by the Databend team for speed, exercised here in PostgreSQL, MySQL and Hive modes.

sqlite3-parser (lemon-rs) is generated from SQLite's own Lemon grammar, making it the reference for the SQLite corpus.

orql is an early-stage Oracle-dialect parser, SELECT only, included at its [author's request](https://github.com/LucaCappelletti94/sql_ast_benchmark/issues/1). It has no pretty-printer, so round-trip and fidelity are N/A.

## Methodology

### Multi-dialect mapping

The corpus is organised as `datasets/{dialect}/{name}.txt`, one statement per line. Each dialect is parsed by every parser that models it, in its matching dialect setting, with the rest reported as N/A. The Oracle corpus, for example, is parsed by sqlparser-rs, polyglot-sql, sqlglot-rust and orql, but not by the PostgreSQL-, SQLite- or MySQL-only parsers.

### Defining "correct"

There is no universal reference across dialects, so correctness is defined per dialect.

PostgreSQL is graded against pg_query (libpg_query, the actual PostgreSQL parser) and SQLite against sqlite3-parser / lemon-rs. The reference splits the corpus into valid and invalid, and each parser is scored on four metrics: recall (reference-valid statements accepted, higher better), false positives (reference-invalid statements wrongly accepted, lower better), round-trip (parse, print, re-parse, re-print is stable on accepted-valid, N/A without a printer), and fidelity (the reference's canonical form of the output matches that of the input, capturing semantics rather than mere self-consistency, N/A without a printer).

The other 11 dialects have no reference. Their statements come from each dialect's own test suites and official samples, so they count as provenance-valid, and the metric is acceptance rate plus round-trip stability.

### Performance

The performance benchmark (`cargo bench`) times each accepted statement individually to build a per-statement parse-time distribution per (parser, dialect). Timing uses an adaptive iteration count (best of several rounds) on a no-`catch_unwind` path, so panic-guard overhead is excluded. Every accepted statement is timed, no sampling.

Raw times go to `target/bench_dist/{dialect}__{parser}.txt` and percentiles plus the round-trip rate to `summary.csv`, so plots regenerate without re-running. `cargo run --release --bin sqlbench plot` renders the eCDF and box-plot SVGs.

## Dataset Corpus

311,594 statements across 34 files and 13 dialects, committed compressed as `datasets.tar.zst` (5.3 MB) and unpacked to `datasets/{dialect}/{name}.txt`. The commands below extract it automatically on first use, so no manual step is needed (to unpack it by hand anyway, run `tar --zstd -xf datasets.tar.zst`).

The original fetch/extraction tooling has been removed (see git history). The compressed corpus is the source of truth.

| Dialect | Files | Statements | Example sources |
| --- | ---: | ---: | --- |
| clickhouse | 2 | 92,268 | ClickHouse stateless tests, ClickBench |
| hive | 2 | 41,294 | Apache Hive clientpositive, hive-testbench |
| duckdb | 3 | 41,148 | DuckDB test/sql + benchmark, DataFusion slt |
| mysql | 4 | 30,220 | TiDB integration tests, Dolt, employees db, TPC-H |
| postgresql | 3 | 29,402 | PostgreSQL regress suite + contrib + test modules, defog-ai |
| oracle | 2 | 21,648 | Oracle sample schemas and examples |
| tsql | 2 | 14,782 | ANTLR T-SQL examples, Stack Exchange Data Explorer |
| spark_sql | 3 | 14,464 | Spark sql-tests, ClickBench, spark-sql-perf |
| sqlite | 2 | 12,119 | Spider, sql-create-context |
| multi | 1 | 10,962 | sqlfluff dialect fixtures |
| redshift | 3 | 2,992 | redshift-benchmarks, redshift-utils, ClickBench |
| bigquery | 2 | 224 | TPC for BigQuery, ClickBench |
| trino | 2 | 71 | Trino parser tests, ClickBench |

All sources are openly licensed (Apache-2.0, MIT, BSD, public domain or CC-BY). Natural-language-with-embedded-SQL datasets are intentionally excluded.

## Correctness Results

Run on the full corpus with:

```bash
cargo run --release --bin sqlbench correctness
```

### PostgreSQL (reference: pg_query / libpg_query)

27,844 reference-valid and 1,558 reference-invalid statements.

| Parser | Recall | False positives | Round-trip | Fidelity |
| --- | ---: | ---: | ---: | ---: |
| pg_query.rs (reference) | 100.0% | 0.0% | 99.9% | 99.9% |
| pg_query (summary) | 100.0% | 0.0% | N/A | N/A |
| sqlparser-rs | 88.6% | 11.0% | 100.0% | 98.8% |
| polyglot-sql | 84.6% | 27.5% | 98.6% | 91.1% |
| qusql-parse | 76.5% | 11.2% | N/A | N/A |
| sqlglot-rust | 54.7% | 1.3% | 99.7% | 94.9% |
| databend-common-ast | 46.4% | 6.4% | 99.8% | 86.4% |

### SQLite (reference: sqlite3-parser / lemon-rs)

12,040 reference-valid and 79 reference-invalid statements.

| Parser | Recall | False positives | Round-trip | Fidelity |
| --- | ---: | ---: | ---: | ---: |
| sqlite3-parser (reference) | 100.0% | 0.0% | 100.0% | 100.0% |
| sqlparser-rs | 99.9% | 25.3% | 100.0% | 100.0% |
| polyglot-sql | 99.9% | 29.1% | 99.6% | 92.3% |
| qusql-parse | 99.2% | 1.3% | N/A | N/A |
| sqlglot-rust | 99.2% | 17.7% | 100.0% | 73.4% |

### Provenance dialects (acceptance rate, no reference)

Fraction of each dialect's own corpus accepted, parser run in its matching dialect, with round-trip stability in parentheses.

| Dialect (stmts) | sqlparser-rs | polyglot-sql | sqlglot-rust | dialect-specific |
| --- | ---: | ---: | ---: | --- |
| clickhouse (92,268) | 79.5% (100%) | 99.9% (98%) | 56.0% (99%) | -- |
| hive (41,294) | 80.5% (100%) | 84.3% (97%) | 45.6% (100%) | databend 51.7% |
| duckdb (41,148) | 92.1% (100%) | 93.6% (99%) | 63.0% (99%) | -- |
| mysql (30,220) | 75.8% (99%) | 76.1% (98%) | 46.7% (100%) | qusql 64.2%, databend 54.9% |
| oracle (21,648) | 59.2% (100%) | 59.6% (100%) | 54.3% (100%) | orql 0.3% (SELECT only) |
| tsql (14,782) | 72.1% (100%) | 74.8% (99%) | 53.9% (99%) | -- |
| spark_sql (14,464) | 85.1% (100%) | 91.7% (96%) | 58.9% (100%) | -- |
| multi (10,962) | 45.6% (100%) | 67.0% (98%) | 16.4% (99%) | -- |
| redshift (2,992) | 92.1% (100%) | 91.5% (99%) | 79.4% (100%) | -- |
| bigquery (224) | 99.1% (100%) | 99.1% (73%) | 75.0% (92%) | -- |
| trino (71) | 98.6% (100%) | 98.6% (100%) | 56.3% (100%) | -- |

### Key correctness findings

pg_query and sqlite3-parser are the reference, scoring 100% by construction, and are the right choice when correctness must equal what the database accepts.

sqlparser-rs has the best balance among the general-purpose parsers: high recall (88.6% PostgreSQL, 99.9% SQLite) and the highest pure-Rust fidelity (98.8% PostgreSQL, 100% SQLite), with a moderate false-positive rate (11.0% PostgreSQL, 25.3% SQLite) from dialects looser than the real databases. polyglot-sql is the most permissive, posting very high recall (99.9% SQLite and ClickHouse) but the highest PostgreSQL false-positive rate (27.5%).

sqlglot-rust is the most conservative on PostgreSQL (54.7% recall, 1.3% false positives) with strong fidelity (94.9%), but its lowest SQLite fidelity (73.4%) shows its SQLite reprints often diverge from the original. Its parser is dialect-agnostic in this version, so acceptance is uniform across dialects. databend-common-ast has moderate recall and a low false-positive rate, reflecting its Databend/ClickHouse focus.

The hardest provenance dialects for the general-purpose parsers are the mixed `multi` fixtures (~46%, intentionally cross-dialect) and Oracle (~59%, SQL\*Plus syntax), while BigQuery, Trino and Redshift sit above 90% and MySQL and T-SQL land in the low-to-mid 70s.

## Coverage Results

A per-file acceptance breakdown (every dataset file, every supporting parser, in the dialect's matching dialect) is produced by:

```bash
cargo run --release --bin sqlbench correctness --per-file
```

## Performance Results

Both views have one subplot per dialect (titled with its statement count) and a per-dialect legend pairing each parser with two quality metrics: `fail%` (share of the corpus rejected) and `RT%` (Display round-trip stability among accepted, `n/a` without a printer).

In the eCDF view, x = ns per statement (log) and y = fraction of accepted statements parsed within that time, so a curve further left is faster.

![Benchmark results (eCDF)](benchmark_results.svg)

The box-plot view summarises the same per-statement distributions (box = p25/median/p75, whiskers = p10/p90, log-y).

![Benchmark results (box plots)](benchmark_results_boxplot.svg)

PostgreSQL example (ns per statement). Each parser's `fail%`/`RT%` is over its own accepted set, so rows are not comparable on volume:

| Parser | median | p90 | fail% | RT% |
| --- | ---: | ---: | ---: | ---: |
| qusql-parse | 509 | 1,273 | 27% | N/A |
| sqlglot-rust | 1,455 | 3,494 | 48% | 100% |
| pg_query (summary) | 1,881 | 3,808 | 5% | N/A |
| sqlparser-rs | 4,254 | 12,034 | 15% | 100% |
| databend-common-ast | 6,949 | 18,168 | 56% | 100% |
| pg_query.rs (full) | 8,962 | 25,692 | 5% | 100% |
| polyglot-sql | 12,502 | 17,844 | 18% | 99% |

qusql-parse and sqlglot-rust are the fastest per statement. The libpg_query FFI (`pg_query.rs`) is the slowest full parser per call, `pg_query (summary)` is much faster because it skips AST deserialization, and polyglot-sql has a high, flat floor (~9-15 us everywhere) from fixed per-call overhead.

Speed trades off against coverage: the quickest parsers reject the most (qusql-parse 27%, sqlglot-rust 48% of the PostgreSQL corpus), while sqlparser-rs and the pg_query family accept far more at a higher cost. Round-trip is stable wherever a printer exists (~99-100%, BigQuery polyglot-sql the low outlier).

## Running

The corpus auto-extracts on first use, so just run:

```bash
cargo run --release --bin sqlbench correctness --per-file    # per-file acceptance, every dialect
cargo run --release --bin sqlbench correctness               # reference + provenance correctness
cargo bench                                                  # parse-throughput, every dialect
```

The build uses pg_query (libpg_query, the PostgreSQL server's parser) as the PostgreSQL reference.

## Environment

Results were produced on an AMD Ryzen Threadripper PRO 5975WX (32 cores, 64 threads) running Ubuntu 24.04 (Linux 6.17), with stable Rust (2021 edition).

### System requirements

- Rust toolchain (stable, 2021 edition)
- A C compiler and `libclang` for the FFI parser (pg_query builds libpg_query from source)

On Ubuntu/Debian:

```bash
sudo apt install build-essential libclang-dev
```

## Notes on robustness

Deeply nested SQL can overflow the stack in recursive-descent parsers, and a stack overflow aborts the process (uncatchable by `catch_unwind`), so the runners parse on 512 MiB worker threads. Parsers that panic on edge cases (qusql-parse, polyglot-sql, databend, sqlite3-parser, sqlglot-rust, orql) are wrapped in `catch_unwind`, treating a panic as a parse failure.

## Reproducibility

Git dependencies track each parser's latest commit. The hashes in the Parsers Under Test table identify the exact versions benchmarked. To pin them, replace the git dependencies in `Cargo.toml` with crates.io versions.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow (hooks, formatting, lints, coverage).

## License

MIT

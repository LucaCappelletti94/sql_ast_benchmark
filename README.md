# SQL Parser Benchmark

[![CI](https://github.com/LucaCappelletti94/sql_ast_benchmark/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/sql_ast_benchmark/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021_edition-orange.svg)](https://www.rust-lang.org)

Benchmarking Rust SQL parsers on a real-world, multi-dialect corpus of 321,128 statements across 13 SQL dialects (PostgreSQL, MySQL, SQLite, ClickHouse, DuckDB, Hive, Spark SQL, Trino, T-SQL, Oracle, BigQuery, Redshift, plus a mixed-dialect set). Each parser is run in its best-matching dialect, and "correct" is graded against a real reference parser where one exists.

## What changed in the May 2026 refresh

- All benchmarked crates were updated to their latest versions (sqlparser 0.62, polyglot-sql 0.4.1, qusql-parse 0.8, databend-common-ast 0.2.5, pg_parse 0.14, pg_query and orql to latest commits).
- Three parsers were added: **sqlglot-rust** (standalone 30-dialect parser), **sqlite3-parser / lemon-rs** (SQLite's real Lemon grammar), and **senax-mysql-parser** (MySQL CREATE TABLE only).
- The benchmark went from PostgreSQL-only to **multi-dialect**: every parser is now run in the dialect that matches the corpus it is being tested against.
- The corpus was expanded from a few thousand PostgreSQL statements to 321,128 statements over 13 dialects, now shipped pre-built and compressed as `datasets.tar.zst`.

## Parsers Under Test

| Parser | Version | Source | Implementation | Dialects |
| --- | --- | --- | --- | --- |
| **[sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs)** | 0.62.0 | git [`182eae8`](https://github.com/sqlparser-rs/sqlparser-rs/commit/182eae8191962985d3e668895c66841e420d6258) | Pure Rust, handwritten recursive descent | 14 dedicated dialects |
| **[sqlglot-rust](https://crates.io/crates/sqlglot-rust)** | 0.9.28 | crates.io | Pure Rust, standalone port of Python sqlglot | 30 (parser currently dialect-agnostic) |
| **[polyglot-sql](https://github.com/tobilg/polyglot)** | 0.4.1 | git [`dbdead6`](https://github.com/tobilg/polyglot/commit/dbdead65405449825923b3834a09bfc0d2c8bc4e) | Pure Rust, transpiler | 32 |
| **[pg_query.rs](https://github.com/pganalyze/pg_query.rs)** | 6.1.1 | git [`7e189a9`](https://github.com/pganalyze/pg_query.rs/commit/7e189a9dd1d4e441a2d44e6655c793f101bba3fa) | Rust FFI to C (libpg_query) | PostgreSQL (the PostgreSQL oracle) |
| **[pg_parse](https://github.com/paupino/pg_parse)** | 0.14.0 | git [`4e7f10c`](https://github.com/paupino/pg_parse/commit/4e7f10ce401337f070b82bfc267c5aa38b262e25) | Rust FFI to C (libpg_query), alternate API | PostgreSQL (optional feature) |
| **[qusql-parse](https://crates.io/crates/qusql-parse)** | 0.8.0 | crates.io | Pure Rust, zero-copy | PostgreSQL, MariaDB/MySQL, SQLite |
| **[databend-common-ast](https://github.com/datafuselabs/databend)** | 0.2.5 | crates.io | Pure Rust, zero-copy, Pratt | PostgreSQL, MySQL, Hive |
| **[sqlite3-parser](https://crates.io/crates/sqlite3-parser)** (lemon-rs) | 0.16.0 | crates.io | Generated from SQLite's Lemon grammar | SQLite (the SQLite oracle) |
| **[orql](https://codeberg.org/xitep/orql)** | 0.1.0 | git [`6a5391b`](https://codeberg.org/xitep/orql/commit/6a5391b1b11f5771ab15e4ba519bdf00fdacc021) | Pure Rust, early-stage | Oracle (SELECT only) |
| **[senax-mysql-parser](https://crates.io/crates/senax-mysql-parser)** | 0.1.10 | crates.io | Pure Rust, nom | MySQL **CREATE TABLE only** |

### Parser notes

- **sqlparser-rs**: the most widely adopted Rust SQL parser (part of Apache DataFusion). Has dedicated dialects for PostgreSQL, MySQL, SQLite, ClickHouse, Hive, MsSql, MySql, Oracle, Redshift, BigQuery, DuckDB, Databricks, Snowflake, ANSI and Generic. Trino maps to Generic and Spark SQL maps to Databricks in this benchmark.
- **sqlglot-rust**: a from-scratch Rust port of Python's sqlglot with a full AST, a SQL generator (so it round-trips), and an optimizer. In this version the parser itself is dialect-agnostic (the dialect argument only affects generation), so its acceptance rate does not change between dialects.
- **polyglot-sql**: a SQL parsing, formatting and transpilation library covering 32 dialects. It is very permissive: on oracle-backed dialects it has a high false-positive rate (it accepts large amounts of SQL the reference parser rejects), so high acceptance numbers should be read alongside its false-positive rate.
- **pg_query.rs**: Rust bindings to libpg_query, which embeds PostgreSQL's actual parser. It is the **ground-truth oracle for the PostgreSQL corpus**, not just another parser under test.
- **pg_query.rs (summary)**: `pg_query::summary()` extracts metadata without deserializing the full AST. Same accept/reject decisions as full parse, much faster.
- **qusql-parse**: a zero-copy hand-coded parser focused on MySQL/MariaDB and PostgreSQL, with SQLite support added in 0.8. Can `todo!()`-panic on some unimplemented paths, which the benchmark treats as a parse failure.
- **databend-common-ast**: a custom parser built by the Databend team for speed. PostgreSQL, MySQL and Hive modes are exercised here.
- **sqlite3-parser (lemon-rs)**: generated from SQLite's own Lemon grammar, so it is the closest thing to real-SQLite parity in Rust. It is the **ground-truth oracle for the SQLite corpus**.
- **orql**: an early-stage Oracle-dialect parser, SELECT only, included at its [author's request](https://github.com/LucaCappelletti94/sql_ast_benchmark/issues/1). No pretty-printer, so round-trip and fidelity are N/A.
- **senax-mysql-parser**: the only maintained descendant of nom-sql, but it parses **CREATE TABLE statements only**. It therefore reads close to 0% on SELECT-heavy corpora and only registers on DDL. It is included as a DDL-only data point and labeled as such everywhere.

## Methodology

### Multi-dialect mapping

The corpus is organised as `datasets/{dialect}/{name}.txt`, one statement per line. For each dialect, every parser that models that dialect is run with its matching dialect setting. Parsers that do not model a dialect are reported as N/A for it. For example the Oracle corpus is parsed by sqlparser-rs (Oracle dialect), polyglot-sql (Oracle), sqlglot-rust and orql, but not by pg_query, qusql-parse, databend, sqlite3-parser or senax.

### Defining "correct"

There is no universal oracle across dialects, so correctness is defined per dialect:

- **Oracle-backed dialects.** PostgreSQL is graded against **pg_query** (libpg_query, the actual PostgreSQL parser) and SQLite against **sqlite3-parser / lemon-rs** (SQLite's real grammar). The oracle splits the corpus into valid (oracle accepts) and invalid (oracle rejects), and every parser is scored on four metrics:
  - **Recall**: of oracle-valid statements, how many the parser accepts. Higher is better.
  - **False positives**: of oracle-invalid statements, how many the parser wrongly accepts. Lower is better.
  - **Round-trip**: of accepted-valid statements, whether parse, print, re-parse, re-print is stable. Higher is better. N/A without a printer.
  - **Fidelity**: of accepted-valid statements, whether the oracle's canonical form of the parser's output equals the oracle's canonical form of the input (semantic preservation, not just self-consistency). Higher is better. N/A without a printer.
- **Provenance dialects.** The other 11 dialects have no oracle. Statements are drawn from each dialect's own test suites and official samples, so they are treated as provenance-valid and the metric is **acceptance rate** (fraction of the corpus the parser accepts in its matching dialect) plus **round-trip** stability.

### Performance

The performance benchmark (`cargo bench`) is keyed to each parser's accepted set. For every (parser, dialect) pair it: builds the set of statements that parser accepts in that dialect, times each accepted statement individually to produce a per-statement parse-time distribution, and separately times the whole accepted body concatenated and divides by the statement count. Keying on the accepted set means the concatenated parse never stops early on a statement the parser would reject, so the normalized concatenated number is a fair amortized-throughput figure. Per-statement timing uses an adaptive iteration count (best of several rounds) and a no-`catch_unwind` parse path, so the measurement is free of panic-guard overhead. Every accepted statement is timed (no sampling); the full corpus is covered.

Raw per-statement times are written to `target/bench_dist/{dialect}__{parser}.txt` and percentiles plus the normalized concatenated time to `target/bench_dist/summary.csv`, so plots can be regenerated without re-running. `cargo run --release --bin plot` renders `benchmark_results.svg`: one subplot per dialect with an empirical CDF (eCDF) line per parser (x = per-statement time in ns on a log scale, y = fraction of that parser's accepted statements parsed within that time), and a triangle on the x-axis marking the concatenated-normalized time.

## Dataset Corpus

321,128 statements across 34 files and 13 dialects. The corpus is committed pre-built and compressed as `datasets.tar.zst` (5.3 MB). Extract it before running anything:

```bash
tar --zstd -xf datasets.tar.zst   # produces datasets/{dialect}/{name}.txt
```

The original fetch/extraction tooling that scraped these statements from upstream repositories and datasets has been removed; the compressed corpus is the source of truth (see git history for the downloader).

| Dialect | Files | Statements | Example sources |
| --- | ---: | ---: | --- |
| clickhouse | 2 | 92,229 | ClickHouse stateless tests, ClickBench |
| hive | 2 | 41,279 | Apache Hive clientpositive, hive-testbench |
| duckdb | 3 | 41,098 | DuckDB test/sql + benchmark, DataFusion slt |
| mysql | 4 | 30,202 | TiDB integration tests, Dolt, employees db, TPC-H |
| postgresql | 3 | 29,402 | PostgreSQL regress suite + contrib + test modules, defog-ai |
| sqlite | 3 | 24,244 | Spider, sql-create-context, SEDE |
| oracle | 2 | 21,820 | Oracle sample schemas and examples |
| spark_sql | 3 | 14,451 | Spark sql-tests, ClickBench, spark-sql-perf |
| multi | 1 | 10,935 | sqlfluff dialect fixtures |
| tsql | 2 | 6,710 | SQL Server samples, ANTLR T-SQL examples |
| bigquery | 3 | 5,224 | BiomedSQL, TPC for BigQuery, ClickBench |
| redshift | 3 | 2,992 | redshift-benchmarks, redshift-utils, ClickBench |
| trino | 3 | 380 | Trino product tests, parser tests, ClickBench |

All sources are openly licensed (Apache-2.0, MIT, BSD, public domain or CC-BY). Natural-language-with-embedded-SQL datasets are intentionally excluded.

## Correctness Results

Run on the full corpus with:

```bash
cargo run --release --bin correctness
```

### PostgreSQL (oracle: pg_query / libpg_query)

27,844 oracle-valid and 1,558 oracle-invalid statements.

| Parser | Recall | False positives | Round-trip | Fidelity |
| --- | ---: | ---: | ---: | ---: |
| pg_query.rs (oracle) | 100.0% | 0.0% | 99.9% | 99.9% |
| pg_query (summary) | 100.0% | 0.0% | N/A | N/A |
| sqlparser-rs | 88.6% | 11.0% | 100.0% | 98.8% |
| polyglot-sql | 84.6% | 27.5% | 98.6% | 91.1% |
| qusql-parse | 76.5% | 11.2% | N/A | N/A |
| sqlglot-rust | 54.7% | 1.3% | 99.7% | 94.9% |
| databend-common-ast | 46.4% | 6.4% | 99.8% | 86.4% |

### SQLite (oracle: sqlite3-parser / lemon-rs)

18,306 oracle-valid and 5,938 oracle-invalid statements.

| Parser | Recall | False positives | Round-trip | Fidelity |
| --- | ---: | ---: | ---: | ---: |
| sqlite3-parser (oracle) | 100.0% | 0.0% | 100.0% | 100.0% |
| sqlparser-rs | 99.9% | 30.4% | 100.0% | 93.6% |
| polyglot-sql | 98.6% | 57.6% | 99.6% | 68.0% |
| sqlglot-rust | 95.8% | 22.4% | 98.1% | 56.1% |
| qusql-parse | 83.5% | 0.7% | N/A | N/A |

### Provenance dialects (acceptance rate, no oracle)

Acceptance rate = fraction of each dialect's own corpus accepted, with the parser run in its matching dialect. Round-trip stability of accepted statements is in parentheses.

| Dialect (stmts) | sqlparser-rs | polyglot-sql | sqlglot-rust | dialect-specific |
| --- | ---: | ---: | ---: | --- |
| mysql (30,202) | 75.7% (99%) | 76.0% (98%) | 46.7% (100%) | qusql 64.2%, databend 54.8%, senax 7.0% (DDL only) |
| clickhouse (92,229) | 79.5% (100%) | 99.9% (98%) | 56.0% (99%) | -- |
| duckdb (41,098) | 92.1% (100%) | 93.6% (99%) | 63.1% (99%) | -- |
| hive (41,279) | 80.5% (100%) | 84.3% (97%) | 45.6% (100%) | databend 51.7% |
| spark_sql (14,451) | 85.1% (100%) | 91.7% (96%) | 58.9% (100%) | -- |
| trino (380) | 25.8% (100%) | 35.0% (98%) | 16.6% (100%) | -- |
| tsql (6,710) | 34.9% (99%) | 36.3% (98%) | 16.2% (99%) | -- |
| oracle (21,820) | 58.7% (100%) | 59.2% (100%) | 53.8% (100%) | orql 0.3% (SELECT only) |
| bigquery (5,224) | 4.2% (100%) | 4.2% (73%) | 3.2% (92%) | -- |
| redshift (2,992) | 92.1% (100%) | 91.5% (99%) | 79.4% (100%) | -- |
| multi (10,935) | 45.6% (100%) | 67.0% (98%) | 16.3% (99%) | -- |

### Key correctness findings

- **pg_query and sqlite3-parser are the oracles** and score 100% by construction. They are the right choice when correctness must equal "what the database actually accepts."
- **sqlparser-rs** has the best balance among the general-purpose parsers: high recall (88.6% PostgreSQL, 99.9% SQLite) with the highest fidelity of the pure-Rust parsers (98.8% PostgreSQL). Its false-positive rate is moderate (11.0% PostgreSQL, 30.4% SQLite): its dialects are looser than the real databases.
- **polyglot-sql** is the most permissive parser. It posts very high acceptance and recall (98.6% SQLite, 99.9% ClickHouse acceptance) but also the highest false-positive rate (27.5% PostgreSQL, 57.6% SQLite) and the lowest SQLite fidelity (68.0%): it accepts and reprints SQL that the reference parser rejects or that changes meaning.
- **sqlglot-rust** is the most conservative on PostgreSQL (54.7% recall, only 1.3% false positives) with strong fidelity (94.9% PostgreSQL). Because its parser is dialect-agnostic in this version, its acceptance is uniform across dialects rather than tuned per dialect.
- **databend-common-ast** has moderate recall and a low false-positive rate, reflecting its Databend/ClickHouse focus rather than broad PostgreSQL coverage.
- **bigquery acceptance is low for everyone** (around 3 to 4%) because the BiomedSQL portion of that corpus is dominated by BigQuery-proprietary syntax that none of these parsers model.
- **senax-mysql-parser** reads 7.0% on the MySQL corpus, consistent with it being a CREATE TABLE-only parser on a SELECT-heavy corpus.

## Coverage Results

A per-file acceptance breakdown (every dataset file, every supporting parser, in the dialect's matching dialect) is produced by:

```bash
cargo run --release --bin evaluate_datasets
```

## Performance Results

`cargo bench` times every accepted statement in every dialect (full corpus, no sampling) and writes raw per-statement times + percentiles to `target/bench_dist/`. The chart below (one subplot per dialect) shows, for each parser, an empirical CDF of per-statement parse time: x = ns per statement (log), y = fraction of that parser's accepted statements parsed within that time, so a curve further to the left is faster. A triangle on the x-axis marks the concatenated-body time normalized by statement count.

![Benchmark results](benchmark_results.svg)

PostgreSQL example (ns per statement, on the 27,844 statements pg_query accepts; `concat/n` = full accepted body parsed once, divided by n):

| Parser | median | p90 | concat / n |
| --- | ---: | ---: | ---: |
| qusql-parse | 474 | 1,235 | 162 |
| sqlglot-rust | 1,442 | 3,397 | 531 |
| pg_query (summary) | 1,824 | 3,596 | 628 |
| sqlparser-rs | 5,595 | 20,439 | 877 |
| databend-common-ast | 6,635 | 17,497 | 2,139 |
| pg_query.rs (full) | 8,236 | 23,651 | 1,694 |
| polyglot-sql | 12,128 | 16,838 | 1,037 |

Key observations:

- **Bulk amortizes strongly.** For most dialects `concat/n` is far below the per-statement median (sqlparser-rs PostgreSQL: 877 ns amortized vs 5,595 ns per call), because parsing one big body amortizes per-call setup and allocation. The exceptions are Redshift, BigQuery, and Trino, whose corpora are dominated by a few very large analytical queries, so `concat/n` is at or above the median.
- **polyglot-sql has a high, flat per-statement floor** (~9-15 us in every dialect, visible as boxes that never drop low) from fixed per-call overhead, but it amortizes well in bulk.
- **qusql-parse and sqlglot-rust are the fastest per statement**; the libpg_query FFI (`pg_query.rs`) is the slowest full parser per call but competitive in bulk, and `pg_query (summary)` is much faster than full parsing because it skips AST deserialization.
- **senax-mysql** is CREATE-TABLE-only, so its `concat/n` is meaningless (the parser consumes only the first statement of a concatenated body); read only its per-statement distribution.

## Running

```bash
tar --zstd -xf datasets.tar.zst                # extract the corpus into datasets/
cargo run --release --bin evaluate_datasets    # per-file acceptance, every dialect
cargo run --release --bin correctness          # oracle + provenance correctness
cargo bench                                     # parse-throughput, every dialect
```

The default build uses pg_query (the PostgreSQL oracle). To use the alternate libpg_query binding instead:

```bash
cargo run --release --no-default-features --features pg_parse_parser --bin correctness
```

## Environment

- **CPU**: AMD Ryzen Threadripper PRO 5975WX (32 cores, 64 threads)
- **OS**: Ubuntu 24.04 (Linux 6.17)
- **Rust**: 2021 edition (stable)

### System requirements

- Rust toolchain (stable, 2021 edition)
- A C compiler and `libclang` for the FFI parsers (pg_query / pg_parse build libpg_query from source)

On Ubuntu/Debian:

```bash
sudo apt install build-essential libclang-dev
```

## Notes on robustness

Several recursive-descent parsers can overflow the stack on deeply nested SQL, and a stack overflow aborts the process (it is not catchable by `catch_unwind`). The coverage and correctness runners therefore execute parsing on worker threads with a 512 MiB stack. Parsers that panic on edge cases (qusql-parse, polyglot-sql, databend, sqlite3-parser, sqlglot-rust, orql, senax) are wrapped in `catch_unwind`, which treats a panic as a parse failure.

## Reproducibility

Git dependencies track the latest commit of each parser; the commit hashes in the Parsers Under Test table identify the exact versions benchmarked. To pin versions, replace the git dependencies in `Cargo.toml` with crates.io version numbers.

## Development

```bash
git config core.hooksPath .githooks   # enable fmt + clippy pre-commit hooks
cargo fmt --all
cargo clippy --all-targets
```

No unsafe code is allowed (`unsafe_code = "forbid"`). Clippy runs with pedantic and nursery lints enabled.

### Coverage

```bash
tar --zstd -xf datasets.tar.zst   # the bench needs datasets/ present
cargo tarpaulin                    # LLVM engine, includes the bench
```

`tarpaulin.toml` configures the LLVM engine and runs the Criterion benchmark in verify-only mode (`--test`) as part of coverage, since the benchmark is the main exercise of the multi-dialect `BenchParser` layer. With the corpus present this covers `benches/parsing.rs` and the dialect-mapping / accept / reprint paths in `src/lib.rs`. The CLI tool binaries (`correctness`, `evaluate_datasets`, etc.) have no unit tests and read as uncovered.

## License

MIT

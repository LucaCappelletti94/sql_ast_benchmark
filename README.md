# SQL Parser Benchmark

[![CI](https://github.com/LucaCappelletti94/sql_ast_benchmark/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/sql_ast_benchmark/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/LucaCappelletti94/sql_ast_benchmark/graph/badge.svg)](https://codecov.io/gh/LucaCappelletti94/sql_ast_benchmark)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021_edition-orange.svg)](https://www.rust-lang.org)
[![Explorer](https://img.shields.io/website?url=https%3A%2F%2Fsql-ast-benchmark.luca.phd&label=explorer&up_message=online&down_message=offline)](https://sql-ast-benchmark.luca.phd)

Benchmarking Rust SQL parsers on a real-world corpus of 311,594 statements across 13 SQL dialects. Each parser runs in its best-matching dialect, and correctness is graded against a real reference parser where one exists.

## Abstract

Choosing a SQL parser for a Rust project means weighing dialect coverage, correctness, and speed, yet those trade-offs are seldom measured on realistic input. We benchmarked the actively maintained Rust SQL parsers on a large, multi-dialect corpus of real-world statements so the choice can rest on evidence rather than on each library's own claims.

We evaluated eight parser libraries: [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) (Apache DataFusion), [pg_query.rs](https://github.com/pganalyze/pg_query.rs) and its faster summary mode (Rust bindings to [libpg_query](https://github.com/pganalyze/libpg_query), PostgreSQL's own parser), [databend-common-ast](https://crates.io/crates/databend-common-ast), [polyglot-sql](https://github.com/tobilg/polyglot), [sqlglot-rust](https://crates.io/crates/sqlglot-rust), [qusql-parse](https://crates.io/crates/qusql-parse), and [sqlite3-parser](https://crates.io/crates/sqlite3-parser) (lemon-rs), plus [orql](https://codeberg.org/xitep/orql) on Oracle. We ran them against a corpus of 311,594 statements spanning 13 dialects, drawn from each engine's own regression suites and official samples and committed compressed so every run is reproducible.

We exercised each parser in the dialect that matches the corpus under test. Where a dialect has a runnable engine, we labelled each statement valid or invalid with the real database engine itself, run in Docker via [testcontainers](https://github.com/testcontainers/testcontainers-rs): a statement counts as valid unless the engine reports a syntax error, so a missing table or column still counts as parsed. Against that ground truth we scored the parsers on recall (valid statements accepted), false positives (invalid statements wrongly accepted), display round-trip stability, and canonical-form fidelity. The other dialects have no runnable engine, so their statements count as provenance-valid and the metric is simply the acceptance rate. Across all dialects, we captured speed as a per-statement parse-time distribution over every accepted statement.

On their home dialect the reference bindings are exact by construction, so the more telling comparison is among the pure-Rust parsers. There, [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) is the most broadly capable, the permissive parsers such as [polyglot-sql](https://github.com/tobilg/polyglot) accept the most statements but pay for it with a high false-positive rate, and the stricter parsers reject more in exchange for precision. Speed spans more than an order of magnitude, from well under a microsecond per statement for the fastest parsers to the low single-digit microseconds for most, with [polyglot-sql](https://github.com/tobilg/polyglot) a clear outlier at roughly fifteen. No parser leads on every axis, so the right choice comes down to what a given project values most: broad coverage, few false positives, or raw speed.

## Parsers Under Test

| Parser | Version | Source | Implementation | Dialects |
| --- | --- | --- | --- | --- |
| **[sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs)** | 0.62.0 | git [`182eae8`](https://github.com/sqlparser-rs/sqlparser-rs/commit/182eae8191962985d3e668895c66841e420d6258) | Pure Rust, handwritten recursive descent | 14 dedicated dialects |
| **[sqlglot-rust](https://crates.io/crates/sqlglot-rust)** | 0.9.37 | crates.io | Pure Rust, standalone port of Python sqlglot | 30 (parser currently dialect-agnostic) |
| **[polyglot-sql](https://github.com/tobilg/polyglot)** | 0.4.1 | git [`dbdead6`](https://github.com/tobilg/polyglot/commit/dbdead65405449825923b3834a09bfc0d2c8bc4e) | Pure Rust, transpiler | 32 |
| **[pg_query.rs](https://github.com/pganalyze/pg_query.rs)** | 6.1.1 | git [`7e189a9`](https://github.com/pganalyze/pg_query.rs/commit/7e189a9dd1d4e441a2d44e6655c793f101bba3fa) | Rust FFI to C (libpg_query) | PostgreSQL |
| **[qusql-parse](https://crates.io/crates/qusql-parse)** | 0.8.0 | crates.io | Pure Rust, zero-copy | PostgreSQL, MariaDB/MySQL, SQLite |
| **[databend-common-ast](https://github.com/datafuselabs/databend)** | 0.2.5 | crates.io | Pure Rust, zero-copy, Pratt | PostgreSQL, MySQL, Hive |
| **[sqlite3-parser](https://crates.io/crates/sqlite3-parser)** (lemon-rs) | 0.16.0 | crates.io | Generated from SQLite's Lemon grammar | SQLite |
| **[orql](https://codeberg.org/xitep/orql)** | 0.1.0 | git [`6a5391b`](https://codeberg.org/xitep/orql/commit/6a5391b1b11f5771ab15e4ba519bdf00fdacc021) | Pure Rust, early-stage | Oracle (SELECT only) |

Per-parser repository metadata (stars, contributors, fuzzing, test and benchmark suites, license) is shown on each parser page in the [explorer](https://sql-ast-benchmark.luca.phd).

## Corpus

311,594 statements across 34 files and 13 dialects, committed compressed as `datasets.tar.zst` (5.3 MB) and unpacked to `datasets/{dialect}/{name}.txt`, one statement per line. The commands below extract it automatically on first use. All sources are openly licensed (Apache-2.0, MIT, BSD, public domain or CC-BY), drawn from each engine's own regression suites and official samples. Natural-language-with-embedded-SQL datasets are intentionally excluded.

Correctness is defined per dialect. Dialects with a runnable engine are graded against that real database engine, run in Docker via testcontainers by the `oracle` crate: a statement is valid unless the engine reports a syntax error (a missing table or column still counts as parsed). The validity labels are computed once and committed under `oracle/labels`, so grading and CI need no Docker. That reference splits the corpus into valid and invalid and scores recall, false positives, round-trip, and fidelity. Dialects with no runnable engine (cloud services, heavy JVM engines) have no reference, so their statements count as provenance-valid (sourced from each engine's own suites) and the metric is acceptance rate. Speed is a per-statement parse-time distribution over every accepted statement, timed with an adaptive iteration count on a no-`catch_unwind` path.

## Running

The corpus auto-extracts on first use, so just run:

```bash
cargo run --release --bin sqlbench correctness --per-file    # per-file acceptance, every dialect
cargo run --release --bin sqlbench correctness               # reference + provenance correctness
cargo bench                                                  # parse-throughput, every dialect
cargo run --release --bin sqlbench export                    # regenerate web/assets/bench.json for the explorer
```

Validity labels for the reference dialects are produced by the `oracle` crate (real engines in Docker via testcontainers) and committed under `oracle/labels`, so `correctness` and `export` need no Docker. Regenerate them with `cargo run --release -p oracle`.

### Requirements

- Rust toolchain (stable, 2021 edition)
- A C compiler and `libclang` for the FFI parser (pg_query builds libpg_query from source)

On Ubuntu/Debian: `sudo apt install build-essential libclang-dev`

Results in the explorer were produced on an AMD Ryzen Threadripper PRO 5975WX (32 cores, 64 threads) running Ubuntu 24.04.

## Notes on robustness

Deeply nested SQL can overflow the stack in recursive-descent parsers, and a stack overflow aborts the process (uncatchable by `catch_unwind`), so the runners parse on 512 MiB worker threads. Parsers that panic on edge cases are wrapped in `catch_unwind`, treating a panic as a parse failure.

## Reproducibility

Git dependencies track each parser's latest commit, and the hashes in the Parsers Under Test table identify the exact versions benchmarked. To pin them, replace the git dependencies in `Cargo.toml` with crates.io versions.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow (hooks, formatting, lints, coverage).

## License

MIT

# SQL Parser Benchmark

[![CI](https://github.com/LucaCappelletti94/sql_ast_benchmark/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/sql_ast_benchmark/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/LucaCappelletti94/sql_ast_benchmark/graph/badge.svg)](https://codecov.io/gh/LucaCappelletti94/sql_ast_benchmark)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021_edition-orange.svg)](https://www.rust-lang.org)
[![Explorer](https://img.shields.io/website?url=https%3A%2F%2Fsql-ast-benchmark.luca.phd&label=explorer&up_message=online&down_message=offline)](https://sql-ast-benchmark.luca.phd)

Benchmarking Rust SQL parsers on a real-world corpus of 340,938 statements across 13 SQL dialects. Each parser runs in its best-matching dialect, and correctness is graded against a real reference parser where one exists.

## Abstract

Choosing a SQL parser for a Rust project means weighing dialect coverage, correctness, and speed, yet those trade-offs are seldom measured on realistic input. We benchmarked the actively maintained Rust SQL parsers on a large, multi-dialect corpus of real-world statements so the choice can rest on evidence rather than on each library's own claims.

We evaluated nine parser libraries: [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) (Apache DataFusion), [pg_query.rs](https://github.com/pganalyze/pg_query.rs) and its faster summary mode (Rust bindings to [libpg_query](https://github.com/pganalyze/libpg_query), PostgreSQL's own parser), [databend-common-ast](https://crates.io/crates/databend-common-ast), [polyglot-sql](https://github.com/tobilg/polyglot), [sqlglot-rust](https://crates.io/crates/sqlglot-rust), [qusql-parse](https://crates.io/crates/qusql-parse), [sqlite3-parser](https://crates.io/crates/sqlite3-parser) (lemon-rs), and [turso_parser](https://crates.io/crates/turso_parser) (the SQLite parser from Turso), plus [orql](https://codeberg.org/xitep/orql) on Oracle. We ran them against a corpus of 340,938 statements spanning 13 dialects, drawn from each engine's own regression suites and official samples and committed compressed so every run is reproducible.

We exercised each parser in the dialect that matches the corpus under test. Where a dialect has a runnable engine, we labelled each statement valid or invalid with the real database engine itself, run in Docker via [testcontainers](https://github.com/testcontainers/testcontainers-rs): a statement counts as valid unless the engine reports a syntax error, so a missing table or column still counts as parsed. Against that ground truth we scored the parsers on recall (valid statements accepted), false positives (invalid statements wrongly accepted), and display round-trip stability. The other dialects have no runnable engine, so their statements count as provenance-valid and the metric is simply the acceptance rate. Across all dialects, we captured speed as a per-statement parse-time distribution over every accepted statement, and memory as the peak and retained bytes per statement under a counting allocator. A batch axis additionally parses each parser's whole accepted set as a single script, showing what bulk parsing amortizes, and a time machine benchmarks the historical releases of every pure-Rust parser (59 versions in total, including every sqlparser-rs minor since January 2023), so each parser page also charts how coverage, speed, and memory evolved across releases.

On their home dialect the reference bindings are exact by construction, so the more telling comparison is among the pure-Rust parsers. There, [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) is the most broadly capable, the permissive parsers such as [polyglot-sql](https://github.com/tobilg/polyglot) accept the most statements but pay for it with a high false-positive rate, and the stricter parsers reject more in exchange for precision. Speed spans more than an order of magnitude, from well under a microsecond per statement for the fastest parsers to the low single-digit microseconds for most, with [polyglot-sql](https://github.com/tobilg/polyglot) a clear outlier at roughly fifteen. No parser leads on every axis, so the right choice comes down to what a given project values most: broad coverage, few false positives, or raw speed.

## Parsers Under Test

| Parser | Version | Source | Implementation | Dialects |
| --- | --- | --- | --- | --- |
| **[sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs)** | 0.62.0 | git [`b376022`](https://github.com/sqlparser-rs/sqlparser-rs/commit/b3760221) | Pure Rust, handwritten recursive descent | 14 dedicated dialects |
| **[sqlglot-rust](https://crates.io/crates/sqlglot-rust)** | 0.10.1 | crates.io | Pure Rust, standalone port of Python sqlglot | 30 (parser currently dialect-agnostic) |
| **[polyglot-sql](https://github.com/tobilg/polyglot)** | 0.5.1 | git [`e3a8913`](https://github.com/tobilg/polyglot/commit/e3a8913a) | Pure Rust, transpiler | 32 |
| **[pg_query.rs](https://github.com/pganalyze/pg_query.rs)** | 6.1.1 | git [`7e189a9`](https://github.com/pganalyze/pg_query.rs/commit/7e189a9dd1d4e441a2d44e6655c793f101bba3fa) | Rust FFI to C (libpg_query) | PostgreSQL |
| **[qusql-parse](https://crates.io/crates/qusql-parse)** | 0.8.0 | crates.io | Pure Rust, zero-copy | PostgreSQL, MariaDB/MySQL, SQLite |
| **[databend-common-ast](https://github.com/datafuselabs/databend)** | 0.2.5 | crates.io | Pure Rust, zero-copy, Pratt | PostgreSQL, MySQL, Hive |
| **[sqlite3-parser](https://crates.io/crates/sqlite3-parser)** (lemon-rs) | 0.16.0 | crates.io | Generated from SQLite's Lemon grammar | SQLite |
| **[turso_parser](https://crates.io/crates/turso_parser)** | 0.6.1 | crates.io | Pure Rust, handwritten recursive descent over a Lemon token table | SQLite |
| **[orql](https://codeberg.org/xitep/orql)** | 0.1.0 | git [`6a5391b`](https://codeberg.org/xitep/orql/commit/6a5391b1b11f5771ab15e4ba519bdf00fdacc021) | Pure Rust, early-stage | Oracle (SELECT only) |

Per-parser repository metadata (stars, contributors, fuzzing, test and benchmark suites, license) is shown on each parser page in the [explorer](https://sql-ast-benchmark.luca.phd).

## Corpus

340,938 statements across 32 files and 13 dialects, committed compressed as `datasets.tar.zst` (5.6 MB) and unpacked to `datasets/{dialect}/{name}.txt`, one statement per line. The commands below extract it automatically on first use. All sources are openly licensed (Apache-2.0, MIT, BSD, public domain or CC-BY), drawn from each engine's own regression suites and official samples. The SQLite corpus includes the SQLite project's own official test suite (public domain), which exercises SQLite-specific grammar such as PRAGMAs, virtual tables, recursive CTEs, and upsert. Natural-language-with-embedded-SQL datasets are intentionally excluded.

Correctness is defined per dialect. Dialects with a runnable engine are graded against that real database engine, run in Docker via testcontainers by the `oracle` crate: a statement is valid unless the engine reports a syntax error (a missing table or column still counts as parsed). The validity labels are computed once and committed under `oracle/labels`, so grading and CI need no Docker. That reference splits the corpus into valid and invalid and scores recall, false positives, and round-trip. Dialects with no runnable engine (cloud services, heavy JVM engines) have no reference, so their statements count as provenance-valid (sourced from each engine's own suites) and the metric is acceptance rate. Speed is a per-statement parse-time distribution over every accepted statement, timed with an adaptive iteration count on a no-`catch_unwind` path. Memory is measured separately with a counting allocator, as peak live bytes and retained (AST) bytes per statement. A companion batch axis parses each parser's whole accepted set as one script and normalizes the time and memory by the statement count, showing what bulk parsing amortizes against parsing one statement at a time. A batch that does not parse the whole set (a parser that bails out partway) is dropped rather than reported, and parsers without a multi-statement entry point (databend-common-ast) sit out the batch axis.

## Running

The corpus auto-extracts on first use. To rebuild the whole explorer snapshot (`web/assets/bench.json.zst`) with one command:

```bash
cargo regen   # timing benches + memory benches + time-machine + export, in order
```

That is an alias (see `.cargo/config.toml`) for `cargo run --release --bin sqlbench -- regen`. The memory measurement installs a counting global allocator, so it has to run in its own process, separate from the timing bench (which must stay on the default allocator for fair numbers). The `regen` command orchestrates that sequence so you do not have to. The individual steps, if you want to run one on its own:

```bash
cargo run --release --bin sqlbench correctness --per-file       # per-file acceptance, every dialect
cargo run --release --bin sqlbench correctness                  # reference + provenance correctness
cargo bench                                                     # parse time (per-statement and batch), every dialect
cargo run --release -p membench                                 # per-statement memory (peak + retained bytes)
cargo run --release -p membench -- batch                        # whole-script (batch) memory, per statement
cargo run --release -p timemachine --bin timemachine-mem -- --full   # per-version memory (writes a sidecar)
cargo run --release -p timemachine --bin timemachine -- --full       # per-version time + correctness, writes history
cargo run --release --bin sqlbench export                       # regenerate web/assets/bench.json.zst for the explorer
```

`cargo bench` runs both the per-statement (`parsing`) and whole-script (`batch_parsing`) timing benches. Add `--bench batch_parsing` to run only the batch one. `export` reads whatever the benches left under `target/`, warning rather than failing for any missing source, so the memory and batch columns stay empty until their producers have run.

The `timemachine` crate benchmarks several historical versions of each pure-Rust parser at once (via `package`-rename aliases in `timemachine/Cargo.toml`) and writes a compressed `web/assets/history.json.zst` that the explorer embeds and decompresses in the browser. Each parser page then shows how that library's time, memory, and correctness changed across releases, with a version picker. Cargo can only host semver-incompatible versions side by side, so the milestones are the latest patch of every `0.x` minor the shared adapter compiles against: `sqlparser-rs` gets 33 points (every minor from 0.30, January 2023, through 0.62), `sqlite3-parser` eight, `qusql-parse` seven (its 0.1.0 release parses pathologically slowly on parts of the corpus and is excluded), `polyglot-sql` four, `databend-common-ast` three, `sqlglot-rust` two, while `turso_parser` and `orql` have a single published release and so show one point. The FFI parsers (`pg_query`) are excluded because two builds of libpg_query collide at link. Without `--full` the runners use a small per-dialect sample, which is a fast pipeline check rather than publishable numbers.

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

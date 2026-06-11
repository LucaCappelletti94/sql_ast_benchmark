# Changelog

## June 2026: parser refresh and a failed-to-parse badge

- Updated the benchmarked parsers to their latest versions: polyglot-sql 0.4.4 to 0.5.1, sqlglot-rust 0.10.0 to 0.10.1, and the git-tracked sqlparser-rs and pg_query.rs to their current commits (sqlparser-rs now at b3760221). turso_parser stays on 0.6.1 since the only newer version is a prerelease.
- The time machine gains the new release points (polyglot-sql 0.5.1 and sqlglot-rust 0.10.1) so the trends end at the current code.
- Each parser page gains a failed-to-parse badge in the meta-grid showing how many statements the parser rejected that it was expected to accept, summed across every dialect. It is a neutral coverage figure (every parser misses some real-world SQL), with the percentage and denominator in the tooltip.
- Added three CI-practice badges per parser: cargo deny (whether CI enforces a dependency policy with cargo deny), cargo audit (whether CI scans dependencies against the RustSec advisory database, via cargo audit or cargo deny check advisories), and cargo mutants (whether CI runs mutation testing). As of this snapshot turso is the only parser running cargo deny (its licenses check only), and none run cargo audit or cargo mutants.

## June 2026: robustness badges

- Each parser page gains a Robustness section with six per-parser badges mined from the parser's own source and behavior, so a chooser can weigh crash-safety alongside speed and coverage.
- Static panic discipline: a new `featurescan` crate parses each parser's library source with `syn` and counts panic-inducing constructs (panic!, unreachable!, unimplemented!, todo!, unwrap, expect, indexing), excluding tests, benches, and test-helper files, and reads the crate's own lint policy so a parser that bans those lints by design is shown as banned. The counts are a code-smell proxy, not a crash proof.
- Empirical panic rate: grading now tells a caught panic apart from an honest error, so each parser page reports how often it actually panics on the real corpus instead of returning an error. qusql-parse is the only parser that panics on real input (a fraction of a percent), and turso_parser's many static unreachable! macros never fire, which is exactly why the static and empirical signals are shown side by side.
- Recursion depth: a child-process probe measures how deeply each parser nests input before it either rejects with a clean recursion-limit error or overflows the stack and aborts the process. Among the pure-Rust parsers only sqlparser-rs (limit 48) and sqlite3-parser (no call recursion) are depth-guarded, while polyglot-sql overflows at depth 232.
- Unsafe surface (count plus whether the crate forbids unsafe), direct dependency count, and whether the AST derives serde round out the badges.
- The feature scan and depth probe run as part of `cargo regen`, and their committed JSON snapshots are baked into the site at build time, so the wasm build stays free of network access.

## June 2026: real engines, batch axis, and the time machine

- Validity is now graded against the real database engines (PostgreSQL, SQLite, MySQL, ClickHouse, DuckDB, SQL Server), run once locally in Docker via testcontainers by the `oracle` crate, with the labels committed under `oracle/labels` so grading and CI need no Docker. Library oracles are gone.
- Fixed the SQLite oracle mislabeling grammar errors as valid (it only recognized a few syntax-error phrasings, so rejections like "ORDER BY clause should come after INTERSECT not before" slipped through, reported in gwenn/lemon-rs#102). The classifier now treats any prepare error as invalid unless it is a missing-object error.
- Added turso_parser (the SQLite parser from Turso) as a tenth library, and per-statement rejection reasons on the failing-statement lists.
- The SQLite corpus now includes the SQLite project's own official test suite (29,344 statements, total corpus 340,938), which finally spreads the parsers on real SQLite grammar instead of leaving everyone near 100 percent.
- A batch (whole-script) axis times and measures memory for each parser's whole accepted set parsed as one script, normalized per statement and shown next to the single-statement means, with a completeness guard so a parser that bails out partway never reports a misleading number.
- A time machine benchmarks historical releases of every pure-Rust parser (59 versions across 8 families, including every sqlparser-rs minor since 0.30): each parser page gains a version picker plus date-axis trends for parse time and memory (median with interquartile bars) and for accept/recall and false positives. The FFI pg_query is excluded (two libpg_query builds collide at link), as is qusql-parse 0.1.0 (pathological parse time on parts of the corpus).
- The committed snapshots are now zstd-compressed and decompressed in the browser (`bench.json.zst` is about 26x smaller than the old raw JSON), keeping the site free of runtime fetches.
- One-command regeneration: `cargo regen` runs the timing benches, the memory benches, the time-machine passes, and the export in order.

## May 2026 refresh

- All benchmarked crates were updated to their latest versions (sqlparser 0.62, polyglot-sql 0.4.1, qusql-parse 0.8, databend-common-ast 0.2.5, sqlglot-rust 0.9.37, pg_query and orql to latest commits).
- Removed pg_parse and the pg_query_parser/pg_parse_parser Cargo features. pg_query.rs (libpg_query) is now an unconditional dependency and the sole PostgreSQL reference.
- Two parsers were added: sqlglot-rust (standalone 30-dialect parser) and sqlite3-parser / lemon-rs (SQLite's real Lemon grammar).
- The benchmark went from PostgreSQL-only to multi-dialect: every parser is now run in the dialect that matches the corpus it is being tested against.
- The corpus was expanded from a few thousand PostgreSQL statements to 311,594 statements over 13 dialects, now shipped pre-built and compressed as `datasets.tar.zst`.
- A data-quality pass removed mislabeled or non-SQL content: BiomedSQL (natural-language answers) and a metadata-contaminated Trino file were dropped, Stack Exchange Data Explorer queries were relabeled from SQLite to T-SQL, Oracle SQL\*Plus directives were stripped, and the SQL Server samples were dropped because their `GO` batch separators defeated statement segmentation.
- The five separate tools were consolidated into a single `sqlbench` binary (`correctness`, `correctness --per-file`, `plot`), and the grading core was extracted into testable library modules.
- The performance benchmark was rewritten around per-statement parse-time distributions (eCDF and box-plot views), with per-subplot legends showing each parser's rejection and Display round-trip rates. An earlier concatenated-body throughput metric was dropped as unsound on a real corpus (line comments and `COPY ... FROM STDIN` make the parser segment a joined body into far fewer statements than the input).

# Changelog

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

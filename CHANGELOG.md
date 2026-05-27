# Changelog

## May 2026 refresh

- All benchmarked crates were updated to their latest versions (sqlparser 0.62, polyglot-sql 0.4.1, qusql-parse 0.8, databend-common-ast 0.2.5, pg_parse 0.14, pg_query and orql to latest commits).
- Two parsers were added: **sqlglot-rust** (standalone 30-dialect parser) and **sqlite3-parser / lemon-rs** (SQLite's real Lemon grammar).
- The benchmark went from PostgreSQL-only to **multi-dialect**: every parser is now run in the dialect that matches the corpus it is being tested against.
- The corpus was expanded from a few thousand PostgreSQL statements to 311,594 statements over 13 dialects, now shipped pre-built and compressed as `datasets.tar.zst`.
- A data-quality pass removed mislabeled or non-SQL content: BiomedSQL (natural-language answers) and a metadata-contaminated Trino file were dropped, Stack Exchange Data Explorer queries were relabeled from SQLite to T-SQL, Oracle SQL\*Plus directives were stripped, and the SQL Server samples were dropped because their `GO` batch separators defeated statement segmentation.
- The five separate tools were consolidated into a single `sqlbench` binary (`correctness`, `correctness --per-file`, `plot`), and the grading core was extracted into testable library modules.
- The performance benchmark was rewritten around per-statement parse-time distributions (eCDF and box-plot views), with per-subplot legends showing each parser's rejection and Display round-trip rates. An earlier concatenated-body throughput metric was dropped as unsound on a real corpus (line comments and `COPY ... FROM STDIN` make the parser segment a joined body into far fewer statements than the input).

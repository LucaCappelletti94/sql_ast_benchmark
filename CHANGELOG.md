# Changelog

## May 2026 refresh

- All benchmarked crates were updated to their latest versions (sqlparser 0.62, polyglot-sql 0.4.1, qusql-parse 0.8, databend-common-ast 0.2.5, pg_parse 0.14, pg_query and orql to latest commits).
- Three parsers were added: **sqlglot-rust** (standalone 30-dialect parser), **sqlite3-parser / lemon-rs** (SQLite's real Lemon grammar), and **senax-mysql-parser** (MySQL CREATE TABLE only).
- The benchmark went from PostgreSQL-only to **multi-dialect**: every parser is now run in the dialect that matches the corpus it is being tested against.
- The corpus was expanded from a few thousand PostgreSQL statements to 311,594 statements over 13 dialects, now shipped pre-built and compressed as `datasets.tar.zst`.
- A data-quality pass removed mislabeled/non-SQL content: BiomedSQL (natural-language answers, not SQL) was dropped, the Stack Exchange Data Explorer queries were relabeled from SQLite to their real T-SQL dialect, a metadata-contaminated Trino testcase file was dropped, Oracle SQL\*Plus directive lines were stripped, and the SQL Server sample scripts were dropped because their `GO`-batch separators (not `;`) defeated statement segmentation.
- The five separate tools were consolidated into a single `sqlbench` binary (`correctness`, `correctness --per-file`, `plot`), and the grading core was extracted into testable library modules.
- The performance benchmark was rewritten around per-statement parse-time distributions (eCDF and box-plot views), with per-subplot legends reporting each parser's rejection rate and Display round-trip rate. An earlier concatenated-body throughput metric was dropped after it proved unsound on a real multi-dialect corpus (line comments and `COPY ... FROM STDIN` cause the parser to segment a joined body into far fewer statements than the input).

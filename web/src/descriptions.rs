//! Editorial blurbs shown on the dialect and parser detail pages.
//!
//! These are short, factual descriptions written for the site. They are kept as
//! plain data here (separate from layout) so the wording is easy to review and
//! revise. Keys match `DialectData::dir_name` and `BenchParser::name`. An empty
//! string means "no blurb" and the page simply omits the paragraph.
//!
//! Source links are woven into the prose with lightweight `[label](url)` markup
//! that the viewer renders as inline anchors (see `blurb` in `components`).

/// One-paragraph description of a SQL dialect, keyed by its `dir_name`.
#[must_use]
pub fn dialect_blurb(dir: &str) -> &'static str {
    match dir {
        "postgresql" => "[PostgreSQL](https://www.postgresql.org/) is a mature object-relational database known for standards compliance and extensibility. Its [SQL](https://www.postgresql.org/docs/current/sql.html) is among the richest in common use. It is one of the two reference dialects here, graded against [libpg_query](https://github.com/pganalyze/libpg_query), the parser from the PostgreSQL server itself.",
        "sqlite" => "[SQLite](https://www.sqlite.org/) is a small, serverless, single-file engine, the most widely deployed database in the world. Its [dialect](https://www.sqlite.org/lang.html) is compact and forgiving of loose syntax. It is the other reference dialect, graded against the [lemon](https://www.sqlite.org/lemon.html) grammar in [sqlite3-parser](https://crates.io/crates/sqlite3-parser).",
        "mysql" => "[MySQL](https://www.mysql.com/) is one of the most widely used open-source relational databases. Its [dialect](https://dev.mysql.com/doc/refman/en/sql-statements.html) uses backtick quoting and departs from the SQL standard in several notable ways.",
        "clickhouse" => "[ClickHouse](https://clickhouse.com/) is a column-oriented database built for real-time analytics over very large datasets. Its [SQL](https://clickhouse.com/docs/en/sql-reference) adds arrays, nested columns, and analytical functions.",
        "duckdb" => "[DuckDB](https://duckdb.org/) is an in-process analytical database, often called SQLite for analytics. Its [dialect](https://duckdb.org/docs/sql/introduction) is largely PostgreSQL-compatible with ergonomic shortcuts and list and struct types.",
        "hive" => "[Apache Hive](https://hive.apache.org/) is a data-warehouse layer over Hadoop with a SQL-like language, [HiveQL](https://cwiki.apache.org/confluence/display/Hive/LanguageManual). Its dialect reflects its distributed roots, with partitions, buckets, and complex types.",
        "spark_sql" => "[Spark SQL](https://spark.apache.org/sql/) is the structured-data module of Apache Spark, giving SQL access to distributed datasets. Its [dialect](https://spark.apache.org/docs/latest/sql-ref.html) follows Hive and the SQL standard closely with Spark-specific additions.",
        "trino" => "[Trino](https://trino.io/) (formerly PrestoSQL) is a distributed query engine for fast federated queries across many data sources. It targets ANSI [SQL](https://trino.io/docs/current/sql.html) with connector-oriented and analytic extensions.",
        "tsql" => "T-SQL is the dialect of Microsoft [SQL Server](https://www.microsoft.com/sql-server) and Azure SQL. It adds procedural constructs, square-bracket quoting, and its own [built-in functions](https://learn.microsoft.com/en-us/sql/t-sql/language-reference).",
        "oracle" => "[Oracle Database](https://www.oracle.com/database/) is an enterprise relational database with an extensive [SQL dialect](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/) and PL/SQL. It carries idioms such as the DUAL table and the (+) outer-join operator.",
        "bigquery" => "[BigQuery](https://cloud.google.com/bigquery) is Google Cloud's serverless data warehouse, queried with [GoogleSQL](https://cloud.google.com/bigquery/docs/reference/standard-sql/query-syntax). The dialect uses backtick quoting and first-class STRUCT and ARRAY types.",
        "redshift" => "[Amazon Redshift](https://aws.amazon.com/redshift/) is AWS's columnar, massively parallel data warehouse, derived from PostgreSQL 8.0. Its [dialect](https://docs.aws.amazon.com/redshift/latest/dg/cm_chap_SQLCommandRef.html) stays PostgreSQL-like but diverges in functions, types, and distribution and sort-key DDL.",
        "multi" => "Multi-dialect is not a single engine but a mixed corpus combining statements from many SQL dialects in one file. It stresses each parser on heterogeneous input, showing how broadly it generalizes. Acceptance is graded by provenance, since no single reference applies.",
        _ => "",
    }
}

/// One-paragraph description of a parser, keyed by its display name.
#[must_use]
pub fn parser_blurb(name: &str) -> &'static str {
    match name {
        "sqlparser-rs" => "[sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) (crate [sqlparser](https://crates.io/crates/sqlparser)) is a pure-Rust, hand-written parser maintained under [Apache DataFusion](https://github.com/apache/datafusion). It models many dialects via a pluggable Dialect trait and prints its AST back to SQL, so it is graded for round-trip and fidelity. It is the most widely used SQL parser in Rust.",
        "pg_query.rs" => "[pg_query.rs](https://github.com/pganalyze/pg_query.rs) (crate [pg_query](https://crates.io/crates/pg_query)) wraps [libpg_query](https://github.com/pganalyze/libpg_query), the PostgreSQL server's own C parser, through Rust FFI. As the very code PostgreSQL runs, it is the reference for the PostgreSQL dialect, and it can deparse back to SQL. It models only PostgreSQL.",
        "pg_query (summary)" => "pg_query (summary) runs the same [libpg_query](https://github.com/pganalyze/libpg_query) parse as [pg_query.rs](https://github.com/pganalyze/pg_query.rs) (crate [pg_query](https://crates.io/crates/pg_query)) but returns a compact C-side summary instead of the full parse tree, which is much faster. It shows the parse-only throughput of the reference parser. It models only PostgreSQL.",
        "qusql-parse" => "[qusql-parse](https://crates.io/crates/qusql-parse) is a pure-Rust, zero-copy parser with dialect-aware options and ranked diagnostics rather than first-error failure. It runs here in PostgreSQL, MariaDB, and SQLite modes. Its coverage of complex SELECT statements is partial.",
        "polyglot-sql" => "[polyglot-sql](https://github.com/tobilg/polyglot) (crate [polyglot-sql](https://crates.io/crates/polyglot-sql)) is a young pure-Rust parser and transpiler covering many dialects from one grammar. It regenerates its AST as SQL, so it is graded for round-trip and fidelity. Its per-call setup cost amortizes over large batches.",
        "databend-common-ast" => "[databend-common-ast](https://crates.io/crates/databend-common-ast) is the SQL front end of [Databend](https://github.com/datafuselabs/databend), a Rust cloud data warehouse. It pairs a zero-copy tokenizer with a custom Pratt parser, offers PostgreSQL, MySQL, and Hive modes, and prints its AST back to SQL.",
        "orql" => "[orql](https://codeberg.org/xitep/orql) is a pure-Rust Oracle SQL parser focused on SELECT statements, added at its author's request. It models only the Oracle dialect, so it appears on the Oracle results alone. It does not pretty-print, so round-trip and fidelity are n/a.",
        "sqlglot-rust" => "[sqlglot-rust](https://crates.io/crates/sqlglot-rust) is a Rust parser and transpiler in the spirit of Python's [SQLGlot](https://github.com/tobymao/sqlglot), covering many dialects. It regenerates its AST as SQL, so it is graded for round-trip and fidelity across the dialects it models.",
        "sqlite3-parser" => "[sqlite3-parser](https://crates.io/crates/sqlite3-parser) (also known as [lemon-rs](https://github.com/gwenn/lemon-rs)) is a pure-Rust streaming lexer and LALR parser reimplementing SQLite's grammar. It models only SQLite and provides the SQLite reference. It can reprint statements, so it is graded for round-trip on SQLite.",
        "turso_parser" => "[turso_parser](https://crates.io/crates/turso_parser) is the SQL front end of [Turso](https://github.com/tursodatabase/turso), a from-scratch Rust rewrite of SQLite (formerly Limbo). It pairs a lemon-generated token table with a hand-written recursive-descent parser for SQLite's grammar, so unlike sqlite3-parser's LALR tables the parsing is hand-rolled. It models only SQLite and can reprint statements, so it is graded for round-trip on SQLite.",
        _ => "",
    }
}

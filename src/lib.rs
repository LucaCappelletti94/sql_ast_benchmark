use databend_common_ast::parser::{
    parse_sql as databend_parse, tokenize_sql as databend_tokenize, Dialect as DatabendDialect,
};
use orql::parser as orql_parser;
use polyglot_sql::{parse as polyglot_parse, DialectType, Generator as PolyglotGenerator};
use qusql_parse::{parse_statements, Issues, Level, ParseOptions, SQLDialect};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use crate::datasets::Dialect;
use fallible_iterator::FallibleIterator as _;
use sqlglot_rust::Dialect as SqlglotDialect;
use sqlparser::dialect::{
    BigQueryDialect, ClickHouseDialect, DatabricksDialect, Dialect as SqlparserDialect,
    DuckDbDialect, GenericDialect, HiveDialect, MsSqlDialect, MySqlDialect, OracleDialect,
    RedshiftSqlDialect, SQLiteDialect,
};

fn pg_query_canonical(sql: &str) -> Option<String> {
    pg_query::parse(sql).ok()?.deparse().ok()
}

// Multi-dialect benchmark layer. Each parser runs in its best-matching dialect.
// One it does not model returns `None` (N/A). Correctness uses reference where
// one exists (pg_query for PostgreSQL, lemon-rs for SQLite), else acceptance rate.

// Dialect mappings.

/// Best-matching sqlparser-rs dialect for a corpus dialect (always available).
/// Trino has no dedicated dialect (uses Generic), and Spark SQL maps to Databricks.
fn sqlparser_dialect(d: Dialect) -> Box<dyn SqlparserDialect> {
    match d {
        Dialect::Postgresql => Box::new(PostgreSqlDialect {}),
        Dialect::Mysql => Box::new(MySqlDialect {}),
        Dialect::Sqlite => Box::new(SQLiteDialect {}),
        Dialect::Clickhouse => Box::new(ClickHouseDialect {}),
        Dialect::Hive => Box::new(HiveDialect {}),
        Dialect::Duckdb => Box::new(DuckDbDialect {}),
        Dialect::SparkSql => Box::new(DatabricksDialect {}),
        Dialect::Tsql => Box::new(MsSqlDialect {}),
        Dialect::Oracle => Box::new(OracleDialect {}),
        Dialect::Bigquery => Box::new(BigQueryDialect {}),
        Dialect::Redshift => Box::new(RedshiftSqlDialect {}),
        Dialect::Trino | Dialect::Multi => Box::new(GenericDialect {}),
    }
}

/// polyglot-sql dialect (covers every corpus dialect).
const fn polyglot_dialect(d: Dialect) -> DialectType {
    match d {
        Dialect::Postgresql => DialectType::PostgreSQL,
        Dialect::Mysql => DialectType::MySQL,
        Dialect::Sqlite => DialectType::SQLite,
        Dialect::Clickhouse => DialectType::ClickHouse,
        Dialect::Hive => DialectType::Hive,
        Dialect::Trino => DialectType::Trino,
        Dialect::Duckdb => DialectType::DuckDB,
        Dialect::SparkSql => DialectType::Spark,
        Dialect::Tsql => DialectType::TSQL,
        Dialect::Oracle => DialectType::Oracle,
        Dialect::Bigquery => DialectType::BigQuery,
        Dialect::Redshift => DialectType::Redshift,
        Dialect::Multi => DialectType::Generic,
    }
}

/// sqlglot-rust dialect. Note: in this version sqlglot-rust's parser is
/// dialect-agnostic (the argument only affects generation), so acceptance does
/// not vary by dialect for this parser.
const fn sqlglot_dialect(d: Dialect) -> SqlglotDialect {
    match d {
        Dialect::Postgresql => SqlglotDialect::Postgres,
        Dialect::Mysql => SqlglotDialect::Mysql,
        Dialect::Sqlite => SqlglotDialect::Sqlite,
        Dialect::Clickhouse => SqlglotDialect::ClickHouse,
        Dialect::Hive => SqlglotDialect::Hive,
        Dialect::Trino => SqlglotDialect::Trino,
        Dialect::Duckdb => SqlglotDialect::DuckDb,
        Dialect::SparkSql => SqlglotDialect::Spark,
        Dialect::Tsql => SqlglotDialect::Tsql,
        Dialect::Oracle => SqlglotDialect::Oracle,
        Dialect::Bigquery => SqlglotDialect::BigQuery,
        Dialect::Redshift => SqlglotDialect::Redshift,
        Dialect::Multi => SqlglotDialect::Ansi,
    }
}

/// qusql-parse dialect, or `None` for dialects it does not model.
const fn qusql_dialect(d: Dialect) -> Option<SQLDialect> {
    match d {
        Dialect::Postgresql => Some(SQLDialect::PostgreSQL),
        Dialect::Mysql => Some(SQLDialect::MariaDB),
        Dialect::Sqlite => Some(SQLDialect::Sqlite),
        _ => None,
    }
}

/// databend dialect, or `None` for dialects it does not model.
const fn databend_dialect_of(d: Dialect) -> Option<DatabendDialect> {
    match d {
        Dialect::Postgresql => Some(DatabendDialect::PostgreSQL),
        Dialect::Mysql => Some(DatabendDialect::MySQL),
        Dialect::Hive => Some(DatabendDialect::Hive),
        _ => None,
    }
}

// Per-parser primitives (acceptance + reprint).

/// Extract a human-readable message from a caught panic payload.
fn panic_reason(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panicked".to_string())
}

/// Run a parse closure under panic protection, turning a panic into an error
/// message. Several parsers use `todo!()`/`panic!` on unimplemented paths, so
/// this keeps a worker alive and still yields a reason for the rejection.
fn catch_parse(
    f: impl FnOnce() -> Result<(), String> + std::panic::UnwindSafe,
) -> Result<(), String> {
    match std::panic::catch_unwind(f) {
        Ok(r) => r,
        Err(p) => Err(panic_reason(&*p)),
    }
}

fn qusql_try(sql: &str, d: SQLDialect) -> Result<(), String> {
    catch_parse(|| {
        let opts = ParseOptions::new()
            .dialect(d)
            .arguments(qusql_parse::SQLArguments::Dollar);
        let mut issues = Issues::new(sql);
        let _ = parse_statements(sql, &mut issues, &opts);
        issues
            .get()
            .iter()
            .find(|i| i.level == Level::Error)
            .map_or(Ok(()), |e| Err(e.message.to_string()))
    })
}

fn databend_try(sql: &str, d: DatabendDialect) -> Result<(), String> {
    catch_parse(|| {
        let tokens = databend_tokenize(sql).map_err(|e| e.to_string())?;
        databend_parse(&tokens, d)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

fn sqlite3_try(sql: &str) -> Result<(), String> {
    catch_parse(|| {
        let mut parser = sqlite3_parser::lexer::sql::Parser::new(sql.as_bytes());
        loop {
            match parser.next() {
                Ok(Some(_)) => {}
                Ok(None) => return Ok(()),
                Err(e) => return Err(e.to_string()),
            }
        }
    })
}

fn sqlite3_reprint(sql: &str) -> Option<String> {
    std::panic::catch_unwind(|| {
        let mut parser = sqlite3_parser::lexer::sql::Parser::new(sql.as_bytes());
        let mut out: Vec<String> = Vec::new();
        loop {
            match parser.next() {
                Ok(Some(cmd)) => out.push(cmd.to_string()),
                Ok(None) => break,
                Err(_) => return None,
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out.join("; "))
        }
    })
    .unwrap_or(None)
}

fn turso_try(sql: &str) -> Result<(), String> {
    catch_parse(|| {
        let mut parser = turso_parser::parser::Parser::new(sql.as_bytes());
        loop {
            match parser.next_cmd() {
                Ok(Some(_)) => {}
                Ok(None) => return Ok(()),
                Err(e) => return Err(e.to_string()),
            }
        }
    })
}

fn turso_reprint(sql: &str) -> Option<String> {
    std::panic::catch_unwind(|| {
        let mut parser = turso_parser::parser::Parser::new(sql.as_bytes());
        let mut out: Vec<String> = Vec::new();
        loop {
            match parser.next_cmd() {
                Ok(Some(cmd)) => out.push(cmd.to_string()),
                Ok(None) => break,
                Err(_) => return None,
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out.join("; "))
        }
    })
    .unwrap_or(None)
}

fn sqlglot_reprint(sql: &str, d: Dialect) -> Option<String> {
    std::panic::catch_unwind(|| {
        let stmts = sqlglot_rust::parser::parse_statements(sql, sqlglot_dialect(d)).ok()?;
        if stmts.is_empty() {
            return None;
        }
        Some(
            stmts
                .iter()
                .map(|s| sqlglot_rust::generate(s, sqlglot_dialect(d)))
                .collect::<Vec<_>>()
                .join("; "),
        )
    })
    .unwrap_or(None)
}

fn sqlparser_reprint(sql: &str, d: Dialect) -> Option<String> {
    let dialect = sqlparser_dialect(d);
    let stmts = Parser::parse_sql(&*dialect, sql).ok()?;
    if stmts.is_empty() {
        return None;
    }
    Some(
        stmts
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "),
    )
}

fn polyglot_reprint(sql: &str, d: Dialect) -> Option<String> {
    std::panic::catch_unwind(|| {
        let exprs = polyglot_parse(sql, polyglot_dialect(d)).ok()?;
        if exprs.is_empty() {
            return None;
        }
        PolyglotGenerator::new().generate(&exprs[0]).ok()
    })
    .unwrap_or(None)
}

fn databend_reprint(sql: &str, d: DatabendDialect) -> Option<String> {
    std::panic::catch_unwind(|| {
        let tokens = databend_tokenize(sql).ok()?;
        let (stmt, _) = databend_parse(&tokens, d).ok()?;
        Some(stmt.to_string())
    })
    .unwrap_or(None)
}

// Reference.

/// Canonical form for a reference-backed dialect, used for fidelity checks.
/// `None` for dialects with no reference (or when the relevant feature is off).
fn reference_canonical(sql: &str, d: Dialect) -> Option<String> {
    match d {
        Dialect::Postgresql => pg_query_canonical(sql),
        Dialect::Sqlite => sqlite3_reprint(sql),
        _ => None,
    }
}

/// Does the reference accept this statement?
///
/// The reference is the real database engine, validated offline and read from
/// the committed cache (see [`crate::oracle_cache`]). `Some(true/false)` on a
/// cache hit, `None` if the dialect has no reference engine or the statement is
/// not in the cache.
#[must_use]
pub fn reference_accepts(sql: &str, d: Dialect) -> Option<bool> {
    oracle_cache::reference_accepts(sql, d)
}

/// Is `d` a reference-backed dialect (a real engine cache exists, so recall and
/// false-positive are graded)?
#[must_use]
pub fn has_reference(d: Dialect) -> bool {
    oracle_cache::has_reference(d)
}

/// Dialects with a library canonicalizer for the fidelity metric.
///
/// `PostgreSQL` via `pg_query`, `SQLite` via `lemon-rs`. This is independent of
/// the validity reference (the real engine), which only labels statements
/// valid/invalid.
#[must_use]
pub const fn has_canonical(d: Dialect) -> bool {
    matches!(d, Dialect::Postgresql | Dialect::Sqlite)
}

// BenchParser.

/// A parser under test. The single source of truth for dialect support,
/// acceptance, round-trip stability and reference fidelity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchParser {
    Sqlparser,
    PgQuery,
    PgQuerySummary,
    Qusql,
    Polyglot,
    Databend,
    Orql,
    Sqlglot,
    Sqlite3,
    Turso,
}

impl BenchParser {
    /// All parsers compiled into the current build.
    #[must_use]
    pub fn all() -> Vec<Self> {
        vec![
            Self::Sqlparser,
            Self::PgQuery,
            Self::PgQuerySummary,
            Self::Qusql,
            Self::Polyglot,
            Self::Databend,
            Self::Orql,
            Self::Sqlglot,
            Self::Sqlite3,
            Self::Turso,
        ]
    }

    /// Human-readable name for reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sqlparser => "sqlparser-rs",
            Self::PgQuery => "pg_query.rs",
            Self::PgQuerySummary => "pg_query (summary)",
            Self::Qusql => "qusql-parse",
            Self::Polyglot => "polyglot-sql",
            Self::Databend => "databend-common-ast",
            Self::Orql => "orql",
            Self::Sqlglot => "sqlglot-rust",
            Self::Sqlite3 => "sqlite3-parser",
            Self::Turso => "turso_parser",
        }
    }

    /// Does this parser model `dialect`? (acceptance returns `Some` iff true)
    #[must_use]
    pub fn supports(self, dialect: Dialect) -> bool {
        self.accepts("", dialect).is_some() || self.accepts("SELECT 1", dialect).is_some()
    }

    /// `Some(true)` accepted, `Some(false)` rejected, `None` dialect unsupported.
    #[must_use]
    pub fn accepts(self, sql: &str, dialect: Dialect) -> Option<bool> {
        self.try_parse(sql, dialect).map(|r| r.is_ok())
    }

    /// Parse `sql` in `dialect`, capturing the rejection reason. `None` = the
    /// parser does not model the dialect, `Some(Ok(()))` = accepted, and
    /// `Some(Err(msg))` = rejected with the parser's own error message (or a
    /// panic message for parsers that abort on edge-case input). Panic-protected
    /// like [`Self::accepts`], so it never unwinds the caller.
    #[must_use]
    pub fn try_parse(self, sql: &str, dialect: Dialect) -> Option<Result<(), String>> {
        match self {
            Self::Sqlparser => Some(
                Parser::parse_sql(&*sqlparser_dialect(dialect), sql)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            Self::PgQuery => (dialect == Dialect::Postgresql)
                .then(|| pg_query::parse(sql).map(|_| ()).map_err(|e| e.to_string())),
            Self::PgQuerySummary => (dialect == Dialect::Postgresql).then(|| {
                pg_query::summary(sql, -1)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }),
            Self::Qusql => qusql_dialect(dialect).map(|d| qusql_try(sql, d)),
            Self::Polyglot => Some(catch_parse(|| {
                polyglot_parse(sql, polyglot_dialect(dialect))
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })),
            Self::Databend => databend_dialect_of(dialect).map(|d| databend_try(sql, d)),
            Self::Orql => (dialect == Dialect::Oracle).then(|| {
                catch_parse(|| {
                    orql_parser::parse(sql)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
            }),
            Self::Sqlglot => Some(catch_parse(|| {
                sqlglot_rust::parser::parse_statements(sql, sqlglot_dialect(dialect))
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })),
            Self::Sqlite3 => (dialect == Dialect::Sqlite).then(|| sqlite3_try(sql)),
            Self::Turso => (dialect == Dialect::Sqlite).then(|| turso_try(sql)),
        }
    }

    /// Parse `sql` once in `dialect` for timing, WITHOUT panic protection.
    /// Returns whether the parse succeeded. Intended only for statements already
    /// in the accepted set (re-parsing cannot panic). On rejected or edge-case
    /// input it may abort the process. Avoiding `catch_unwind` keeps the timing
    /// free of landing-pad overhead and fair across parsers.
    #[must_use]
    pub fn parse_once(self, sql: &str, dialect: Dialect) -> bool {
        match self {
            Self::Sqlparser => Parser::parse_sql(&*sqlparser_dialect(dialect), sql).is_ok(),
            Self::PgQuery => pg_query::parse(sql).is_ok(),
            Self::PgQuerySummary => pg_query::summary(sql, -1).is_ok(),
            Self::Qusql => {
                let d = qusql_dialect(dialect).unwrap_or(SQLDialect::PostgreSQL);
                let opts = ParseOptions::new()
                    .dialect(d)
                    .arguments(qusql_parse::SQLArguments::Dollar);
                let mut issues = Issues::new(sql);
                let _ = parse_statements(sql, &mut issues, &opts);
                !issues.get().iter().any(|i| i.level == Level::Error)
            }
            Self::Polyglot => polyglot_parse(sql, polyglot_dialect(dialect)).is_ok(),
            Self::Databend => {
                let d = databend_dialect_of(dialect).unwrap_or(DatabendDialect::PostgreSQL);
                databend_tokenize(sql)
                    .ok()
                    .and_then(|t| databend_parse(&t, d).ok())
                    .is_some()
            }
            Self::Orql => orql_parser::parse(sql).is_ok(),
            Self::Sqlglot => {
                sqlglot_rust::parser::parse_statements(sql, sqlglot_dialect(dialect)).is_ok()
            }
            Self::Sqlite3 => {
                let mut parser = sqlite3_parser::lexer::sql::Parser::new(sql.as_bytes());
                loop {
                    match parser.next() {
                        Ok(Some(_)) => {}
                        Ok(None) => break true,
                        Err(_) => break false,
                    }
                }
            }
            Self::Turso => {
                let mut parser = turso_parser::parser::Parser::new(sql.as_bytes());
                loop {
                    match parser.next_cmd() {
                        Ok(Some(_)) => {}
                        Ok(None) => break true,
                        Err(_) => break false,
                    }
                }
            }
        }
    }

    /// Whether this parser exposes a multi-statement (batch) parse entry point,
    /// so it can consume a whole script in one call. Only `databend-common-ast`
    /// parses a single statement at a time, so it is excluded from the batch
    /// benchmark (reported n/a there).
    #[must_use]
    pub const fn can_batch(self) -> bool {
        !matches!(self, Self::Databend)
    }

    /// Parse a whole multi-statement script `sql` in `dialect` for batch timing,
    /// WITHOUT panic protection (like [`Self::parse_once`]). Returns the number
    /// of statements the parser reports parsing, or `None` if the parser does
    /// not model the dialect or has no batch entry point ([`Self::can_batch`]).
    ///
    /// Fail-fast parsers (those returning a `Vec` or erroring on the first bad
    /// statement) yield `0` if the whole batch fails; streaming parsers
    /// (`sqlite3-parser`, `turso_parser`) yield the count parsed before the
    /// first error or EOF. Batches are built from already-accepted statements,
    /// so a clean run parses all of them; the count is kept for coverage.
    #[must_use]
    pub fn parse_batch(self, sql: &str, dialect: Dialect) -> Option<usize> {
        match self {
            Self::Sqlparser => {
                Some(Parser::parse_sql(&*sqlparser_dialect(dialect), sql).map_or(0, |v| v.len()))
            }
            Self::PgQuery => (dialect == Dialect::Postgresql)
                .then(|| pg_query::parse(sql).map_or(0, |r| r.protobuf.stmts.len())),
            Self::PgQuerySummary => (dialect == Dialect::Postgresql)
                .then(|| pg_query::summary(sql, -1).map_or(0, |r| r.statement_types.len())),
            Self::Qusql => qusql_dialect(dialect).map(|d| {
                let opts = ParseOptions::new()
                    .dialect(d)
                    .arguments(qusql_parse::SQLArguments::Dollar);
                let mut issues = Issues::new(sql);
                let stmts = parse_statements(sql, &mut issues, &opts);
                // Resilient parser: report a full count only when error-free.
                if issues.get().iter().any(|i| i.level == Level::Error) {
                    0
                } else {
                    stmts.len()
                }
            }),
            Self::Polyglot => {
                Some(polyglot_parse(sql, polyglot_dialect(dialect)).map_or(0, |v| v.len()))
            }
            // Single-statement parser: no batch entry point.
            Self::Databend => None,
            Self::Orql => {
                (dialect == Dialect::Oracle).then(|| orql_parser::parse(sql).map_or(0, |v| v.len()))
            }
            Self::Sqlglot => Some(
                sqlglot_rust::parser::parse_statements(sql, sqlglot_dialect(dialect))
                    .map_or(0, |v| v.len()),
            ),
            Self::Sqlite3 => (dialect == Dialect::Sqlite).then(|| {
                let mut parser = sqlite3_parser::lexer::sql::Parser::new(sql.as_bytes());
                let mut n = 0;
                loop {
                    match parser.next() {
                        Ok(Some(_)) => n += 1,
                        Ok(None) | Err(_) => break n,
                    }
                }
            }),
            Self::Turso => (dialect == Dialect::Sqlite).then(|| {
                let mut parser = turso_parser::parser::Parser::new(sql.as_bytes());
                let mut n = 0;
                loop {
                    match parser.next_cmd() {
                        Ok(Some(_)) => n += 1,
                        Ok(None) | Err(_) => break n,
                    }
                }
            }),
        }
    }

    /// Parse `sql` while the `membench` allocator is active and return
    /// `(peak, retained)` bytes: the high-water mark of live allocations during
    /// the parse, and the bytes still live afterwards (the produced AST plus any
    /// scaffolding it keeps alive). `None` for the `libpg_query` bindings, whose
    /// real work happens in C and is invisible to the Rust allocator, and for
    /// dialects a parser does not model. Intended for accepted statements, called
    /// single-threaded from the `membench` binary.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn measure_mem(self, sql: &str, dialect: Dialect) -> Option<(usize, usize)> {
        use std::hint::black_box;
        // (peak, retained) relative to the live total just before the parse.
        let snap = |before: usize| {
            (
                mem::peak().saturating_sub(before),
                mem::live().saturating_sub(before),
            )
        };
        match self {
            // libpg_query allocates in C, invisible to the Rust allocator.
            Self::PgQuery | Self::PgQuerySummary => None,
            Self::Sqlparser => {
                let before = mem::live();
                mem::reset_peak();
                let ast = Parser::parse_sql(&*sqlparser_dialect(dialect), sql);
                black_box(&ast);
                let r = snap(before);
                drop(ast);
                Some(r)
            }
            Self::Qusql => {
                let d = qusql_dialect(dialect)?;
                let before = mem::live();
                mem::reset_peak();
                let opts = ParseOptions::new()
                    .dialect(d)
                    .arguments(qusql_parse::SQLArguments::Dollar);
                let mut issues = Issues::new(sql);
                let ast = parse_statements(sql, &mut issues, &opts);
                black_box((&ast, &issues));
                let r = snap(before);
                drop(ast);
                drop(issues);
                Some(r)
            }
            Self::Polyglot => {
                let before = mem::live();
                mem::reset_peak();
                let ast = polyglot_parse(sql, polyglot_dialect(dialect));
                black_box(&ast);
                let r = snap(before);
                drop(ast);
                Some(r)
            }
            Self::Databend => {
                let d = databend_dialect_of(dialect)?;
                let before = mem::live();
                mem::reset_peak();
                let toks = databend_tokenize(sql);
                let ast = toks.as_ref().ok().map(|t| databend_parse(t, d));
                black_box((&toks, &ast));
                let r = snap(before);
                drop(ast);
                drop(toks);
                Some(r)
            }
            Self::Orql => {
                let before = mem::live();
                mem::reset_peak();
                let ast = orql_parser::parse(sql);
                black_box(&ast);
                let r = snap(before);
                drop(ast);
                Some(r)
            }
            Self::Sqlglot => {
                let before = mem::live();
                mem::reset_peak();
                let ast = sqlglot_rust::parser::parse_statements(sql, sqlglot_dialect(dialect));
                black_box(&ast);
                let r = snap(before);
                drop(ast);
                Some(r)
            }
            Self::Sqlite3 => {
                if dialect != Dialect::Sqlite {
                    return None;
                }
                let before = mem::live();
                mem::reset_peak();
                let mut parser = sqlite3_parser::lexer::sql::Parser::new(sql.as_bytes());
                let mut out = Vec::new();
                while let Ok(Some(cmd)) = parser.next() {
                    out.push(cmd);
                }
                black_box((&parser, &out));
                let r = snap(before);
                drop(out);
                drop(parser);
                Some(r)
            }
            Self::Turso => {
                if dialect != Dialect::Sqlite {
                    return None;
                }
                let before = mem::live();
                mem::reset_peak();
                let mut parser = turso_parser::parser::Parser::new(sql.as_bytes());
                let mut out = Vec::new();
                while let Ok(Some(cmd)) = parser.next_cmd() {
                    out.push(cmd);
                }
                black_box((&parser, &out));
                let r = snap(before);
                drop(out);
                drop(parser);
                Some(r)
            }
        }
    }

    /// Like [`Self::measure_mem`], but for a whole multi-statement script: it
    /// holds every statement's AST live at once, so the `(peak, retained)` it
    /// reports reflects batch parsing (a grown `Vec` of statements, all ASTs
    /// retained together). `None` when the parser has no batch entry point
    /// ([`Self::can_batch`], so databend), when its memory is invisible to the
    /// Rust allocator (the `libpg_query` bindings), or when it does not model
    /// `dialect`. Called single-threaded from the `membench` binary; under any
    /// other binary the counters are zero and it returns `Some((0, 0))`.
    #[must_use]
    pub fn measure_mem_batch(self, sql: &str, dialect: Dialect) -> Option<(usize, usize)> {
        if !self.can_batch() {
            return None;
        }
        self.measure_mem(sql, dialect)
    }

    /// Parse and pretty-print, returning `None` if the parser has no printer, does not
    /// model `dialect`, or fails to parse `sql`.
    #[must_use]
    pub fn reprint(self, sql: &str, dialect: Dialect) -> Option<String> {
        match self {
            Self::Sqlparser => sqlparser_reprint(sql, dialect),
            Self::Polyglot => polyglot_reprint(sql, dialect),
            Self::Databend => databend_dialect_of(dialect).and_then(|d| databend_reprint(sql, d)),
            Self::Sqlglot => sqlglot_reprint(sql, dialect),
            Self::Sqlite3 => (dialect == Dialect::Sqlite)
                .then(|| sqlite3_reprint(sql))
                .flatten(),
            Self::Turso => (dialect == Dialect::Sqlite)
                .then(|| turso_reprint(sql))
                .flatten(),
            Self::PgQuery => (dialect == Dialect::Postgresql)
                .then(|| pg_query::parse(sql).ok().and_then(|p| p.deparse().ok()))
                .flatten(),
            _ => None,
        }
    }

    /// Whether this parser has a pretty-printer (can round-trip / be graded for
    /// fidelity) for `dialect`.
    #[must_use]
    pub fn can_reprint(self, dialect: Dialect) -> bool {
        match self {
            Self::Sqlparser | Self::Polyglot | Self::Sqlglot => true,
            Self::Databend => databend_dialect_of(dialect).is_some(),
            Self::Sqlite3 | Self::Turso => dialect == Dialect::Sqlite,
            Self::PgQuery => dialect == Dialect::Postgresql,
            _ => false,
        }
    }

    /// Round-trip stability: reprint(sql) == reprint(reprint(sql)).
    /// `None` if the parser cannot reprint in this dialect.
    #[must_use]
    pub fn roundtrips(self, sql: &str, dialect: Dialect) -> Option<bool> {
        if !self.can_reprint(dialect) {
            return None;
        }
        let Some(first) = self.reprint(sql, dialect) else {
            return Some(false);
        };
        Some(
            self.reprint(&first, dialect)
                .is_some_and(|second| first == second),
        )
    }

    /// Reference fidelity: the reference's canonical form of this parser's output
    /// equals the reference's canonical form of the input. `None` if the parser
    /// cannot reprint or the dialect has no reference.
    #[must_use]
    pub fn fidelity(self, sql: &str, dialect: Dialect) -> Option<bool> {
        if !self.can_reprint(dialect) || !has_canonical(dialect) {
            return None;
        }
        let Some(out) = self.reprint(sql, dialect) else {
            return Some(false);
        };
        match (
            reference_canonical(sql, dialect),
            reference_canonical(&out, dialect),
        ) {
            (Some(a), Some(b)) => Some(a == b),
            _ => Some(false),
        }
    }
}

pub mod batch;
pub mod bench_dist;
pub mod datasets;
pub mod export;
pub mod mem;
pub mod oracle_cache;
pub mod report;
pub mod stats;

#[cfg(test)]
mod tests {
    use super::{has_canonical, has_reference, reference_accepts, BenchParser};
    use crate::datasets::Dialect;

    const ALL_DIALECTS: [Dialect; 13] = [
        Dialect::Postgresql,
        Dialect::Mysql,
        Dialect::Sqlite,
        Dialect::Clickhouse,
        Dialect::Hive,
        Dialect::Trino,
        Dialect::Duckdb,
        Dialect::SparkSql,
        Dialect::Tsql,
        Dialect::Oracle,
        Dialect::Bigquery,
        Dialect::Redshift,
        Dialect::Multi,
    ];

    /// Dialects a parser models, in `ALL_DIALECTS` order.
    fn supported(p: BenchParser) -> Vec<Dialect> {
        ALL_DIALECTS
            .into_iter()
            .filter(|&d| p.supports(d))
            .collect()
    }

    #[test]
    fn accepts_returns_some_exactly_for_supported_dialects() {
        for p in BenchParser::all() {
            for d in ALL_DIALECTS {
                assert_eq!(
                    p.accepts("SELECT 1", d).is_some(),
                    p.supports(d),
                    "{}: accepts/supports disagree on {d:?}",
                    p.name()
                );
            }
        }
    }

    #[test]
    fn multi_dialect_parsers_support_everything() {
        for p in [
            BenchParser::Sqlparser,
            BenchParser::Polyglot,
            BenchParser::Sqlglot,
        ] {
            assert_eq!(
                supported(p).len(),
                ALL_DIALECTS.len(),
                "{} should model every dialect",
                p.name()
            );
        }
    }

    #[test]
    fn dialect_specific_parsers_model_expected_sets() {
        assert_eq!(
            supported(BenchParser::Qusql),
            vec![Dialect::Postgresql, Dialect::Mysql, Dialect::Sqlite]
        );
        assert_eq!(
            supported(BenchParser::Databend),
            vec![Dialect::Postgresql, Dialect::Mysql, Dialect::Hive]
        );
        assert_eq!(supported(BenchParser::Sqlite3), vec![Dialect::Sqlite]);
        assert_eq!(supported(BenchParser::Turso), vec![Dialect::Sqlite]);
        assert_eq!(supported(BenchParser::Orql), vec![Dialect::Oracle]);
    }

    #[test]
    fn pg_query_models_postgresql_only() {
        assert_eq!(supported(BenchParser::PgQuery), vec![Dialect::Postgresql]);
        assert_eq!(
            supported(BenchParser::PgQuerySummary),
            vec![Dialect::Postgresql]
        );
    }

    #[test]
    fn parser_names_are_unique() {
        let mut names: Vec<&str> = BenchParser::all().iter().map(|p| p.name()).collect();
        let len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), len, "duplicate parser name");
    }

    #[test]
    fn sqlparser_accepts_valid_and_rejects_garbage() {
        assert_eq!(
            BenchParser::Sqlparser.accepts("SELECT 1", Dialect::Postgresql),
            Some(true)
        );
        assert_eq!(
            BenchParser::Sqlparser.accepts("SELECT 1 FROM", Dialect::Postgresql),
            Some(false)
        );
    }

    #[test]
    fn try_parse_yields_a_reason_on_rejection() {
        // Accepted: Ok with no message. Rejected: Err with a non-empty reason.
        assert_eq!(
            BenchParser::Sqlparser.try_parse("SELECT 1", Dialect::Postgresql),
            Some(Ok(()))
        );
        let rejected = BenchParser::Sqlparser
            .try_parse("SELECT 1 FROM", Dialect::Postgresql)
            .expect("postgresql is modelled");
        let reason = rejected.expect_err("garbage should be rejected");
        assert!(!reason.is_empty(), "rejection reason should not be empty");
        // Unsupported dialect/parser pairing stays None.
        assert_eq!(
            BenchParser::Sqlite3.try_parse("SELECT 1", Dialect::Postgresql),
            None
        );
    }

    #[test]
    fn parse_once_agrees_with_accepts_on_accepted_statements() {
        let sql = "SELECT 1";
        for p in BenchParser::all() {
            if p.accepts(sql, Dialect::Postgresql) == Some(true) {
                assert!(
                    p.parse_once(sql, Dialect::Postgresql),
                    "{}: parse_once should succeed on an accepted statement",
                    p.name()
                );
            }
        }
    }

    #[test]
    fn reference_excludes_dialects_without_a_real_engine() {
        // Cloud, heavy-JVM, and Oracle dialects never get a real-engine cache, so
        // they are never reference-graded and the reference verdict is None,
        // independent of which label caches happen to be present.
        for d in [
            Dialect::Bigquery,
            Dialect::Redshift,
            Dialect::Trino,
            Dialect::Hive,
            Dialect::SparkSql,
            Dialect::Oracle,
            Dialect::Multi,
        ] {
            assert!(!has_reference(d), "{d:?} should never be reference-graded");
            assert_eq!(reference_accepts("SELECT 1", d), None);
        }
        // PostgreSQL and SQLite keep a library canonicalizer for fidelity,
        // independent of the validity cache.
        assert!(has_canonical(Dialect::Postgresql));
        assert!(has_canonical(Dialect::Sqlite));
    }

    #[test]
    fn roundtrip_and_fidelity_gating() {
        // No pretty-printer => round-trip is N/A.
        assert_eq!(
            BenchParser::Orql.roundtrips("SELECT 1 FROM dual", Dialect::Oracle),
            None
        );
        assert_eq!(
            BenchParser::Qusql.roundtrips("SELECT 1", Dialect::Postgresql),
            None
        );
        // Fidelity needs a library canonicalizer: None on a dialect without one
        // even for a parser that can reprint.
        assert_eq!(
            BenchParser::Sqlparser.fidelity("SELECT 1", Dialect::Mysql),
            None
        );
        // Reprintable parser on a reference dialect => a verdict (Some).
        assert!(BenchParser::Sqlparser
            .roundtrips("SELECT 1", Dialect::Postgresql)
            .is_some());
        assert!(BenchParser::Sqlparser
            .fidelity("SELECT 1", Dialect::Sqlite)
            .is_some());
    }

    #[test]
    fn can_batch_excludes_only_databend() {
        // databend-common-ast parses one statement per call; every other parser
        // has a multi-statement entry point.
        assert!(!BenchParser::Databend.can_batch());
        for p in BenchParser::all() {
            if p != BenchParser::Databend {
                assert!(p.can_batch(), "{} should support batch parsing", p.name());
            }
        }
    }

    #[test]
    fn parse_batch_counts_a_three_statement_script() {
        let script = "SELECT 1; SELECT 2; SELECT 3";
        // Multi-dialect Vec parsers count every statement.
        assert_eq!(
            BenchParser::Sqlparser.parse_batch(script, Dialect::Postgresql),
            Some(3)
        );
        // Streaming SQLite parsers count every statement too.
        assert_eq!(
            BenchParser::Sqlite3.parse_batch(script, Dialect::Sqlite),
            Some(3)
        );
        assert_eq!(
            BenchParser::Turso.parse_batch(script, Dialect::Sqlite),
            Some(3)
        );
    }

    #[test]
    fn parse_batch_is_none_when_unavailable() {
        // No batch entry point, even on a dialect databend models.
        assert_eq!(
            BenchParser::Databend.parse_batch("SELECT 1", Dialect::Postgresql),
            None
        );
        // Unsupported dialect for a dialect-specific parser.
        assert_eq!(
            BenchParser::Sqlite3.parse_batch("SELECT 1", Dialect::Postgresql),
            None
        );
        assert_eq!(
            BenchParser::Orql.parse_batch("SELECT 1", Dialect::Postgresql),
            None
        );
    }

    #[test]
    fn measure_mem_batch_gating() {
        // The counting allocator only exists in the membench binary, so under
        // cargo test these assert the gating (Some vs None), not byte values.
        let script = "SELECT 1; SELECT 2";
        // Batch-capable, Rust-visible parser: Some (value is (0, 0) here).
        assert!(BenchParser::Sqlparser
            .measure_mem_batch(script, Dialect::Postgresql)
            .is_some());
        // No batch entry point.
        assert_eq!(
            BenchParser::Databend.measure_mem_batch(script, Dialect::Postgresql),
            None
        );
        // Memory invisible to the Rust allocator (parses in C).
        assert_eq!(
            BenchParser::PgQuery.measure_mem_batch(script, Dialect::Postgresql),
            None
        );
        // Unsupported dialect for a dialect-specific parser.
        assert_eq!(
            BenchParser::Sqlite3.measure_mem_batch(script, Dialect::Postgresql),
            None
        );
    }
}

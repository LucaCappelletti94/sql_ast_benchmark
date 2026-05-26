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

#[must_use]
pub fn is_valid_sqlparser(sql: &str) -> bool {
    let dialect = PostgreSqlDialect {};
    Parser::parse_sql(&dialect, sql).is_ok()
}

#[cfg(feature = "pg_query_parser")]
#[must_use]
pub fn is_valid_pg_query(sql: &str) -> bool {
    pg_query::parse(sql).is_ok()
}

#[cfg(feature = "pg_query_parser")]
fn pg_query_canonical(sql: &str) -> Option<String> {
    pg_query::parse(sql).ok()?.deparse().ok()
}

#[must_use]
pub fn concatenate_statements(statements: &[String]) -> String {
    statements.join("; ")
}

// ════════════════════════════════════════════════════════════════════════════
//  Multi-dialect benchmark layer
//
//  Each parser is run in its best-matching dialect for a given corpus. Parsers
//  that do not model a dialect return `None` (reported as N/A). "Correct" is
//  graded against an oracle where one exists (pg_query for PostgreSQL, lemon-rs
//  for SQLite) and otherwise by acceptance rate over a dialect's own corpus.
// ════════════════════════════════════════════════════════════════════════════

// ── Dialect mappings ────────────────────────────────────────────────────────

/// Best-matching sqlparser-rs dialect for a corpus dialect (always available).
/// Trino has no dedicated dialect (uses Generic); Spark SQL maps to Databricks.
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

/// qusql-parse dialect; `None` for dialects it does not model.
const fn qusql_dialect(d: Dialect) -> Option<SQLDialect> {
    match d {
        Dialect::Postgresql => Some(SQLDialect::PostgreSQL),
        Dialect::Mysql => Some(SQLDialect::MariaDB),
        Dialect::Sqlite => Some(SQLDialect::Sqlite),
        _ => None,
    }
}

/// databend dialect; `None` for dialects it does not model.
const fn databend_dialect_of(d: Dialect) -> Option<DatabendDialect> {
    match d {
        Dialect::Postgresql => Some(DatabendDialect::PostgreSQL),
        Dialect::Mysql => Some(DatabendDialect::MySQL),
        Dialect::Hive => Some(DatabendDialect::Hive),
        _ => None,
    }
}

// ── Per-parser primitives (acceptance + reprint) ────────────────────────────

fn qusql_accepts_dialect(sql: &str, d: SQLDialect) -> bool {
    // qusql-parse uses todo!()/panic in some unimplemented paths; treat those
    // as parse failures rather than letting them abort the worker thread.
    std::panic::catch_unwind(|| {
        let opts = ParseOptions::new()
            .dialect(d)
            .arguments(qusql_parse::SQLArguments::Dollar);
        let mut issues = Issues::new(sql);
        let _ = parse_statements(sql, &mut issues, &opts);
        !issues.get().iter().any(|i| i.level == Level::Error)
    })
    .unwrap_or(false)
}

fn databend_accepts_dialect(sql: &str, d: DatabendDialect) -> bool {
    std::panic::catch_unwind(|| {
        databend_tokenize(sql)
            .ok()
            .and_then(|tokens| databend_parse(&tokens, d).ok())
            .is_some()
    })
    .unwrap_or(false)
}

fn sqlite3_accepts(sql: &str) -> bool {
    std::panic::catch_unwind(|| {
        let mut parser = sqlite3_parser::lexer::sql::Parser::new(sql.as_bytes());
        loop {
            match parser.next() {
                Ok(Some(_)) => {}
                Ok(None) => return true,
                Err(_) => return false,
            }
        }
    })
    .unwrap_or(false)
}

/// senax-mysql-parser only parses CREATE TABLE; accepts iff the whole input is
/// a single CREATE TABLE (trailing whitespace / `;` allowed).
fn senax_accepts(sql: &str) -> bool {
    std::panic::catch_unwind(
        || match senax_mysql_parser::create::creation(sql.as_bytes()) {
            Ok((rest, _)) => rest.iter().all(|b| b.is_ascii_whitespace() || *b == b';'),
            Err(_) => false,
        },
    )
    .unwrap_or(false)
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

// ── Oracles (ground truth) ──────────────────────────────────────────────────

/// Canonical form for an oracle-backed dialect, used for fidelity checks.
/// `None` for dialects with no oracle (or when the relevant feature is off).
fn oracle_canonical(sql: &str, d: Dialect) -> Option<String> {
    match d {
        #[cfg(feature = "pg_query_parser")]
        Dialect::Postgresql => pg_query_canonical(sql),
        Dialect::Sqlite => sqlite3_reprint(sql),
        _ => None,
    }
}

/// Does an oracle accept this statement? `Some(true/false)` for oracle-backed
/// dialects (`PostgreSQL` via `pg_query`, `SQLite` via lemon-rs), `None` otherwise.
#[must_use]
pub fn oracle_accepts(sql: &str, d: Dialect) -> Option<bool> {
    match d {
        #[cfg(feature = "pg_query_parser")]
        Dialect::Postgresql => Some(pg_query::parse(sql).is_ok()),
        Dialect::Sqlite => Some(sqlite3_accepts(sql)),
        _ => None,
    }
}

/// Is `d` an oracle-backed dialect (has ground truth for recall/false-positive)?
#[must_use]
pub const fn has_oracle(d: Dialect) -> bool {
    match d {
        #[cfg(feature = "pg_query_parser")]
        Dialect::Postgresql => true,
        Dialect::Sqlite => true,
        _ => false,
    }
}

// ── BenchParser ─────────────────────────────────────────────────────────────

/// A parser under test. The single source of truth for dialect support,
/// acceptance, round-trip stability and oracle fidelity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchParser {
    Sqlparser,
    #[cfg(feature = "pg_query_parser")]
    PgQuery,
    #[cfg(feature = "pg_query_parser")]
    PgQuerySummary,
    #[cfg(feature = "pg_parse_parser")]
    PgParse,
    Qusql,
    Polyglot,
    Databend,
    Orql,
    Sqlglot,
    Sqlite3,
    Senax,
}

impl BenchParser {
    /// All parsers compiled into the current build.
    #[must_use]
    pub fn all() -> Vec<Self> {
        vec![
            Self::Sqlparser,
            #[cfg(feature = "pg_query_parser")]
            Self::PgQuery,
            #[cfg(feature = "pg_query_parser")]
            Self::PgQuerySummary,
            #[cfg(feature = "pg_parse_parser")]
            Self::PgParse,
            Self::Qusql,
            Self::Polyglot,
            Self::Databend,
            Self::Orql,
            Self::Sqlglot,
            Self::Sqlite3,
            Self::Senax,
        ]
    }

    /// Human-readable name for reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sqlparser => "sqlparser-rs",
            #[cfg(feature = "pg_query_parser")]
            Self::PgQuery => "pg_query.rs",
            #[cfg(feature = "pg_query_parser")]
            Self::PgQuerySummary => "pg_query (summary)",
            #[cfg(feature = "pg_parse_parser")]
            Self::PgParse => "pg_parse",
            Self::Qusql => "qusql-parse",
            Self::Polyglot => "polyglot-sql",
            Self::Databend => "databend-common-ast",
            Self::Orql => "orql",
            Self::Sqlglot => "sqlglot-rust",
            Self::Sqlite3 => "sqlite3-parser",
            Self::Senax => "senax-mysql (DDL only)",
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
        match self {
            Self::Sqlparser => Some(Parser::parse_sql(&*sqlparser_dialect(dialect), sql).is_ok()),
            #[cfg(feature = "pg_query_parser")]
            Self::PgQuery => (dialect == Dialect::Postgresql).then(|| pg_query::parse(sql).is_ok()),
            #[cfg(feature = "pg_query_parser")]
            Self::PgQuerySummary => {
                (dialect == Dialect::Postgresql).then(|| pg_query::summary(sql, -1).is_ok())
            }
            #[cfg(feature = "pg_parse_parser")]
            Self::PgParse => (dialect == Dialect::Postgresql).then(|| pg_parse::parse(sql).is_ok()),
            Self::Qusql => qusql_dialect(dialect).map(|d| qusql_accepts_dialect(sql, d)),
            Self::Polyglot => Some(
                std::panic::catch_unwind(|| polyglot_parse(sql, polyglot_dialect(dialect)).is_ok())
                    .unwrap_or(false),
            ),
            Self::Databend => {
                databend_dialect_of(dialect).map(|d| databend_accepts_dialect(sql, d))
            }
            Self::Orql => (dialect == Dialect::Oracle).then(|| {
                std::panic::catch_unwind(|| orql_parser::parse(sql).is_ok()).unwrap_or(false)
            }),
            Self::Sqlglot => Some(
                std::panic::catch_unwind(|| {
                    sqlglot_rust::parser::parse_statements(sql, sqlglot_dialect(dialect)).is_ok()
                })
                .unwrap_or(false),
            ),
            Self::Sqlite3 => (dialect == Dialect::Sqlite).then(|| sqlite3_accepts(sql)),
            Self::Senax => (dialect == Dialect::Mysql).then(|| senax_accepts(sql)),
        }
    }

    /// Parse `sql` once in `dialect` for timing, WITHOUT panic protection.
    /// Returns whether the parse succeeded. Intended only for statements already
    /// known to be in the accepted set (so re-parsing cannot panic); calling it
    /// on rejected/edge-case input may abort the process. Avoiding `catch_unwind`
    /// keeps the timing free of landing-pad overhead and fair across parsers.
    #[must_use]
    pub fn parse_once(self, sql: &str, dialect: Dialect) -> bool {
        match self {
            Self::Sqlparser => Parser::parse_sql(&*sqlparser_dialect(dialect), sql).is_ok(),
            #[cfg(feature = "pg_query_parser")]
            Self::PgQuery => pg_query::parse(sql).is_ok(),
            #[cfg(feature = "pg_query_parser")]
            Self::PgQuerySummary => pg_query::summary(sql, -1).is_ok(),
            #[cfg(feature = "pg_parse_parser")]
            Self::PgParse => pg_parse::parse(sql).is_ok(),
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
            Self::Senax => senax_mysql_parser::create::creation(sql.as_bytes()).is_ok(),
        }
    }

    /// Parse and pretty-print; `None` if the parser has no printer, does not
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
            #[cfg(feature = "pg_query_parser")]
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
            Self::Sqlite3 => dialect == Dialect::Sqlite,
            #[cfg(feature = "pg_query_parser")]
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

    /// Oracle fidelity: the oracle's canonical form of this parser's output
    /// equals the oracle's canonical form of the input. `None` if the parser
    /// cannot reprint or the dialect has no oracle.
    #[must_use]
    pub fn fidelity(self, sql: &str, dialect: Dialect) -> Option<bool> {
        if !self.can_reprint(dialect) || !has_oracle(dialect) {
            return None;
        }
        let Some(out) = self.reprint(sql, dialect) else {
            return Some(false);
        };
        match (
            oracle_canonical(sql, dialect),
            oracle_canonical(&out, dialect),
        ) {
            (Some(a), Some(b)) => Some(a == b),
            _ => Some(false),
        }
    }
}

pub mod datasets;

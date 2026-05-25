use databend_common_ast::parser::{
    parse_sql as databend_parse, tokenize_sql as databend_tokenize, Dialect as DatabendDialect,
};
use orql::parser as orql_parser;
use polyglot_sql::{parse as polyglot_parse, DialectType, Generator as PolyglotGenerator};
use qusql_parse::{parse_statements, Issues, Level, ParseOptions, SQLDialect};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::path::Path;

use crate::datasets::Dialect;
use fallible_iterator::FallibleIterator as _;
use sqlglot_rust::Dialect as SqlglotDialect;
use sqlparser::dialect::{
    BigQueryDialect, ClickHouseDialect, DatabricksDialect, Dialect as SqlparserDialect,
    DuckDbDialect, GenericDialect, HiveDialect, MsSqlDialect, MySqlDialect, OracleDialect,
    RedshiftSqlDialect, SQLiteDialect,
};

fn qusql_parse_options() -> ParseOptions {
    ParseOptions::new()
        .dialect(SQLDialect::PostgreSQL)
        .arguments(qusql_parse::SQLArguments::Dollar)
}

// Dataset files
pub const SPIDER_SELECT_FILE: &str = "spider_select.txt";
pub const GRETEL_SELECT_FILE: &str = "gretel_select.txt";
pub const GRETEL_INSERT_FILE: &str = "gretel_insert.txt";
pub const GRETEL_UPDATE_FILE: &str = "gretel_update.txt";
pub const GRETEL_DELETE_FILE: &str = "gretel_delete.txt";

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
#[must_use]
pub fn is_valid_pg_query_summary(sql: &str) -> bool {
    pg_query::summary(sql, -1).is_ok()
}

#[must_use]
pub fn is_valid_polyglot(sql: &str) -> bool {
    std::panic::catch_unwind(|| polyglot_parse(sql, DialectType::PostgreSQL).is_ok())
        .unwrap_or(false)
}

#[must_use]
pub fn is_valid_qusql_parse(sql: &str) -> bool {
    // qusql-parse can panic (todo!()) on unimplemented paths; treat as failure.
    std::panic::catch_unwind(|| {
        let mut issues = Issues::new(sql);
        let _ = parse_statements(sql, &mut issues, &qusql_parse_options());
        !issues.get().iter().any(|i| i.level == Level::Error)
    })
    .unwrap_or(false)
}

#[cfg(feature = "pg_parse_parser")]
#[must_use]
pub fn is_valid_pg_parse(sql: &str) -> bool {
    pg_parse::parse(sql).is_ok()
}

#[must_use]
pub fn is_valid_databend(sql: &str) -> bool {
    // databend-common-ast can panic on certain inputs instead of returning Err;
    // catch_unwind treats those as parse failures.
    std::panic::catch_unwind(|| {
        databend_tokenize(sql)
            .ok()
            .and_then(|tokens| databend_parse(&tokens, DatabendDialect::PostgreSQL).ok())
            .is_some()
    })
    .unwrap_or(false)
}

// ── Fidelity checks ───────────────────────────────────────────────────────────
// Each function checks whether a parser's output is semantically equivalent to
// the input, using pg_query's canonical deparse as the reference:
//   pg_query_deparse(pg_query_parse(parser_output(sql)))
//     == pg_query_deparse(pg_query_parse(sql))
// Returns false if either parse or deparse fails, or if the canonical forms differ.

#[cfg(feature = "pg_query_parser")]
fn pg_query_canonical(sql: &str) -> Option<String> {
    pg_query::parse(sql).ok()?.deparse().ok()
}

/// sqlparser-rs fidelity: does its pretty-printed output have the same
/// `pg_query` canonical form as the original SQL?
#[cfg(feature = "pg_query_parser")]
#[must_use]
pub fn sqlparser_fidelity(sql: &str) -> bool {
    let dialect = PostgreSqlDialect {};
    let Ok(stmts) = Parser::parse_sql(&dialect, sql) else {
        return false;
    };
    if stmts.is_empty() {
        return false;
    }
    let printed = stmts
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    match (pg_query_canonical(sql), pg_query_canonical(&printed)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// polyglot-sql fidelity: does its generated output have the same
/// `pg_query` canonical form as the original SQL?
#[cfg(feature = "pg_query_parser")]
#[must_use]
pub fn polyglot_fidelity(sql: &str) -> bool {
    std::panic::catch_unwind(|| {
        let Ok(exprs) = polyglot_parse(sql, DialectType::PostgreSQL) else {
            return false;
        };
        if exprs.is_empty() {
            return false;
        }
        let Ok(generated) = PolyglotGenerator::new().generate(&exprs[0]) else {
            return false;
        };
        match (pg_query_canonical(sql), pg_query_canonical(&generated)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    })
    .unwrap_or(false)
}

/// databend fidelity: does its pretty-printed output have the same
/// `pg_query` canonical form as the original SQL?
#[cfg(feature = "pg_query_parser")]
#[must_use]
pub fn databend_fidelity(sql: &str) -> bool {
    std::panic::catch_unwind(|| {
        let Ok(tokens) = databend_tokenize(sql) else {
            return false;
        };
        let Ok((stmt, _)) = databend_parse(&tokens, DatabendDialect::PostgreSQL) else {
            return false;
        };
        let printed = stmt.to_string();
        match (pg_query_canonical(sql), pg_query_canonical(&printed)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    })
    .unwrap_or(false)
}

// ── Round-trip stability checks ───────────────────────────────────────────────
// Each function: parse → pretty-print → re-parse → re-print, check stability.
// Returns false if the parser rejects the input or if the output is unstable.

/// sqlparser-rs: parse → `to_string` → re-parse → `to_string`, check stability
#[must_use]
pub fn sqlparser_roundtrip(sql: &str) -> bool {
    let dialect = PostgreSqlDialect {};
    let Ok(stmts) = Parser::parse_sql(&dialect, sql) else {
        return false;
    };
    if stmts.is_empty() {
        return false;
    }
    let printed = stmts
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    let Ok(stmts2) = Parser::parse_sql(&dialect, &printed) else {
        return false;
    };
    let reprinted = stmts2
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    printed == reprinted
}

#[cfg(feature = "pg_query_parser")]
/// `pg_query`: parse → deparse → re-parse → deparse, check stability
#[must_use]
pub fn pg_query_roundtrip(sql: &str) -> bool {
    let Ok(parsed) = pg_query::parse(sql) else {
        return false;
    };
    let Ok(deparsed) = parsed.deparse() else {
        return false;
    };
    let Ok(parsed2) = pg_query::parse(&deparsed) else {
        return false;
    };
    let Ok(deparsed2) = parsed2.deparse() else {
        return false;
    };
    deparsed == deparsed2
}

/// polyglot-sql: parse → generate → re-parse → generate, check stability
#[must_use]
pub fn polyglot_roundtrip(sql: &str) -> bool {
    std::panic::catch_unwind(|| {
        let Ok(exprs) = polyglot_parse(sql, DialectType::PostgreSQL) else {
            return false;
        };
        if exprs.is_empty() {
            return false;
        }
        let Ok(generated) = PolyglotGenerator::new().generate(&exprs[0]) else {
            return false;
        };
        let Ok(exprs2) = polyglot_parse(&generated, DialectType::PostgreSQL) else {
            return false;
        };
        if exprs2.is_empty() {
            return false;
        }
        let Ok(generated2) = PolyglotGenerator::new().generate(&exprs2[0]) else {
            return false;
        };
        generated == generated2
    })
    .unwrap_or(false)
}

#[must_use]
pub fn is_valid_orql(sql: &str) -> bool {
    std::panic::catch_unwind(|| orql_parser::parse(sql).is_ok()).unwrap_or(false)
}

/// databend-common-ast: parse → `to_string` → re-parse → `to_string`, check stability
#[must_use]
pub fn databend_roundtrip(sql: &str) -> bool {
    std::panic::catch_unwind(|| {
        let Ok(tokens) = databend_tokenize(sql) else {
            return false;
        };
        let Ok((stmt, _)) = databend_parse(&tokens, DatabendDialect::PostgreSQL) else {
            return false;
        };
        let printed = stmt.to_string();
        let Ok(tokens2) = databend_tokenize(&printed) else {
            return false;
        };
        let Ok((stmt2, _)) = databend_parse(&tokens2, DatabendDialect::PostgreSQL) else {
            return false;
        };
        printed == stmt2.to_string()
    })
    .unwrap_or(false)
}

/// Load Spider SELECT statements (real-world text-to-SQL queries)
#[must_use]
pub fn load_spider_select() -> Vec<String> {
    load_statements_from_file(SPIDER_SELECT_FILE)
}

/// Load Gretel SELECT statements (synthetic but realistic)
#[must_use]
pub fn load_gretel_select() -> Vec<String> {
    load_statements_from_file(GRETEL_SELECT_FILE)
}

/// Load all SELECT statements (Spider + Gretel)
#[must_use]
pub fn load_select_statements() -> Vec<String> {
    let mut stmts = load_spider_select();
    stmts.extend(load_gretel_select());
    stmts
}

/// Load INSERT statements (Gretel)
#[must_use]
pub fn load_insert_statements() -> Vec<String> {
    load_statements_from_file(GRETEL_INSERT_FILE)
}

/// Load UPDATE statements (Gretel)
#[must_use]
pub fn load_update_statements() -> Vec<String> {
    load_statements_from_file(GRETEL_UPDATE_FILE)
}

/// Load DELETE statements (Gretel)
#[must_use]
pub fn load_delete_statements() -> Vec<String> {
    load_statements_from_file(GRETEL_DELETE_FILE)
}

/// Load all DML statements
#[must_use]
pub fn load_dml_statements() -> Vec<String> {
    let mut stmts = load_select_statements();
    stmts.extend(load_insert_statements());
    stmts.extend(load_update_statements());
    stmts.extend(load_delete_statements());
    stmts
}

fn load_statements_from_file(filename: &str) -> Vec<String> {
    let path = Path::new(filename);
    if path.exists() {
        std::fs::read_to_string(path)
            .expect("Failed to read statements file")
            .lines()
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect()
    } else {
        eprintln!("Warning: {filename} not found");
        Vec::new()
    }
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

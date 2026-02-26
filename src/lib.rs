use databend_common_ast::parser::{
    parse_sql as databend_parse, tokenize_sql as databend_tokenize, Dialect as DatabendDialect,
};
use orql::parser as orql_parser;
use polyglot_sql::{parse as polyglot_parse, DialectType, Generator as PolyglotGenerator};
use sql_parse::{parse_statements, Issues, Level, ParseOptions, SQLDialect};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::path::Path;

fn sql_parse_options() -> ParseOptions {
    ParseOptions::new().dialect(SQLDialect::PostgreSQL)
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
pub fn is_valid_sql_parse(sql: &str) -> bool {
    // sql-parse uses todo!() in some unimplemented paths (e.g. hex literals);
    // catch_unwind treats those as parse failures.
    std::panic::catch_unwind(|| {
        let mut issues = Issues::new(sql);
        let _ = parse_statements(sql, &mut issues, &sql_parse_options());
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

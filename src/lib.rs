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
pub fn is_valid_sql_parse(sql: &str) -> bool {
    let mut issues = Issues::new(sql);
    let _ = parse_statements(sql, &mut issues, &sql_parse_options());
    !issues.get().iter().any(|i| i.level == Level::Error)
}

#[cfg(feature = "pg_parse_parser")]
#[must_use]
pub fn is_valid_pg_parse(sql: &str) -> bool {
    pg_parse::parse(sql).is_ok()
}

/// Check if valid for both sqlparser-rs and the active FFI parser(s)
#[must_use]
pub fn is_valid_both(sql: &str) -> bool {
    let mut valid = is_valid_sqlparser(sql);

    #[cfg(feature = "pg_query_parser")]
    {
        valid = valid && is_valid_pg_query(sql);
    }

    #[cfg(feature = "pg_parse_parser")]
    {
        valid = valid && is_valid_pg_parse(sql);
    }

    valid
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

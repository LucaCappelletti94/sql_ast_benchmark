use sql_ast_benchmark::{
    is_valid_sql_parse, load_delete_statements, load_insert_statements, load_select_statements,
    load_update_statements,
};

#[cfg(feature = "pg_query_parser")]
use sql_ast_benchmark::is_valid_pg_query;

#[cfg(feature = "pg_parse_parser")]
use sql_ast_benchmark::is_valid_pg_parse;

fn main() {
    let select = load_select_statements();
    let insert = load_insert_statements();
    let update = load_update_statements();
    let delete = load_delete_statements();

    println!("sql-parse compatibility:");
    check_compat("SELECT", &select, is_valid_sql_parse);
    check_compat("INSERT", &insert, is_valid_sql_parse);
    check_compat("UPDATE", &update, is_valid_sql_parse);
    check_compat("DELETE", &delete, is_valid_sql_parse);

    #[cfg(feature = "pg_query_parser")]
    {
        println!("\npg_query compatibility:");
        check_compat("SELECT", &select, is_valid_pg_query);
        check_compat("INSERT", &insert, is_valid_pg_query);
        check_compat("UPDATE", &update, is_valid_pg_query);
        check_compat("DELETE", &delete, is_valid_pg_query);
    }

    #[cfg(feature = "pg_parse_parser")]
    {
        println!("\npg_parse compatibility:");
        check_compat("SELECT", &select, is_valid_pg_parse);
        check_compat("INSERT", &insert, is_valid_pg_parse);
        check_compat("UPDATE", &update, is_valid_pg_parse);
        check_compat("DELETE", &delete, is_valid_pg_parse);
    }
}

fn check_compat(name: &str, stmts: &[String], checker: fn(&str) -> bool) {
    let ok = stmts.iter().filter(|s| checker(s)).count();
    let total = stmts.len();
    let pct = 100.0 * ok as f64 / total as f64;
    println!("  {name}: {ok}/{total} ({pct:.1}%)");
}

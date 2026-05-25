/// Writes `pg_sqlparser_failures.txt`: every statement from the postgresql
/// datasets that `pg_query` accepts but sqlparser-rs (`PostgreSqlDialect`) rejects.
///
/// These are valid `PostgreSQL` statements that represent gaps in sqlparser's
/// `PostgreSQL` coverage.
///
///   cargo run --bin `pg_sqlparser_failures`
#[cfg(feature = "pg_query_parser")]
use sql_ast_benchmark::{is_valid_pg_query, is_valid_sqlparser};

#[cfg(not(feature = "pg_query_parser"))]
fn main() {
    eprintln!("ERROR: pg_query_parser feature is required.");
    std::process::exit(1);
}

#[cfg(feature = "pg_query_parser")]
fn main() {
    use std::fs;

    std::panic::set_hook(Box::new(|_| {}));

    let sources = &[
        "datasets/postgresql/defog_data.txt",
        "datasets/postgresql/defog_sql.txt",
        "datasets/postgresql/pg_regress.txt",
    ];

    let mut failures: Vec<String> = Vec::new();

    for path in sources {
        let Ok(content) = fs::read_to_string(path) else {
            eprintln!("skip: {path} (not found)");
            continue;
        };
        let stmts: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        let before = failures.len();
        for stmt in &stmts {
            if is_valid_pg_query(stmt) && !is_valid_sqlparser(stmt) {
                failures.push(stmt.to_string());
            }
        }
        eprintln!("  {path}: {} gaps", failures.len() - before);
    }

    let out = failures.join("\n") + "\n";
    fs::write("pg_sqlparser_failures.txt", &out).expect("write failed");
    println!(
        "Written {} statements to pg_sqlparser_failures.txt",
        failures.len()
    );
}

//! Historical sqlparser-rs versions, one `Parser` impl per milestone.
//!
//! Adding a version is three lines: a Cargo alias, one `sqlparser_version!`
//! invocation here, and one entry in the registry. Versions that share the
//! public API are covered by the macro; an API break would get its own block.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{Parser, ParserId};

/// Generate a `Parser` impl for one renamed sqlparser crate.
///
/// `$cr` is the `package`-renamed crate (e.g. `sqlparser_v0_50`). The dialect
/// mapper uses only the dialects present across every covered milestone, falling
/// back to `GenericDialect` for the rest, so the same code compiles against each
/// version and the trend stays internally consistent.
macro_rules! sqlparser_version {
    ($name:ident, $cr:ident, $ver:literal, $released:literal) => {
        pub struct $name;

        impl $name {
            fn dialect(d: Dialect) -> Box<dyn $cr::dialect::Dialect> {
                match d {
                    Dialect::Postgresql => Box::new($cr::dialect::PostgreSqlDialect {}),
                    Dialect::Mysql => Box::new($cr::dialect::MySqlDialect {}),
                    Dialect::Sqlite => Box::new($cr::dialect::SQLiteDialect {}),
                    Dialect::Clickhouse => Box::new($cr::dialect::ClickHouseDialect {}),
                    Dialect::Hive => Box::new($cr::dialect::HiveDialect {}),
                    Dialect::Tsql => Box::new($cr::dialect::MsSqlDialect {}),
                    Dialect::Bigquery => Box::new($cr::dialect::BigQueryDialect {}),
                    // Oracle, DuckDB, Redshift, Spark, Trino and Multi did not all
                    // exist as dedicated dialects across these releases, so use the
                    // generic dialect uniformly for them.
                    _ => Box::new($cr::dialect::GenericDialect {}),
                }
            }
        }

        impl Parser for $name {
            fn id(&self) -> ParserId {
                ParserId {
                    family: "sqlparser-rs",
                    version: $ver,
                    released: $released,
                }
            }

            fn supports(&self, _dialect: Dialect) -> bool {
                true
            }

            fn try_parse(&self, sql: &str, dialect: Dialect) -> Option<Result<(), String>> {
                Some(
                    std::panic::catch_unwind(|| {
                        $cr::parser::Parser::parse_sql(&*Self::dialect(dialect), sql)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                    .unwrap_or_else(|_| Err("panicked".to_string())),
                )
            }

            fn parse_once(&self, sql: &str, dialect: Dialect) -> bool {
                $cr::parser::Parser::parse_sql(&*Self::dialect(dialect), sql).is_ok()
            }

            fn parse_batch(&self, sql: &str, dialect: Dialect) -> Option<usize> {
                Some(
                    $cr::parser::Parser::parse_sql(&*Self::dialect(dialect), sql)
                        .map_or(0, |v| v.len()),
                )
            }

            fn can_batch(&self) -> bool {
                true
            }

            fn measure_mem(&self, sql: &str, dialect: Dialect) -> Option<(usize, usize)> {
                use sql_ast_benchmark::mem;
                use std::hint::black_box;
                let before = mem::live();
                mem::reset_peak();
                let ast = $cr::parser::Parser::parse_sql(&*Self::dialect(dialect), sql);
                black_box(&ast);
                let r = (
                    mem::peak().saturating_sub(before),
                    mem::live().saturating_sub(before),
                );
                drop(ast);
                Some(r)
            }

            fn reprint(&self, sql: &str, dialect: Dialect) -> Option<String> {
                std::panic::catch_unwind(|| {
                    let stmts =
                        $cr::parser::Parser::parse_sql(&*Self::dialect(dialect), sql).ok()?;
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
                })
                .unwrap_or(None)
            }

            fn can_reprint(&self, _dialect: Dialect) -> bool {
                true
            }
        }
    };
}

sqlparser_version!(SqlparserV0_40, sqlparser_v0_40, "0.40.0", "2023-11-27");
sqlparser_version!(SqlparserV0_45, sqlparser_v0_45, "0.45.0", "2024-04-12");
sqlparser_version!(SqlparserV0_50, sqlparser_v0_50, "0.50.0", "2024-08-16");
sqlparser_version!(SqlparserV0_55, sqlparser_v0_55, "0.55.0", "2025-03-05");
sqlparser_version!(SqlparserV0_62, sqlparser_v0_62, "0.62.0", "2026-05-07");

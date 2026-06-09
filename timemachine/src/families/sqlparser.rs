//! Historical sqlparser-rs versions, one `Parser` impl per milestone.
//!
//! Adding a version is three lines: a Cargo alias, one `sqlparser_version!`
//! invocation here, and one entry in the registry. Versions that share the
//! public API are covered by the macro. An API break would get its own block.

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

sqlparser_version!(SqlparserV0_30, sqlparser_v0_30, "0.30.0", "2023-01-02");
sqlparser_version!(SqlparserV0_31, sqlparser_v0_31, "0.31.0", "2023-03-01");
sqlparser_version!(SqlparserV0_32, sqlparser_v0_32, "0.32.0", "2023-03-06");
sqlparser_version!(SqlparserV0_33, sqlparser_v0_33, "0.33.0", "2023-04-10");
sqlparser_version!(SqlparserV0_34, sqlparser_v0_34, "0.34.0", "2023-05-19");
sqlparser_version!(SqlparserV0_35, sqlparser_v0_35, "0.35.0", "2023-06-23");
sqlparser_version!(SqlparserV0_36, sqlparser_v0_36, "0.36.1", "2023-07-21");
sqlparser_version!(SqlparserV0_37, sqlparser_v0_37, "0.37.0", "2023-08-22");
sqlparser_version!(SqlparserV0_38, sqlparser_v0_38, "0.38.0", "2023-09-21");
sqlparser_version!(SqlparserV0_39, sqlparser_v0_39, "0.39.0", "2023-10-27");
sqlparser_version!(SqlparserV0_40, sqlparser_v0_40, "0.40.0", "2023-11-27");
sqlparser_version!(SqlparserV0_41, sqlparser_v0_41, "0.41.0", "2023-12-22");
sqlparser_version!(SqlparserV0_42, sqlparser_v0_42, "0.42.0", "2024-01-25");
sqlparser_version!(SqlparserV0_43, sqlparser_v0_43, "0.43.1", "2024-01-25");
sqlparser_version!(SqlparserV0_44, sqlparser_v0_44, "0.44.0", "2024-03-03");
sqlparser_version!(SqlparserV0_45, sqlparser_v0_45, "0.45.0", "2024-04-12");
sqlparser_version!(SqlparserV0_46, sqlparser_v0_46, "0.46.0", "2024-05-03");
sqlparser_version!(SqlparserV0_47, sqlparser_v0_47, "0.47.0", "2024-06-01");
sqlparser_version!(SqlparserV0_48, sqlparser_v0_48, "0.48.0", "2024-07-09");
sqlparser_version!(SqlparserV0_49, sqlparser_v0_49, "0.49.0", "2024-07-23");
sqlparser_version!(SqlparserV0_50, sqlparser_v0_50, "0.50.0", "2024-08-16");
sqlparser_version!(SqlparserV0_51, sqlparser_v0_51, "0.51.0", "2024-09-11");
sqlparser_version!(SqlparserV0_52, sqlparser_v0_52, "0.52.0", "2024-11-11");
sqlparser_version!(SqlparserV0_53, sqlparser_v0_53, "0.53.0", "2024-12-18");
sqlparser_version!(SqlparserV0_54, sqlparser_v0_54, "0.54.0", "2025-01-23");
sqlparser_version!(SqlparserV0_55, sqlparser_v0_55, "0.55.0", "2025-03-05");
sqlparser_version!(SqlparserV0_56, sqlparser_v0_56, "0.56.0", "2025-05-02");
sqlparser_version!(SqlparserV0_57, sqlparser_v0_57, "0.57.0", "2025-06-23");
sqlparser_version!(SqlparserV0_58, sqlparser_v0_58, "0.58.0", "2025-07-24");
sqlparser_version!(SqlparserV0_59, sqlparser_v0_59, "0.59.0", "2025-09-24");
sqlparser_version!(SqlparserV0_60, sqlparser_v0_60, "0.60.0", "2025-12-07");
sqlparser_version!(SqlparserV0_61, sqlparser_v0_61, "0.61.0", "2026-02-10");
sqlparser_version!(SqlparserV0_62, sqlparser_v0_62, "0.62.0", "2026-05-07");

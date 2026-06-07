//! Historical sqlglot-rust versions. Models every dialect and pretty-prints, so
//! it is graded for round-trip and fidelity like the current build.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{Parser, ParserId};

macro_rules! sqlglot_version {
    ($name:ident, $cr:ident, $ver:literal, $released:literal) => {
        pub struct $name;

        impl $name {
            fn dialect(d: Dialect) -> $cr::Dialect {
                match d {
                    Dialect::Postgresql => $cr::Dialect::Postgres,
                    Dialect::Mysql => $cr::Dialect::Mysql,
                    Dialect::Sqlite => $cr::Dialect::Sqlite,
                    Dialect::Clickhouse => $cr::Dialect::ClickHouse,
                    Dialect::Hive => $cr::Dialect::Hive,
                    Dialect::Trino => $cr::Dialect::Trino,
                    Dialect::Duckdb => $cr::Dialect::DuckDb,
                    Dialect::SparkSql => $cr::Dialect::Spark,
                    Dialect::Tsql => $cr::Dialect::Tsql,
                    Dialect::Oracle => $cr::Dialect::Oracle,
                    Dialect::Bigquery => $cr::Dialect::BigQuery,
                    Dialect::Redshift => $cr::Dialect::Redshift,
                    Dialect::Multi => $cr::Dialect::Ansi,
                }
            }
        }

        impl Parser for $name {
            fn id(&self) -> ParserId {
                ParserId {
                    family: "sqlglot-rust",
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
                        $cr::parser::parse_statements(sql, Self::dialect(dialect))
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                    .unwrap_or_else(|_| Err("panicked".to_string())),
                )
            }

            fn parse_once(&self, sql: &str, dialect: Dialect) -> bool {
                $cr::parser::parse_statements(sql, Self::dialect(dialect)).is_ok()
            }

            fn parse_batch(&self, sql: &str, dialect: Dialect) -> Option<usize> {
                Some(
                    $cr::parser::parse_statements(sql, Self::dialect(dialect))
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
                let ast = $cr::parser::parse_statements(sql, Self::dialect(dialect));
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
                    let d = Self::dialect(dialect);
                    let stmts = $cr::parser::parse_statements(sql, d).ok()?;
                    if stmts.is_empty() {
                        return None;
                    }
                    Some(
                        stmts
                            .iter()
                            .map(|s| $cr::generate(s, d))
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

sqlglot_version!(SqlglotV0_9, sqlglot_v0_9, "0.9.37", "2026-05-28");
sqlglot_version!(SqlglotV0_10, sqlglot_v0_10, "0.10.0", "2026-06-03");

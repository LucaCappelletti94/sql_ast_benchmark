//! Historical polyglot-sql versions. Models every dialect and regenerates SQL,
//! so it is graded for round-trip and fidelity.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{Parser, ParserId};

macro_rules! polyglot_version {
    ($name:ident, $cr:ident, $ver:literal, $released:literal) => {
        pub struct $name;

        impl $name {
            fn dialect(d: Dialect) -> $cr::DialectType {
                match d {
                    Dialect::Postgresql => $cr::DialectType::PostgreSQL,
                    Dialect::Mysql => $cr::DialectType::MySQL,
                    Dialect::Sqlite => $cr::DialectType::SQLite,
                    Dialect::Clickhouse => $cr::DialectType::ClickHouse,
                    Dialect::Hive => $cr::DialectType::Hive,
                    Dialect::Trino => $cr::DialectType::Trino,
                    Dialect::Duckdb => $cr::DialectType::DuckDB,
                    Dialect::SparkSql => $cr::DialectType::Spark,
                    Dialect::Tsql => $cr::DialectType::TSQL,
                    Dialect::Oracle => $cr::DialectType::Oracle,
                    Dialect::Bigquery => $cr::DialectType::BigQuery,
                    Dialect::Redshift => $cr::DialectType::Redshift,
                    Dialect::Multi => $cr::DialectType::Generic,
                }
            }
        }

        impl Parser for $name {
            fn id(&self) -> ParserId {
                ParserId {
                    family: "polyglot-sql",
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
                        $cr::parse(sql, Self::dialect(dialect))
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                    .unwrap_or_else(|_| Err("panicked".to_string())),
                )
            }

            fn parse_once(&self, sql: &str, dialect: Dialect) -> bool {
                $cr::parse(sql, Self::dialect(dialect)).is_ok()
            }

            fn parse_batch(&self, sql: &str, dialect: Dialect) -> Option<usize> {
                Some($cr::parse(sql, Self::dialect(dialect)).map_or(0, |v| v.len()))
            }

            fn can_batch(&self) -> bool {
                true
            }

            fn measure_mem(&self, sql: &str, dialect: Dialect) -> Option<(usize, usize)> {
                use sql_ast_benchmark::mem;
                use std::hint::black_box;
                let before = mem::live();
                mem::reset_peak();
                let ast = $cr::parse(sql, Self::dialect(dialect));
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
                    let exprs = $cr::parse(sql, Self::dialect(dialect)).ok()?;
                    if exprs.is_empty() {
                        return None;
                    }
                    $cr::Generator::new().generate(&exprs[0]).ok()
                })
                .unwrap_or(None)
            }

            fn can_reprint(&self, _dialect: Dialect) -> bool {
                true
            }
        }
    };
}

polyglot_version!(PolyglotV0_1, polyglot_v0_1, "0.1.15", "2026-03-16");
polyglot_version!(PolyglotV0_2, polyglot_v0_2, "0.2.3", "2026-04-05");
polyglot_version!(PolyglotV0_3, polyglot_v0_3, "0.3.11", "2026-05-15");
polyglot_version!(PolyglotV0_4, polyglot_v0_4, "0.4.4", "2026-06-03");
polyglot_version!(PolyglotV0_5, polyglot_v0_5, "0.5.1", "2026-06-09");

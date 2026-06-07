//! Historical databend-common-ast versions. Models PostgreSQL, MySQL, and Hive,
//! parses a single statement at a time (no batch entry point), and pretty-prints.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{Parser, ParserId};

macro_rules! databend_version {
    ($name:ident, $cr:ident, $ver:literal, $released:literal) => {
        pub struct $name;

        impl $name {
            fn dialect(d: Dialect) -> Option<$cr::parser::Dialect> {
                match d {
                    Dialect::Postgresql => Some($cr::parser::Dialect::PostgreSQL),
                    Dialect::Mysql => Some($cr::parser::Dialect::MySQL),
                    Dialect::Hive => Some($cr::parser::Dialect::Hive),
                    _ => None,
                }
            }
        }

        impl Parser for $name {
            fn id(&self) -> ParserId {
                ParserId {
                    family: "databend-common-ast",
                    version: $ver,
                    released: $released,
                }
            }

            fn supports(&self, dialect: Dialect) -> bool {
                Self::dialect(dialect).is_some()
            }

            fn try_parse(&self, sql: &str, dialect: Dialect) -> Option<Result<(), String>> {
                let d = Self::dialect(dialect)?;
                Some(
                    std::panic::catch_unwind(|| {
                        let tokens = $cr::parser::tokenize_sql(sql).map_err(|e| e.to_string())?;
                        $cr::parser::parse_sql(&tokens, d)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                    .unwrap_or_else(|_| Err("panicked".to_string())),
                )
            }

            fn parse_once(&self, sql: &str, dialect: Dialect) -> bool {
                let Some(d) = Self::dialect(dialect) else {
                    return false;
                };
                $cr::parser::tokenize_sql(sql)
                    .ok()
                    .and_then(|t| $cr::parser::parse_sql(&t, d).ok())
                    .is_some()
            }

            // Single-statement parser: no multi-statement entry point.
            fn parse_batch(&self, _sql: &str, _dialect: Dialect) -> Option<usize> {
                None
            }

            fn can_batch(&self) -> bool {
                false
            }

            fn measure_mem(&self, sql: &str, dialect: Dialect) -> Option<(usize, usize)> {
                use sql_ast_benchmark::mem;
                use std::hint::black_box;
                let d = Self::dialect(dialect)?;
                let before = mem::live();
                mem::reset_peak();
                let toks = $cr::parser::tokenize_sql(sql);
                let ast = toks.as_ref().ok().map(|t| $cr::parser::parse_sql(t, d));
                black_box((&toks, &ast));
                let r = (
                    mem::peak().saturating_sub(before),
                    mem::live().saturating_sub(before),
                );
                drop(ast);
                drop(toks);
                Some(r)
            }

            fn reprint(&self, sql: &str, dialect: Dialect) -> Option<String> {
                let d = Self::dialect(dialect)?;
                std::panic::catch_unwind(|| {
                    let tokens = $cr::parser::tokenize_sql(sql).ok()?;
                    let (stmt, _) = $cr::parser::parse_sql(&tokens, d).ok()?;
                    Some(stmt.to_string())
                })
                .unwrap_or(None)
            }

            fn can_reprint(&self, dialect: Dialect) -> bool {
                Self::dialect(dialect).is_some()
            }
        }
    };
}

databend_version!(DatabendV0_0, databend_v0_0, "0.0.3", "2024-08-20");
databend_version!(DatabendV0_1, databend_v0_1, "0.1.3", "2024-12-31");
databend_version!(DatabendV0_2, databend_v0_2, "0.2.5", "2026-03-12");

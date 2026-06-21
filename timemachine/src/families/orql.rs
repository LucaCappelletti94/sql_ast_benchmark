//! orql, a pure-Rust Oracle SQL parser focused on SELECT statements. Oracle only
//! and no pretty-printer. Only one release is published, so the history is a
//! single point.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{Parser, ParserId};

macro_rules! orql_version {
    ($name:ident, $cr:ident, $ver:literal, $released:literal) => {
        pub struct $name;

        impl Parser for $name {
            // Surface a caught panic (the adapters fold one into `Err("panicked")`)
            // so `grade_chunk` records the empirical panic rate across releases.
            fn parse_outcome(
                &self,
                sql: &str,
                dialect: Dialect,
            ) -> sql_ast_benchmark::ParseOutcome {
                use sql_ast_benchmark::ParseOutcome;
                match self.try_parse(sql, dialect) {
                    None => ParseOutcome::Unsupported,
                    Some(Ok(())) => ParseOutcome::Accepted,
                    Some(Err(e)) if e == "panicked" => ParseOutcome::Panicked(e),
                    Some(Err(e)) => ParseOutcome::Rejected(e),
                }
            }

            fn id(&self) -> ParserId {
                ParserId {
                    family: "orql",
                    version: $ver,
                    released: $released,
                }
            }

            fn supports(&self, dialect: Dialect) -> bool {
                dialect == Dialect::Oracle
            }

            fn try_parse(&self, sql: &str, dialect: Dialect) -> Option<Result<(), String>> {
                if dialect != Dialect::Oracle {
                    return None;
                }
                Some(
                    std::panic::catch_unwind(|| {
                        $cr::parser::parse(sql)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                    .unwrap_or_else(|_| Err("panicked".to_string())),
                )
            }

            fn parse_once(&self, sql: &str, dialect: Dialect) -> bool {
                dialect == Dialect::Oracle && $cr::parser::parse(sql).is_ok()
            }

            fn parse_batch(&self, sql: &str, dialect: Dialect) -> Option<usize> {
                if dialect != Dialect::Oracle {
                    return None;
                }
                Some($cr::parser::parse(sql).map_or(0, |v| v.len()))
            }

            fn can_batch(&self) -> bool {
                true
            }

            fn measure_mem(&self, sql: &str, dialect: Dialect) -> Option<(usize, usize)> {
                use sql_ast_benchmark::mem;
                use std::hint::black_box;
                if dialect != Dialect::Oracle {
                    return None;
                }
                let before = mem::live();
                mem::reset_peak();
                let ast = $cr::parser::parse(sql);
                black_box(&ast);
                let r = (
                    mem::peak().saturating_sub(before),
                    mem::live().saturating_sub(before),
                );
                drop(ast);
                Some(r)
            }

            fn reprint(&self, _sql: &str, _dialect: Dialect) -> Option<String> {
                None
            }

            fn can_reprint(&self, _dialect: Dialect) -> bool {
                false
            }
        }
    };
}

orql_version!(OrqlV0_1, orql_v0_1, "0.1.0", "2026-01-12");

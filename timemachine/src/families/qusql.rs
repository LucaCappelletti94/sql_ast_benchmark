//! Historical qusql-parse versions. Models PostgreSQL, MariaDB, and SQLite, is
//! resilient (collects ranked issues rather than failing on the first error),
//! and has no pretty-printer (so round-trip and fidelity are N/A).

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{Parser, ParserId};

macro_rules! qusql_version {
    ($name:ident, $cr:ident, $ver:literal, $released:literal) => {
        pub struct $name;

        impl $name {
            fn dialect(d: Dialect) -> Option<$cr::SQLDialect> {
                match d {
                    Dialect::Postgresql => Some($cr::SQLDialect::PostgreSQL),
                    Dialect::Mysql => Some($cr::SQLDialect::MariaDB),
                    Dialect::Sqlite => Some($cr::SQLDialect::Sqlite),
                    _ => None,
                }
            }

            fn options(d: $cr::SQLDialect) -> $cr::ParseOptions {
                $cr::ParseOptions::new()
                    .dialect(d)
                    .arguments($cr::SQLArguments::Dollar)
            }
        }

        impl Parser for $name {
            fn id(&self) -> ParserId {
                ParserId {
                    family: "qusql-parse",
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
                        let opts = Self::options(d);
                        let mut issues = $cr::Issues::new(sql);
                        let _ = $cr::parse_statements(sql, &mut issues, &opts);
                        issues
                            .get()
                            .iter()
                            .find(|i| i.level == $cr::Level::Error)
                            .map_or(Ok(()), |e| Err(e.message.to_string()))
                    })
                    .unwrap_or_else(|_| Err("panicked".to_string())),
                )
            }

            fn parse_once(&self, sql: &str, dialect: Dialect) -> bool {
                let Some(d) = Self::dialect(dialect) else {
                    return false;
                };
                let opts = Self::options(d);
                let mut issues = $cr::Issues::new(sql);
                let _ = $cr::parse_statements(sql, &mut issues, &opts);
                !issues.get().iter().any(|i| i.level == $cr::Level::Error)
            }

            fn parse_batch(&self, sql: &str, dialect: Dialect) -> Option<usize> {
                let d = Self::dialect(dialect)?;
                let opts = Self::options(d);
                let mut issues = $cr::Issues::new(sql);
                let stmts = $cr::parse_statements(sql, &mut issues, &opts);
                if issues.get().iter().any(|i| i.level == $cr::Level::Error) {
                    Some(0)
                } else {
                    Some(stmts.len())
                }
            }

            fn can_batch(&self) -> bool {
                true
            }

            fn measure_mem(&self, sql: &str, dialect: Dialect) -> Option<(usize, usize)> {
                use sql_ast_benchmark::mem;
                use std::hint::black_box;
                let d = Self::dialect(dialect)?;
                let before = mem::live();
                mem::reset_peak();
                let opts = Self::options(d);
                let mut issues = $cr::Issues::new(sql);
                let ast = $cr::parse_statements(sql, &mut issues, &opts);
                black_box((&ast, &issues));
                let r = (
                    mem::peak().saturating_sub(before),
                    mem::live().saturating_sub(before),
                );
                drop(ast);
                drop(issues);
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

// 0.1.0 is excluded: it effectively hangs on parts of the MySQL corpus.
qusql_version!(QusqlV0_2, qusql_v0_2, "0.2.1", "2026-03-27");
qusql_version!(QusqlV0_3, qusql_v0_3, "0.3.0", "2026-03-28");
qusql_version!(QusqlV0_4, qusql_v0_4, "0.4.0", "2026-04-15");
qusql_version!(QusqlV0_5, qusql_v0_5, "0.5.0", "2026-04-19");
qusql_version!(QusqlV0_6, qusql_v0_6, "0.6.0", "2026-04-22");
qusql_version!(QusqlV0_7, qusql_v0_7, "0.7.0", "2026-04-28");
qusql_version!(QusqlV0_8, qusql_v0_8, "0.8.0", "2026-05-03");

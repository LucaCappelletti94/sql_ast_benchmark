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
    // Full dialect set, for releases that model every dialect we map (0.20+).
    ($name:ident, $cr:ident, $ver:literal, $released:literal) => {
        sqlparser_version!($name, $cr, $ver, $released, [
            Postgresql => PostgreSqlDialect,
            Mysql => MySqlDialect,
            Sqlite => SQLiteDialect,
            Clickhouse => ClickHouseDialect,
            Hive => HiveDialect,
            Tsql => MsSqlDialect,
            Bigquery => BigQueryDialect,
        ]);
    };
    // Explicit dialect arms, for older releases that predate some dialects (SQLite
    // arrived in 0.7, Hive in 0.8, ClickHouse in 0.14, BigQuery in 0.18). Any
    // dialect not listed falls back to the generic dialect, the same approach the
    // newest versions use for dialects they do not model.
    ($name:ident, $cr:ident, $ver:literal, $released:literal, [$($variant:ident => $dia:ident),* $(,)?]) => {
        pub struct $name;

        impl $name {
            fn dialect(d: Dialect) -> Box<dyn $cr::dialect::Dialect> {
                match d {
                    $( Dialect::$variant => Box::new($cr::dialect::$dia {}), )*
                    _ => Box::new($cr::dialect::GenericDialect {}),
                }
            }
        }

        impl Parser for $name {
            // Surface a caught panic (the adapters fold one into `Err("panicked")`)
            // so `grade_chunk` records the empirical panic rate across releases.
            fn parse_outcome(&self, sql: &str, dialect: Dialect) -> sql_ast_benchmark::ParseOutcome {
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

// Older releases, with the reduced dialect sets of their era.
sqlparser_version!(SqlparserV0_6, sqlparser_v0_6, "0.6.1", "2020-07-20", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_7, sqlparser_v0_7, "0.7.0", "2020-12-28", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_8, sqlparser_v0_8, "0.8.0", "2021-02-09", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_9, sqlparser_v0_9, "0.9.0", "2021-03-21", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_10, sqlparser_v0_10, "0.10.0", "2021-08-23", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_11, sqlparser_v0_11, "0.11.0", "2021-09-25", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_12, sqlparser_v0_12, "0.12.0", "2021-10-14", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_13, sqlparser_v0_13, "0.13.0", "2021-12-10", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_14, sqlparser_v0_14, "0.14.0", "2022-02-09", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Clickhouse => ClickHouseDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_15, sqlparser_v0_15, "0.15.0", "2022-03-08", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Clickhouse => ClickHouseDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_16, sqlparser_v0_16, "0.16.0", "2022-04-03", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Clickhouse => ClickHouseDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
sqlparser_version!(SqlparserV0_17, sqlparser_v0_17, "0.17.0", "2022-05-10", [Postgresql => PostgreSqlDialect, Mysql => MySqlDialect, Sqlite => SQLiteDialect, Clickhouse => ClickHouseDialect, Hive => HiveDialect, Tsql => MsSqlDialect]);
// 0.18 onward model every dialect we map (BigQuery landed in 0.18), so they use
// the full-set form.
sqlparser_version!(SqlparserV0_18, sqlparser_v0_18, "0.18.0", "2022-06-06");
sqlparser_version!(SqlparserV0_19, sqlparser_v0_19, "0.19.0", "2022-07-28");
sqlparser_version!(SqlparserV0_20, sqlparser_v0_20, "0.20.0", "2022-08-05");
sqlparser_version!(SqlparserV0_21, sqlparser_v0_21, "0.21.0", "2022-08-18");
sqlparser_version!(SqlparserV0_22, sqlparser_v0_22, "0.22.0", "2022-08-26");
sqlparser_version!(SqlparserV0_23, sqlparser_v0_23, "0.23.0", "2022-09-08");
sqlparser_version!(SqlparserV0_24, sqlparser_v0_24, "0.24.0", "2022-09-28");
sqlparser_version!(SqlparserV0_25, sqlparser_v0_25, "0.25.0", "2022-10-03");
sqlparser_version!(SqlparserV0_26, sqlparser_v0_26, "0.26.0", "2022-10-19");
sqlparser_version!(SqlparserV0_27, sqlparser_v0_27, "0.27.0", "2022-11-11");
sqlparser_version!(SqlparserV0_28, sqlparser_v0_28, "0.28.0", "2022-12-05");
sqlparser_version!(SqlparserV0_29, sqlparser_v0_29, "0.29.0", "2022-12-29");
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

#[cfg(test)]
mod tests {
    use super::*;

    // The old-era adapters (reduced dialect sets, generic fallback) must actually
    // parse, not just compile. A plain SELECT is valid in every release back to
    // 0.6, so each oldest-tier version should accept it without panicking.
    #[test]
    fn old_adapters_parse_basic_select() {
        let sql = "SELECT a, b FROM t WHERE a > 1";
        for p in [
            Box::new(SqlparserV0_6) as Box<dyn Parser>,
            Box::new(SqlparserV0_7),
            Box::new(SqlparserV0_9),
            Box::new(SqlparserV0_16),
            Box::new(SqlparserV0_29),
        ] {
            let v = p.id().version;
            assert_eq!(
                p.try_parse(sql, Dialect::Postgresql),
                Some(Ok(())),
                "sqlparser {v} should parse a basic SELECT"
            );
        }
    }
}

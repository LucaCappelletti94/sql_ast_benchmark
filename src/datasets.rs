#![allow(clippy::doc_markdown)]

//! SQL dialects represented in the `datasets/` corpus.
//!
//! The corpus is shipped pre-built and compressed as `datasets.tar.zst` and is
//! organised as `datasets/{dialect}/{name}.txt`. This module only models the
//! dialect of each subdirectory; the fetching/extraction machinery that
//! originally produced the corpus has been removed (see git history).

/// A SQL dialect, matching a subdirectory of `datasets/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Postgresql,
    Mysql,
    Sqlite,
    Clickhouse,
    Hive,
    Trino,
    Duckdb,
    SparkSql,
    Tsql,
    Oracle,
    Bigquery,
    Redshift,
    Multi,
}

impl Dialect {
    /// The `datasets/` subdirectory name for this dialect.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Postgresql => "postgresql",
            Self::Mysql => "mysql",
            Self::Sqlite => "sqlite",
            Self::Clickhouse => "clickhouse",
            Self::Hive => "hive",
            Self::Trino => "trino",
            Self::Duckdb => "duckdb",
            Self::SparkSql => "spark_sql",
            Self::Tsql => "tsql",
            Self::Oracle => "oracle",
            Self::Bigquery => "bigquery",
            Self::Redshift => "redshift",
            Self::Multi => "multi",
        }
    }

    /// Inverse of [`Self::dir_name`]: resolve a `datasets/` subdirectory name.
    #[must_use]
    pub fn from_dir_name(name: &str) -> Option<Self> {
        Some(match name {
            "postgresql" => Self::Postgresql,
            "mysql" => Self::Mysql,
            "sqlite" => Self::Sqlite,
            "clickhouse" => Self::Clickhouse,
            "hive" => Self::Hive,
            "trino" => Self::Trino,
            "duckdb" => Self::Duckdb,
            "spark_sql" => Self::SparkSql,
            "tsql" => Self::Tsql,
            "oracle" => Self::Oracle,
            "bigquery" => Self::Bigquery,
            "redshift" => Self::Redshift,
            "multi" => Self::Multi,
            _ => return None,
        })
    }
}

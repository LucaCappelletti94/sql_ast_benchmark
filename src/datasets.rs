#![allow(clippy::doc_markdown)]

//! SQL dialects represented in the `datasets/` corpus.
//!
//! The corpus ships as `datasets.tar.zst`, organised as
//! `datasets/{dialect}/{name}.txt`, and [`ensure_corpus`] extracts it on demand.
//! This module only models each subdirectory's dialect. The original
//! fetch/extraction machinery has been removed (see git history).

use std::io;
use std::path::Path;

/// Ensure `datasets/` is present, extracting `datasets.tar.zst` if not.
/// Idempotent and a no-op once the corpus is unpacked.
///
/// # Errors
/// Returns an error if `datasets/` is missing and the archive cannot be found
/// or `tar --zstd` fails.
pub fn ensure_corpus() -> io::Result<()> {
    if Path::new("datasets").is_dir() {
        return Ok(());
    }
    if !Path::new("datasets.tar.zst").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "neither datasets/ nor datasets.tar.zst found",
        ));
    }
    let status = std::process::Command::new("tar")
        .args(["--zstd", "-xf", "datasets.tar.zst"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("`tar --zstd -xf datasets.tar.zst` failed"))
    }
}

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

    /// Human-facing dialect name with conventional capitalization (for chart
    /// titles and legends), as opposed to the lowercase [`Self::dir_name`].
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Postgresql => "PostgreSQL",
            Self::Mysql => "MySQL",
            Self::Sqlite => "SQLite",
            Self::Clickhouse => "ClickHouse",
            Self::Hive => "Hive",
            Self::Trino => "Trino",
            Self::Duckdb => "DuckDB",
            Self::SparkSql => "Spark SQL",
            Self::Tsql => "T-SQL",
            Self::Oracle => "Oracle",
            Self::Bigquery => "BigQuery",
            Self::Redshift => "Redshift",
            Self::Multi => "Multi-dialect",
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

#[cfg(test)]
mod tests {
    use super::Dialect;

    const ALL: [Dialect; 13] = [
        Dialect::Postgresql,
        Dialect::Mysql,
        Dialect::Sqlite,
        Dialect::Clickhouse,
        Dialect::Hive,
        Dialect::Trino,
        Dialect::Duckdb,
        Dialect::SparkSql,
        Dialect::Tsql,
        Dialect::Oracle,
        Dialect::Bigquery,
        Dialect::Redshift,
        Dialect::Multi,
    ];

    #[test]
    fn dir_name_roundtrips_for_every_variant() {
        for d in ALL {
            assert_eq!(
                Dialect::from_dir_name(d.dir_name()),
                Some(d),
                "round-trip failed for {d:?}"
            );
        }
    }

    #[test]
    fn dir_names_are_unique() {
        let mut names: Vec<&str> = ALL.iter().map(|d| d.dir_name()).collect();
        names.sort_unstable();
        let len = names.len();
        names.dedup();
        assert_eq!(names.len(), len, "duplicate dir_name across dialects");
    }

    #[test]
    fn from_dir_name_rejects_unknown_and_is_case_sensitive() {
        assert_eq!(Dialect::from_dir_name("nope"), None);
        assert_eq!(Dialect::from_dir_name(""), None);
        assert_eq!(Dialect::from_dir_name("POSTGRESQL"), None);
    }

    #[test]
    fn display_names_are_nonempty_and_unique() {
        let mut names: Vec<&str> = ALL.iter().map(|d| d.display_name()).collect();
        assert!(names.iter().all(|n| !n.is_empty()));
        let len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), len, "duplicate display_name across dialects");
    }
}

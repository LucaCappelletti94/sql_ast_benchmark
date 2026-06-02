//! Per-dialect facts about the database engine behind each SQL dialect, shown
//! as a badge row on each dialect page. Keyed by `dir_name`.
//!
//! This is a hand-recorded snapshot (see [`SNAPSHOT`]) of public facts about
//! each engine (vendor, license, first release, latest stable release,
//! implementation language, and whether the engine itself runs in WebAssembly),
//! not fetched at runtime. Refresh the figures and the date together.

/// The date the engine figures below were collected (ISO 8601).
pub const SNAPSHOT: &str = "2026-06-02";

/// The year [`SNAPSHOT`] falls in, for age math.
const SNAPSHOT_YEAR: u16 = 2026;

/// Facts about the database engine behind one SQL dialect.
pub struct DialectMeta {
    /// Short vendor label for the badge (e.g. "Oracle", "Apache").
    pub vendor: &'static str,
    /// Full vendor name, for the tooltip.
    pub vendor_full: &'static str,
    /// SPDX license id, or "proprietary" for closed/commercial engines.
    pub license: &'static str,
    /// Year the engine was first publicly released.
    pub since: u16,
    /// Latest stable version, or "rolling" for versionless cloud services.
    pub version: &'static str,
    /// Release month of the latest version, "YYYY-MM", or "" when not applicable.
    pub released: &'static str,
    /// Primary implementation language of the engine.
    pub language: &'static str,
    /// Whether the engine itself runs in WebAssembly.
    pub wasm: bool,
    /// The project providing the wasm build (e.g. "PGlite"), or "" when none.
    pub wasm_note: &'static str,
}

/// Engine facts for a dialect's `dir_name`, if recorded. The cross-dialect
/// "multi" corpus and any unknown dir return `None`.
#[must_use]
pub fn dialect_meta(dir: &str) -> Option<DialectMeta> {
    Some(match dir {
        "postgresql" => DialectMeta {
            vendor: "PGDG",
            vendor_full: "PostgreSQL Global Development Group",
            license: "PostgreSQL",
            since: 1996,
            version: "18.4",
            released: "2026-05",
            language: "C",
            wasm: true,
            wasm_note: "PGlite",
        },
        "mysql" => DialectMeta {
            vendor: "Oracle",
            vendor_full: "Oracle (originally MySQL AB)",
            license: "GPL-2.0",
            since: 1995,
            version: "9.7",
            released: "2026-04",
            language: "C++",
            wasm: false,
            wasm_note: "",
        },
        "sqlite" => DialectMeta {
            vendor: "Hwaci",
            vendor_full: "D. Richard Hipp / Hwaci",
            license: "Public Domain",
            since: 2000,
            version: "3.53.1",
            released: "2026-05",
            language: "C",
            wasm: true,
            wasm_note: "sqlite3.wasm",
        },
        "clickhouse" => DialectMeta {
            vendor: "ClickHouse",
            vendor_full: "ClickHouse, Inc. (originally Yandex)",
            license: "Apache-2.0",
            since: 2016,
            version: "26.3",
            released: "2026-05",
            language: "C++",
            wasm: false,
            wasm_note: "",
        },
        "duckdb" => DialectMeta {
            vendor: "DuckDB Labs",
            vendor_full: "DuckDB Labs / CWI",
            license: "MIT",
            since: 2019,
            version: "1.5.3",
            released: "2026-05",
            language: "C++",
            wasm: true,
            wasm_note: "DuckDB-Wasm",
        },
        "hive" => DialectMeta {
            vendor: "Apache",
            vendor_full: "Apache Software Foundation",
            license: "Apache-2.0",
            since: 2010,
            version: "4.2.0",
            released: "2025-11",
            language: "Java",
            wasm: false,
            wasm_note: "",
        },
        "spark_sql" => DialectMeta {
            vendor: "Apache",
            vendor_full: "Apache Software Foundation",
            license: "Apache-2.0",
            since: 2014,
            version: "4.1.2",
            released: "2026-05",
            language: "Scala",
            wasm: false,
            wasm_note: "",
        },
        "trino" => DialectMeta {
            vendor: "Trino SF",
            vendor_full: "Trino Software Foundation (born as Presto)",
            license: "Apache-2.0",
            since: 2013,
            version: "481",
            released: "2026-05",
            language: "Java",
            wasm: false,
            wasm_note: "",
        },
        "tsql" => DialectMeta {
            vendor: "Microsoft",
            vendor_full: "Microsoft",
            license: "proprietary",
            since: 1989,
            version: "2025",
            released: "2025-11",
            language: "C++",
            wasm: false,
            wasm_note: "",
        },
        "oracle" => DialectMeta {
            vendor: "Oracle",
            vendor_full: "Oracle",
            license: "proprietary",
            since: 1979,
            version: "26ai",
            released: "2026-05",
            language: "C",
            wasm: false,
            wasm_note: "",
        },
        "bigquery" => DialectMeta {
            vendor: "Google",
            vendor_full: "Google (Google Cloud)",
            license: "proprietary",
            since: 2011,
            version: "rolling",
            released: "",
            language: "C++",
            wasm: false,
            wasm_note: "",
        },
        "redshift" => DialectMeta {
            vendor: "AWS",
            vendor_full: "Amazon Web Services",
            license: "proprietary",
            since: 2013,
            version: "rolling",
            released: "",
            language: "C++",
            wasm: false,
            wasm_note: "",
        },
        _ => return None,
    })
}

/// Tooltip for the vendor badge.
#[must_use]
pub fn vendor_description(full: &str) -> String {
    format!("Created and maintained by {full}.")
}

/// Tooltip for the license badge.
#[must_use]
pub fn license_description(license: &str) -> String {
    match license {
        "proprietary" => {
            "Proprietary: a closed-source, commercial engine (or managed cloud service)."
                .to_string()
        }
        "Public Domain" => {
            "Public domain: no copyright restrictions, usable for any purpose.".to_string()
        }
        "PostgreSQL" => {
            "The PostgreSQL License: a permissive, BSD/MIT-style open-source license.".to_string()
        }
        "GPL-2.0" => "GPL-2.0: a copyleft open-source license (a commercial license is also sold)."
            .to_string(),
        other => format!("Open source under the {other} license."),
    }
}

/// Tooltip for the first-release badge.
#[must_use]
pub fn since_description(year: u16) -> String {
    let age = SNAPSHOT_YEAR.saturating_sub(year);
    format!("First publicly released in {year}, about {age} years ago.")
}

/// Tooltip for the latest-version badge.
#[must_use]
pub fn version_description(version: &str, released: &str) -> String {
    if version == "rolling" {
        "Continuously deployed managed service with no public version number.".to_string()
    } else if released.is_empty() {
        format!("Latest stable release: {version}.")
    } else {
        format!("Latest stable release {version}, published {released}.")
    }
}

/// Display text for the latest-version badge (version plus its date).
#[must_use]
pub fn version_value(version: &str, released: &str) -> String {
    if released.is_empty() {
        version.to_string()
    } else {
        format!("{version} ({released})")
    }
}

/// Tooltip for the implementation-language badge.
#[must_use]
pub fn language_description(language: &str) -> String {
    format!("The engine is implemented primarily in {language}.")
}

/// Tooltip for the WebAssembly badge.
#[must_use]
pub fn wasm_description(wasm: bool, note: &str) -> String {
    if wasm && !note.is_empty() {
        format!("Runs in WebAssembly via {note}, so the engine itself can execute in the browser.")
    } else if wasm {
        "Has a WebAssembly build, so the engine itself can execute in the browser.".to_string()
    } else {
        "No WebAssembly build: the engine runs only natively or server-side.".to_string()
    }
}

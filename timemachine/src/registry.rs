//! The full set of historical parser versions the time-machine benchmarks.
//!
//! One entry per (family, milestone). The current release of each family is
//! included as the newest point so the trend ends at "now", measured under the
//! same conditions as the older points.

use crate::families::{databend, orql, polyglot, qusql, sqlglot, sqlite3, sqlparser, turso};
use sql_ast_benchmark::Parser;

/// Every benchmarked version, grouped by family in release order (oldest first).
#[must_use]
pub fn all() -> Vec<Box<dyn Parser>> {
    vec![
        Box::new(sqlparser::SqlparserV0_30),
        Box::new(sqlparser::SqlparserV0_31),
        Box::new(sqlparser::SqlparserV0_32),
        Box::new(sqlparser::SqlparserV0_33),
        Box::new(sqlparser::SqlparserV0_34),
        Box::new(sqlparser::SqlparserV0_35),
        Box::new(sqlparser::SqlparserV0_36),
        Box::new(sqlparser::SqlparserV0_37),
        Box::new(sqlparser::SqlparserV0_38),
        Box::new(sqlparser::SqlparserV0_39),
        Box::new(sqlparser::SqlparserV0_40),
        Box::new(sqlparser::SqlparserV0_41),
        Box::new(sqlparser::SqlparserV0_42),
        Box::new(sqlparser::SqlparserV0_43),
        Box::new(sqlparser::SqlparserV0_44),
        Box::new(sqlparser::SqlparserV0_45),
        Box::new(sqlparser::SqlparserV0_46),
        Box::new(sqlparser::SqlparserV0_47),
        Box::new(sqlparser::SqlparserV0_48),
        Box::new(sqlparser::SqlparserV0_49),
        Box::new(sqlparser::SqlparserV0_50),
        Box::new(sqlparser::SqlparserV0_51),
        Box::new(sqlparser::SqlparserV0_52),
        Box::new(sqlparser::SqlparserV0_53),
        Box::new(sqlparser::SqlparserV0_54),
        Box::new(sqlparser::SqlparserV0_55),
        Box::new(sqlparser::SqlparserV0_56),
        Box::new(sqlparser::SqlparserV0_57),
        Box::new(sqlparser::SqlparserV0_58),
        Box::new(sqlparser::SqlparserV0_59),
        Box::new(sqlparser::SqlparserV0_60),
        Box::new(sqlparser::SqlparserV0_61),
        Box::new(sqlparser::SqlparserV0_62),
        Box::new(sqlglot::SqlglotV0_9),
        Box::new(sqlglot::SqlglotV0_10),
        Box::new(polyglot::PolyglotV0_1),
        Box::new(polyglot::PolyglotV0_2),
        Box::new(polyglot::PolyglotV0_3),
        Box::new(polyglot::PolyglotV0_4),
        Box::new(databend::DatabendV0_0),
        Box::new(databend::DatabendV0_1),
        Box::new(databend::DatabendV0_2),
        Box::new(sqlite3::Sqlite3V0_9),
        Box::new(sqlite3::Sqlite3V0_10),
        Box::new(sqlite3::Sqlite3V0_11),
        Box::new(sqlite3::Sqlite3V0_12),
        Box::new(sqlite3::Sqlite3V0_13),
        Box::new(sqlite3::Sqlite3V0_14),
        Box::new(sqlite3::Sqlite3V0_15),
        Box::new(sqlite3::Sqlite3V0_16),
        Box::new(qusql::QusqlV0_2),
        Box::new(qusql::QusqlV0_3),
        Box::new(qusql::QusqlV0_4),
        Box::new(qusql::QusqlV0_5),
        Box::new(qusql::QusqlV0_6),
        Box::new(qusql::QusqlV0_7),
        Box::new(qusql::QusqlV0_8),
        Box::new(turso::TursoV0_6),
        Box::new(orql::OrqlV0_1),
    ]
}

/// Distinct family names present in [`all`], in first-seen order.
#[must_use]
pub fn families() -> Vec<&'static str> {
    let mut seen = Vec::new();
    for p in all() {
        let f = p.id().family;
        if !seen.contains(&f) {
            seen.push(f);
        }
    }
    seen
}

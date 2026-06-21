//! The full set of historical parser versions the time-machine benchmarks.
//!
//! One entry per (family, milestone). The current release of each family is
//! included as the newest point so the trend ends at "now", measured under the
//! same conditions as the older points.
//!
//! Each family is taken back as far as it still builds with a proportionate
//! adapter. Where a line stops, the reason is recorded so the gaps are explicit:
//!
//! - sqlparser-rs: back to 0.6.1 (July 2020). Below 0.6 `parse_sql` returns a
//!   single `ASTNode` instead of `Vec<Statement>`, a different shape that is not
//!   comparable to the rest of the history.
//! - sqlite3-parser: back to 0.9.0. Every release below 0.9 depends on
//!   fallible-iterator 0.2 (the adapter uses 0.3), and 0.1 to 0.5 also use a
//!   divergent generic `Parser::new(input: I)`, so reaching them would need a
//!   second fallible-iterator major version and a separate constructor tier.
//! - qusql-parse: back to 0.2.1. 0.1.0 is excluded because its parser
//!   effectively hangs on parts of the MySQL corpus at full-corpus scale.
//! - polyglot-sql (0.1), databend-common-ast (0.0), sqlglot-rust (0.9),
//!   turso_parser (0.6), orql (0.1): already at their first published release.

use crate::families::{databend, orql, polyglot, qusql, sqlglot, sqlite3, sqlparser, turso};
use sql_ast_benchmark::Parser;

/// Every benchmarked version, grouped by family in release order (oldest first).
#[must_use]
pub fn all() -> Vec<Box<dyn Parser>> {
    vec![
        Box::new(sqlparser::SqlparserV0_6),
        Box::new(sqlparser::SqlparserV0_7),
        Box::new(sqlparser::SqlparserV0_8),
        Box::new(sqlparser::SqlparserV0_9),
        Box::new(sqlparser::SqlparserV0_10),
        Box::new(sqlparser::SqlparserV0_11),
        Box::new(sqlparser::SqlparserV0_12),
        Box::new(sqlparser::SqlparserV0_13),
        Box::new(sqlparser::SqlparserV0_14),
        Box::new(sqlparser::SqlparserV0_15),
        Box::new(sqlparser::SqlparserV0_16),
        Box::new(sqlparser::SqlparserV0_17),
        Box::new(sqlparser::SqlparserV0_18),
        Box::new(sqlparser::SqlparserV0_19),
        Box::new(sqlparser::SqlparserV0_20),
        Box::new(sqlparser::SqlparserV0_21),
        Box::new(sqlparser::SqlparserV0_22),
        Box::new(sqlparser::SqlparserV0_23),
        Box::new(sqlparser::SqlparserV0_24),
        Box::new(sqlparser::SqlparserV0_25),
        Box::new(sqlparser::SqlparserV0_26),
        Box::new(sqlparser::SqlparserV0_27),
        Box::new(sqlparser::SqlparserV0_28),
        Box::new(sqlparser::SqlparserV0_29),
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
        Box::new(polyglot::PolyglotV0_5),
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

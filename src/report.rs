#![allow(clippy::doc_markdown)]

//! Corpus loading and per-(parser, dialect) grading.
//!
//! Shared by the `sqlbench` tool and unit-tested here. `grade_chunk` is the
//! correctness core: it splits a dialect's statements by oracle verdict (where
//! one exists) and tallies per parser recall, false-positive, round-trip and
//! fidelity. It is deterministic, so callers may chunk the corpus and `merge`
//! partial reports for speed.

use crate::datasets::Dialect;
use crate::{has_oracle, oracle_accepts, BenchParser};
use std::fs;
use std::path::Path;

/// Per-parser tallies within one dialect.
#[derive(Clone, Default)]
pub struct ParserStat {
    /// Whether the parser can pretty-print in this dialect (round-trip/fidelity).
    pub can_reprint: bool,
    /// Accepted among oracle-valid statements (recall numerator). For a
    /// provenance dialect (no oracle) every statement is treated as valid, so
    /// this is the plain acceptance count.
    pub accepted_valid: usize,
    /// Accepted among oracle-invalid statements (false-positive numerator).
    pub accepted_invalid: usize,
    /// Round-trip-stable among accepted-valid.
    pub roundtrip_ok: usize,
    /// Oracle-fidelity-preserving among accepted-valid.
    pub fidelity_ok: usize,
}

impl ParserStat {
    const fn merge(&mut self, other: &Self) {
        self.accepted_valid += other.accepted_valid;
        self.accepted_invalid += other.accepted_invalid;
        self.roundtrip_ok += other.roundtrip_ok;
        self.fidelity_ok += other.fidelity_ok;
    }
}

/// Grading of one dialect's corpus across a set of parsers.
pub struct DialectReport {
    pub dialect: Dialect,
    pub has_oracle: bool,
    pub valid_total: usize,
    pub invalid_total: usize,
    pub parsers: Vec<BenchParser>,
    pub stats: Vec<ParserStat>,
}

impl DialectReport {
    /// Zeroed report with `can_reprint` precomputed per parser.
    #[must_use]
    pub fn empty(dialect: Dialect, parsers: &[BenchParser]) -> Self {
        Self {
            dialect,
            has_oracle: has_oracle(dialect),
            valid_total: 0,
            invalid_total: 0,
            parsers: parsers.to_vec(),
            stats: parsers
                .iter()
                .map(|p| ParserStat {
                    can_reprint: p.can_reprint(dialect),
                    ..ParserStat::default()
                })
                .collect(),
        }
    }

    /// Add another report's tallies (same dialect and parser order assumed).
    pub fn merge(&mut self, other: &Self) {
        self.valid_total += other.valid_total;
        self.invalid_total += other.invalid_total;
        for (a, b) in self.stats.iter_mut().zip(other.stats.iter()) {
            a.merge(b);
        }
    }
}

/// Grade a chunk of statements for one dialect. Oracle dialects (PostgreSQL,
/// SQLite) split valid/invalid by the oracle, while provenance dialects treat
/// every statement as valid.
#[must_use]
pub fn grade_chunk(stmts: &[String], dialect: Dialect, parsers: &[BenchParser]) -> DialectReport {
    let oracle = has_oracle(dialect);
    let mut report = DialectReport::empty(dialect, parsers);

    for sql in stmts {
        let is_valid = if oracle {
            oracle_accepts(sql, dialect) == Some(true)
        } else {
            true
        };
        if is_valid {
            report.valid_total += 1;
        } else {
            report.invalid_total += 1;
        }

        for (i, &p) in parsers.iter().enumerate() {
            if p.accepts(sql, dialect) != Some(true) {
                continue;
            }
            if is_valid {
                report.stats[i].accepted_valid += 1;
                if report.stats[i].can_reprint {
                    if p.roundtrips(sql, dialect) == Some(true) {
                        report.stats[i].roundtrip_ok += 1;
                    }
                    if p.fidelity(sql, dialect) == Some(true) {
                        report.stats[i].fidelity_ok += 1;
                    }
                }
            } else {
                report.stats[i].accepted_invalid += 1;
            }
        }
    }
    report
}

/// Number of statements `parser` accepts in `dialect` (per-file coverage).
#[must_use]
pub fn count_accepted(stmts: &[&str], dialect: Dialect, parser: BenchParser) -> usize {
    stmts
        .iter()
        .filter(|s| parser.accepts(s, dialect) == Some(true))
        .count()
}

/// All non-empty statements for a dialect from `datasets/{dir}/*.txt`.
#[must_use]
pub fn load_dialect(dialect: Dialect) -> Vec<String> {
    load_dialect_from(Path::new("datasets"), dialect)
}

/// As [`load_dialect`], but from an arbitrary corpus root (for testing).
#[must_use]
pub fn load_dialect_from(root: &Path, dialect: Dialect) -> Vec<String> {
    let dir = root.join(dialect.dir_name());
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        if let Ok(content) = fs::read_to_string(&f) {
            out.extend(
                content
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(String::from),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{count_accepted, grade_chunk, load_dialect_from, DialectReport};
    use crate::datasets::Dialect;
    use crate::BenchParser;
    use std::fs;
    use std::path::PathBuf;

    /// Unique scratch directory under the system temp dir.
    fn temp_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("sqlbench_{tag}_{}_{nanos}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[cfg(feature = "pg_query_parser")]
    #[test]
    fn grades_postgresql_oracle_split() {
        let stmts = vec!["SELECT 1".to_string(), "SELECT 1 FROM".to_string()];
        let parsers = vec![BenchParser::Sqlparser, BenchParser::PgQuery];
        let r = grade_chunk(&stmts, Dialect::Postgresql, &parsers);

        assert!(r.has_oracle);
        // pg_query accepts "SELECT 1", rejects the truncated one.
        assert_eq!(r.valid_total, 1);
        assert_eq!(r.invalid_total, 1);
        // pg_query is the oracle: full recall, no false positives.
        assert_eq!(r.stats[1].accepted_valid, 1);
        assert_eq!(r.stats[1].accepted_invalid, 0);
        // sqlparser accepts the valid statement.
        assert_eq!(r.stats[0].accepted_valid, 1);
    }

    #[test]
    fn provenance_dialect_treats_everything_as_valid() {
        let stmts = vec!["SELECT 1".to_string()];
        let parsers = vec![BenchParser::Sqlparser];
        let r = grade_chunk(&stmts, Dialect::Clickhouse, &parsers);

        assert!(!r.has_oracle);
        assert_eq!(r.valid_total, 1);
        assert_eq!(r.invalid_total, 0);
        assert_eq!(r.stats[0].accepted_valid, 1);
    }

    #[test]
    fn merge_sums_tallies() {
        let parsers = vec![BenchParser::Sqlparser];
        let mut a = DialectReport::empty(Dialect::Mysql, &parsers);
        a.valid_total = 2;
        a.stats[0].accepted_valid = 1;
        let mut b = DialectReport::empty(Dialect::Mysql, &parsers);
        b.valid_total = 3;
        b.stats[0].accepted_valid = 2;
        a.merge(&b);
        assert_eq!(a.valid_total, 5);
        assert_eq!(a.stats[0].accepted_valid, 3);
    }

    #[test]
    fn count_accepted_counts_only_accepted() {
        let stmts = ["SELECT 1", "SELECT 1 FROM"];
        assert_eq!(
            count_accepted(&stmts, Dialect::Postgresql, BenchParser::Sqlparser),
            1
        );
    }

    #[test]
    fn load_dialect_reads_sorted_nonblank_from_txt_only() {
        let root = temp_root("load");
        let dir = root.join("postgresql");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), "SELECT 1\n\n   \nSELECT 2\n").unwrap();
        fs::write(dir.join("b.txt"), "SELECT 3\n").unwrap();
        fs::write(dir.join("notes.md"), "ignored\n").unwrap();

        let got = load_dialect_from(&root, Dialect::Postgresql);
        // a.txt before b.txt (sorted), blank lines dropped, .md ignored.
        assert_eq!(got, vec!["SELECT 1", "SELECT 2", "SELECT 3"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_dialect_missing_dir_is_empty() {
        let root = temp_root("missing");
        assert!(load_dialect_from(&root, Dialect::Mysql).is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}

#![allow(clippy::doc_markdown)]

//! Corpus loading and per-(parser, dialect) grading.
//!
//! Shared by the `sqlbench` tool and unit-tested here. `grade_chunk` is the
//! correctness core: it splits a dialect's statements by reference verdict (where
//! one exists) and tallies per parser recall, false-positive, round-trip and
//! fidelity. It is deterministic, so callers may chunk the corpus and `merge`
//! partial reports for speed.

use crate::datasets::Dialect;
use crate::{has_reference, reference_accepts, Parser, ParserId};
use std::fs;
use std::path::{Path, PathBuf};

/// Large worker stack: deeply nested SQL overflows recursive-descent parsers
/// and a stack overflow aborts the process (uncatchable), so grading runs on
/// threads with this much headroom.
pub const WORKER_STACK: usize = 512 * 1024 * 1024;

/// Per-parser tallies within one dialect.
#[derive(Clone, Default)]
pub struct ParserStat {
    /// Whether the parser can pretty-print in this dialect (round-trip/fidelity).
    pub can_reprint: bool,
    /// Accepted among reference-valid statements (recall numerator). For a
    /// provenance dialect (no reference) every statement is treated as valid, so
    /// this is the plain acceptance count.
    pub accepted_valid: usize,
    /// Accepted among reference-invalid statements (false-positive numerator).
    pub accepted_invalid: usize,
    /// Round-trip-stable among accepted-valid.
    pub roundtrip_ok: usize,
    /// Reference-fidelity-preserving among accepted-valid.
    pub fidelity_ok: usize,
    /// Statements the parser attempted in this dialect (the panic-rate
    /// denominator): every graded statement, since a supporting parser is run on
    /// all of them. Zero for a parser that does not model the dialect.
    pub attempted: usize,
    /// Statements on which the parser threw a caught panic instead of returning a
    /// result (the empirical panic-rate numerator).
    pub panicked: usize,
}

impl ParserStat {
    const fn merge(&mut self, other: &Self) {
        self.accepted_valid += other.accepted_valid;
        self.accepted_invalid += other.accepted_invalid;
        self.roundtrip_ok += other.roundtrip_ok;
        self.fidelity_ok += other.fidelity_ok;
        self.attempted += other.attempted;
        self.panicked += other.panicked;
    }
}

/// Grading of one dialect's corpus across a set of parsers.
pub struct DialectReport {
    pub dialect: Dialect,
    pub has_reference: bool,
    pub valid_total: usize,
    pub invalid_total: usize,
    /// Identity (family + version) of each graded parser, aligned with `stats`.
    pub parsers: Vec<ParserId>,
    pub stats: Vec<ParserStat>,
}

impl DialectReport {
    /// Zeroed report with `can_reprint` precomputed per parser.
    #[must_use]
    pub fn empty(dialect: Dialect, parsers: &[&dyn Parser]) -> Self {
        Self {
            dialect,
            has_reference: has_reference(dialect),
            valid_total: 0,
            invalid_total: 0,
            parsers: parsers.iter().map(|p| p.id()).collect(),
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

/// Grade a chunk of statements for one dialect. Reference dialects (PostgreSQL,
/// SQLite) split valid/invalid by the reference, while provenance dialects treat
/// every statement as valid.
#[must_use]
pub fn grade_chunk(stmts: &[String], dialect: Dialect, parsers: &[&dyn Parser]) -> DialectReport {
    let reference = has_reference(dialect);
    let mut report = DialectReport::empty(dialect, parsers);

    for sql in stmts {
        let is_valid = if reference {
            match reference_accepts(sql, dialect) {
                Some(v) => v,
                // Coverage miss: the cache does not cover this statement (stale
                // cache). Skip it rather than mislabel it invalid.
                None => continue,
            }
        } else {
            true
        };
        if is_valid {
            report.valid_total += 1;
        } else {
            report.invalid_total += 1;
        }

        for (i, &p) in parsers.iter().enumerate() {
            // A panic is still a non-acceptance (it does not enter the accepted
            // tallies), but it is counted separately for the panic-rate metric.
            match p.parse_outcome(sql, dialect) {
                crate::ParseOutcome::Unsupported => continue,
                crate::ParseOutcome::Panicked(_) => {
                    report.stats[i].attempted += 1;
                    report.stats[i].panicked += 1;
                    continue;
                }
                crate::ParseOutcome::Rejected(_) => {
                    report.stats[i].attempted += 1;
                    continue;
                }
                crate::ParseOutcome::Accepted => report.stats[i].attempted += 1,
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
pub fn count_accepted(stmts: &[&str], dialect: Dialect, parser: &dyn Parser) -> usize {
    stmts
        .iter()
        .filter(|s| parser.accepts(s, dialect) == Some(true))
        .count()
}

/// Grade one dialect, parallelising over statement chunks on [`WORKER_STACK`]
/// threads. `None` if the dialect has no corpus. Used by `sqlbench correctness`
/// and `sqlbench export`.
///
/// # Panics
/// Panics if a worker thread cannot be spawned or panics while grading.
#[must_use]
pub fn grade_dialect(dialect: Dialect, all_parsers: &[&dyn Parser]) -> Option<DialectReport> {
    let stmts = load_dialect(dialect);
    if stmts.is_empty() {
        return None;
    }
    let parsers: Vec<&dyn Parser> = all_parsers
        .iter()
        .copied()
        .filter(|p| p.supports(dialect))
        .collect();

    let n_threads = std::thread::available_parallelism()
        .map_or(8, std::num::NonZeroUsize::get)
        .min(32);
    let chunk = stmts.len().div_ceil(n_threads).max(1);

    let merged = std::thread::scope(|scope| {
        let handles: Vec<_> = stmts
            .chunks(chunk)
            .map(|c| {
                let parsers = &parsers;
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, move || grade_chunk(c, dialect, parsers))
                    .expect("spawn worker")
            })
            .collect();
        let mut acc = DialectReport::empty(dialect, &parsers);
        for h in handles {
            acc.merge(&h.join().expect("grade thread panicked"));
        }
        acc
    });
    Some(merged)
}

/// One dataset file's acceptance counts, aligned to a parser column order.
pub struct FileCoverage {
    pub name: String,
    pub total: usize,
    /// Accepted count per parser, in the order returned alongside this.
    pub accepted: Vec<usize>,
}

/// Per-file acceptance for a dialect, over the parsers that support it.
///
/// Returns the supporting parsers (column order) and one [`FileCoverage`] per
/// `datasets/{dir}/*.txt`, sorted by filename, each graded on a
/// [`WORKER_STACK`] thread.
///
/// # Panics
/// Panics if a worker thread cannot be spawned.
#[allow(clippy::needless_collect)] // handles must all spawn before any join
#[must_use]
pub fn coverage_dialect(
    dialect: Dialect,
    all_parsers: &[&dyn Parser],
) -> (Vec<ParserId>, Vec<FileCoverage>) {
    let parsers: Vec<&dyn Parser> = all_parsers
        .iter()
        .copied()
        .filter(|p| p.supports(dialect))
        .collect();

    let dir = Path::new("datasets").join(dialect.dir_name());
    let Ok(entries) = fs::read_dir(&dir) else {
        return (parsers.iter().map(|p| p.id()).collect(), Vec::new());
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();

    let stats = std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .iter()
            .map(|path| {
                let parsers = &parsers;
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, move || eval_file(path, dialect, parsers))
                    .expect("spawn worker")
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect()
    });
    let ids = parsers.iter().map(|p| p.id()).collect();
    (ids, stats)
}

/// The statements one parser rejected in one dialect, with the corpus total.
pub struct ParserFailures {
    pub parser: ParserId,
    /// Statements the parser failed to accept, in corpus order.
    pub rejected: Vec<String>,
    /// The parser's error message for each rejected statement, aligned with
    /// `rejected` (same length, same order).
    pub reasons: Vec<String>,
    /// Total statements graded for the dialect (denominator for the count).
    pub total: usize,
}

/// For each parser that supports `dialect`, collect the statements it rejected.
///
/// These are the actionable "should parse but did not" cases a parser author
/// would want to fix. Reference-invalid statements are excluded so the set stays
/// meaningful: only statements the parser ought to accept (reference-valid, or
/// provenance-valid where there is no reference).
///
/// # Panics
/// Panics if a worker thread cannot be spawned or panics while grading.
#[must_use]
pub fn failures_dialect(dialect: Dialect, all_parsers: &[&dyn Parser]) -> Vec<ParserFailures> {
    failures_dialect_from(Path::new("datasets"), dialect, all_parsers)
}

/// As [`failures_dialect`], but from an arbitrary corpus root (for testing).
///
/// # Panics
/// Panics if a worker thread cannot be spawned or panics while grading.
#[allow(clippy::needless_collect)] // handles must all spawn before any join
#[must_use]
pub fn failures_dialect_from(
    root: &Path,
    dialect: Dialect,
    all_parsers: &[&dyn Parser],
) -> Vec<ParserFailures> {
    let stmts = load_dialect_from(root, dialect);
    if stmts.is_empty() {
        return Vec::new();
    }
    let reference = has_reference(dialect);
    // Only statements the parser is expected to accept count as failures.
    let expected: Vec<&String> = stmts
        .iter()
        .filter(|s| !reference || reference_accepts(s, dialect) == Some(true))
        .collect();
    let total = expected.len();

    let parsers: Vec<&dyn Parser> = all_parsers
        .iter()
        .copied()
        .filter(|p| p.supports(dialect))
        .collect();

    // One worker per parser: each scans the expected-valid statements and keeps
    // the rejects. Parsing dominates, so parser-level parallelism is plenty.
    std::thread::scope(|scope| {
        let handles: Vec<_> = parsers
            .iter()
            .map(|&p| {
                let expected = &expected;
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, move || {
                        // Parse each expected statement once, keeping the ones the
                        // parser rejects alongside the reason it gave.
                        let mut rejected = Vec::new();
                        let mut reasons = Vec::new();
                        for &s in expected {
                            if let Some(Err(reason)) = p.try_parse(s, dialect) {
                                rejected.push(s.clone());
                                reasons.push(reason);
                            }
                        }
                        ParserFailures {
                            parser: p.id(),
                            rejected,
                            reasons,
                            total,
                        }
                    })
                    .expect("spawn worker")
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("failure thread panicked"))
            .collect()
    })
}

/// Acceptance counts for one dataset file (None if unreadable or empty).
fn eval_file(path: &Path, dialect: Dialect, parsers: &[&dyn Parser]) -> Option<FileCoverage> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let content = fs::read_to_string(path).ok()?;
    let stmts: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if stmts.is_empty() {
        return None;
    }
    let accepted = parsers
        .iter()
        .map(|&p| count_accepted(&stmts, dialect, p))
        .collect();
    Some(FileCoverage {
        name,
        total: stmts.len(),
        accepted,
    })
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
    use super::{count_accepted, eval_file, grade_chunk, load_dialect_from, DialectReport};
    use crate::datasets::Dialect;
    use crate::{BenchParser, Parser};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn eval_file_counts_nonblank_and_acceptance() {
        let root = temp_root("evalfile");
        let p = root.join("q.txt");
        fs::write(&p, "SELECT 1\n\nSELECT 1 FROM\n").unwrap();
        let sp = BenchParser::Sqlparser;
        let parsers: [&dyn Parser; 1] = [&sp];
        let fc = eval_file(&p, Dialect::Postgresql, &parsers).unwrap();
        assert_eq!(fc.total, 2); // two non-blank lines
        assert_eq!(fc.accepted[0], 1); // sqlparser accepts "SELECT 1", rejects truncated
        let _ = fs::remove_dir_all(&root);
    }

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

    /// A parser whose outcome is driven by the statement text, so grading of
    /// panics can be exercised deterministically without a real parser that
    /// happens to panic. "OK" is accepted, "PANIC" panics, anything else rejects.
    struct MockParser;

    impl crate::Parser for MockParser {
        fn id(&self) -> crate::ParserId {
            crate::ParserId {
                family: "mock",
                version: "0",
                released: "",
            }
        }
        fn supports(&self, _d: Dialect) -> bool {
            true
        }
        fn try_parse(&self, sql: &str, dialect: Dialect) -> Option<Result<(), String>> {
            match self.parse_outcome(sql, dialect) {
                crate::ParseOutcome::Unsupported => None,
                crate::ParseOutcome::Accepted => Some(Ok(())),
                crate::ParseOutcome::Rejected(e) | crate::ParseOutcome::Panicked(e) => Some(Err(e)),
            }
        }
        fn parse_outcome(&self, sql: &str, _d: Dialect) -> crate::ParseOutcome {
            match sql {
                "OK" => crate::ParseOutcome::Accepted,
                "PANIC" => crate::ParseOutcome::Panicked("boom".to_string()),
                _ => crate::ParseOutcome::Rejected("nope".to_string()),
            }
        }
        fn parse_once(&self, sql: &str, _d: Dialect) -> bool {
            sql == "OK"
        }
        fn parse_batch(&self, _sql: &str, _d: Dialect) -> Option<usize> {
            None
        }
        fn can_batch(&self) -> bool {
            false
        }
        fn measure_mem(&self, _sql: &str, _d: Dialect) -> Option<(usize, usize)> {
            None
        }
        fn reprint(&self, _sql: &str, _d: Dialect) -> Option<String> {
            None
        }
        fn can_reprint(&self, _d: Dialect) -> bool {
            false
        }
    }

    #[test]
    fn grade_chunk_counts_panics_separately_from_rejections() {
        let stmts = vec![
            "OK".to_string(),
            "PANIC".to_string(),
            "PANIC".to_string(),
            "reject".to_string(),
        ];
        let mock = MockParser;
        let parsers: [&dyn Parser; 1] = [&mock];
        // Multi is a provenance dialect: every statement is treated as valid.
        let r = grade_chunk(&stmts, Dialect::Multi, &parsers);
        let s = &r.stats[0];
        assert_eq!(s.attempted, 4, "every statement attempted");
        assert_eq!(s.panicked, 2, "two panics counted");
        assert_eq!(s.accepted_valid, 1, "only OK accepted");
        // A panic is a non-acceptance: it must not inflate the accepted tallies.
        assert_eq!(s.accepted_invalid, 0);
    }

    #[test]
    fn provenance_dialect_treats_everything_as_valid() {
        let stmts = vec!["SELECT 1".to_string()];
        let sp = BenchParser::Sqlparser;
        let parsers: [&dyn Parser; 1] = [&sp];
        // "Multi" never has a reference engine, so it stays provenance-graded.
        let r = grade_chunk(&stmts, Dialect::Multi, &parsers);

        assert!(!r.has_reference);
        assert_eq!(r.valid_total, 1);
        assert_eq!(r.invalid_total, 0);
        assert_eq!(r.stats[0].accepted_valid, 1);
    }

    #[test]
    fn failures_collects_rejected_expected_statements() {
        // On a provenance dialect (no reference engine), every statement is
        // expected-valid, so a parser's rejections are exactly its failures.
        // "Multi" never has a reference cache, keeping this test self-contained.
        let root = temp_root("failures_multi");
        let dir = root.join("multi");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("q.txt"), "SELECT 1\nSELECT 1 FROM\n").unwrap();

        let bp = BenchParser::Sqlparser;
        let parsers: [&dyn Parser; 1] = [&bp];
        let fails = super::failures_dialect_from(&root, Dialect::Multi, &parsers);
        let sp = fails
            .iter()
            .find(|f| f.parser.family == "sqlparser-rs")
            .unwrap();
        // Both statements are expected (provenance treats all as valid).
        assert_eq!(sp.total, 2);
        // sqlparser accepts "SELECT 1" and rejects the truncated one.
        assert!(sp.rejected.iter().any(|s| s == "SELECT 1 FROM"));
        assert!(!sp.rejected.iter().any(|s| s == "SELECT 1"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn failures_empty_for_missing_corpus() {
        let root = temp_root("failures_missing");
        let bp = BenchParser::Sqlparser;
        let parsers: [&dyn Parser; 1] = [&bp];
        let fails = super::failures_dialect_from(&root, Dialect::Trino, &parsers);
        assert!(fails.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn merge_sums_tallies() {
        let bp = BenchParser::Sqlparser;
        let parsers: [&dyn Parser; 1] = [&bp];
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
        let sp = BenchParser::Sqlparser;
        assert_eq!(
            count_accepted(&stmts, Dialect::Postgresql, &sp as &dyn Parser),
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

//! Reads the raw per-statement timings that `cargo bench` writes.
//!
//! The timings live in `target/bench_dist/` and are consumed by the JSON
//! [`crate::export`] step that feeds the web explorer. One file per
//! `(dialect, parser)`, one ns value per line.

use crate::stats::slug;
use std::fs;

/// Directory where the benchmark writes raw per-statement timing files.
pub const DIST_DIR: &str = "target/bench_dist";

/// Directory where `membench` writes raw per-statement memory files.
pub const MEM_DIR: &str = "target/mem_dist";

/// Directory where the batch (whole-script) time bench writes its summary.
pub const BATCH_DIST_DIR: &str = "target/batch_dist";

/// Directory where `membench -- batch` writes its batch-memory summary.
pub const BATCH_MEM_DIR: &str = "target/batch_mem_dist";

/// Ascending-sorted byte values for one `(dialect, parser, kind)`, where `kind`
/// is `"peak"` or `"retained"`, from `target/mem_dist/{dialect}__{slug}.{kind}.txt`
/// (empty if absent).
#[must_use]
pub fn load_mem(dialect: &str, parser: &str, kind: &str) -> Vec<f64> {
    let path = format!("{MEM_DIR}/{dialect}__{}.{kind}.txt", slug(parser));
    fs::read_to_string(path)
        .map(|c| parse_times(&c))
        .unwrap_or_default()
}

/// Ascending-sorted ns timings for one `(dialect, parser)` from its raw
/// `target/bench_dist/{dialect}__{slug}.txt` file (empty if absent).
#[must_use]
pub fn load_times(dialect: &str, parser: &str) -> Vec<f64> {
    let path = format!("{DIST_DIR}/{dialect}__{}.txt", slug(parser));
    fs::read_to_string(path)
        .map(|c| parse_times(&c))
        .unwrap_or_default()
}

/// Parse one-value-per-line ns timings: drop blanks/unparsable/non-positive,
/// return ascending-sorted.
fn parse_times(content: &str) -> Vec<f64> {
    let mut v: Vec<f64> = content
        .lines()
        .filter_map(|l| l.trim().parse::<f64>().ok())
        .filter(|x| *x > 0.0)
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

#[cfg(test)]
mod tests {
    use super::{load_times, parse_times};

    #[test]
    fn parse_times_drops_junk_and_sorts() {
        let v = parse_times("30\n\n  10  \nnot-a-number\n-5\n0\n20\n");
        assert_eq!(v, vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn parse_times_empty_input() {
        assert!(parse_times("").is_empty());
    }

    #[test]
    fn load_times_missing_file_is_empty() {
        assert!(load_times("nope_dialect", "nope_parser").is_empty());
    }
}

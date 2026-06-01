//! Reads the raw per-statement timings that `cargo bench` writes.
//!
//! The timings live in `target/bench_dist/` and are consumed by the JSON
//! [`crate::export`] step that feeds the web explorer. One file per
//! `(dialect, parser)`, one ns value per line.

use crate::stats::slug;
use std::fs;

/// Directory where the benchmark writes raw per-statement timing files.
pub const DIST_DIR: &str = "target/bench_dist";

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

//! The committed-snapshot JSON schema, shared by `sqlbench export` (serialize)
//! and the `web` viewer (deserialize).

use serde::{Deserialize, Serialize};

/// Top-level results bundle (one committed `bench.json`).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Bundle {
    /// RFC3339 timestamp of when the snapshot was exported.
    pub generated_utc: String,
    /// Short git commit the snapshot was built from, if known.
    pub git_commit: Option<String>,
    /// All parser display names that appear anywhere, in palette order.
    pub parsers: Vec<String>,
    /// One entry per dialect, in display order.
    pub dialects: Vec<DialectData>,
}

/// Everything the viewer shows for one dialect.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DialectData {
    pub dir_name: String,
    pub display_name: String,
    pub has_reference: bool,
    pub valid_total: usize,
    pub invalid_total: usize,
    /// Per-parser correctness metrics (reference dialects) or acceptance
    /// (provenance dialects).
    pub correctness: Vec<ParserMetrics>,
    /// Per-parser timing distribution (percentiles + downsampled eCDF).
    pub perf: Vec<ParserPerf>,
    /// Per-file acceptance matrix.
    pub coverage: CoverageMatrix,
}

/// Correctness metrics for one parser in one dialect. Percentages are
/// precomputed as `Option<f64>` so the viewer does pure formatting (None = N/A),
/// matching the CLI's semantics.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParserMetrics {
    pub parser: String,
    pub accepted_valid: usize,
    pub accepted_invalid: usize,
    /// Reference dialects: accepted among reference-valid.
    pub recall_pct: Option<f64>,
    /// Reference dialects: accepted among reference-invalid (lower is better).
    pub false_positive_pct: Option<f64>,
    /// Display round-trip rate among accepted (None without a printer).
    pub roundtrip_pct: Option<f64>,
    /// Reference dialects: canonical-form fidelity among accepted.
    pub fidelity_pct: Option<f64>,
    /// Provenance dialects: fraction of the corpus accepted.
    pub accept_pct: Option<f64>,
}

/// Timing distribution for one parser in one dialect.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParserPerf {
    pub parser: String,
    pub n_total: usize,
    pub n_accepted: usize,
    pub min: f64,
    pub p10: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
    /// Display round-trip rate among accepted (None without a printer).
    pub roundtrip_pct: Option<f64>,
    /// Downsampled empirical CDF: `[ns, fraction]` points, ascending.
    pub ecdf: Vec<[f64; 2]>,
}

/// Per-file acceptance matrix for one dialect.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CoverageMatrix {
    /// Column order (parsers that model this dialect).
    pub parsers: Vec<String>,
    pub files: Vec<CoverageFile>,
    pub subtotal_total: usize,
    /// Per-column accepted totals, same order as `parsers`.
    pub subtotal_accepted: Vec<usize>,
}

/// One dataset file's acceptance counts.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CoverageFile {
    pub name: String,
    pub total: usize,
    /// Per-column accepted counts, same order as `CoverageMatrix::parsers`.
    pub accepted: Vec<usize>,
}

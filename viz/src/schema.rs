//! The committed-snapshot JSON schema, shared by `sqlbench export` (serialize)
//! and the `web` viewer (deserialize).

use serde::{Deserialize, Serialize};

/// Top-level results bundle (one committed `bench.json.zst`).
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
    /// Per-parser rejected-statement previews and download info.
    #[serde(default)]
    pub failures: Vec<ParserFailures>,
    /// Per-parser memory distribution (peak and retained bytes per statement).
    #[serde(default)]
    pub memory: Vec<ParserMem>,
    /// Per-parser whole-script (batch) results: the cost of parsing the whole
    /// accepted set as one script, normalized per statement.
    #[serde(default)]
    pub batch: Vec<ParserBatch>,
}

/// Whole-script (batch) parse results for one parser in one dialect.
///
/// The parser's whole accepted set is concatenated into a single script and
/// parsed in one call; the cost is divided by the statement count. This
/// complements the per-statement [`ParserPerf`]/[`ParserMem`], exposing the
/// amortization a batch API gains or loses (a grown `Vec` of statements, all
/// ASTs held at once). The values are means (total over count), so they compare
/// to the per-statement `mean`. Fields are `Option` because the batch time and
/// batch memory benches run separately and either may be absent (and the
/// `libpg_query` bindings have batch time but no Rust-visible batch memory).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParserBatch {
    pub parser: String,
    /// Statements fed into the batch (the parser's accepted set).
    pub n_accepted: usize,
    /// Whole-script parse time divided by statement count (ns).
    #[serde(default)]
    pub ns_per_stmt: Option<f64>,
    /// Peak live bytes during the whole-script parse, per statement.
    #[serde(default)]
    pub peak_per_stmt: Option<f64>,
    /// Retained bytes after the whole-script parse, per statement.
    #[serde(default)]
    pub retained_per_stmt: Option<f64>,
}

/// Per-statement memory distribution for one parser in one dialect. Bytes,
/// measured by the `membench` allocator over the accepted statements.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParserMem {
    pub parser: String,
    /// Number of statements measured.
    pub n: usize,
    /// Peak live bytes during the parse (the working-set high-water mark).
    pub peak: MemDist,
    /// Bytes still live after the parse: the AST plus the scaffolding it retains.
    pub retained: MemDist,
}

/// A byte distribution: the same percentile set as [`ParserPerf`], in bytes,
/// plus a downsampled empirical CDF for charting.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MemDist {
    pub min: f64,
    pub p10: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
    /// Standard deviation of the sample (0 in older snapshots). Used for the
    /// time-machine trend's error band.
    #[serde(default)]
    pub std: f64,
    /// Downsampled empirical CDF: `[bytes, fraction]` points, ascending.
    #[serde(default)]
    pub ecdf: Vec<[f64; 2]>,
}

/// A preview of the statements one parser rejected in one dialect, plus the
/// info needed to offer the full set as a download. The full list is shipped
/// separately as a committed `.tsv.zst` file (see `download`); only a short
/// preview is embedded in the JSON to keep it small.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParserFailures {
    pub parser: String,
    /// Total statements this parser rejected that it was expected to accept.
    pub rejected_total: usize,
    /// Total statements the parser was expected to accept (the denominator), so
    /// the UI can show "N of M rejected".
    #[serde(default)]
    pub expected_total: usize,
    /// A handful of example rejected statements, pre-rendered to static
    /// syntax-highlighted HTML at export time so the viewer ships no runtime
    /// highlighter. Each entry is the inner HTML of one `<pre>` block.
    pub preview_html: Vec<String>,
    /// The parser's error message for each previewed statement, aligned with
    /// `preview_html` (same index). Plain text, escaped by the viewer at render.
    #[serde(default)]
    pub preview_reasons: Vec<String>,
    /// Path (relative to the site root) of the full `.tsv.zst` download, or
    /// `None` when there were no failures to ship.
    pub download: Option<String>,
}

/// Correctness metrics for one parser in one dialect. Percentages are
/// precomputed as `Option<f64>` so the viewer does pure formatting (None = N/A),
/// matching the CLI's semantics.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParserMetrics {
    pub parser: String,
    /// Benchmarked version of this parser (the time-machine point). Empty in
    /// older snapshots, where it is simply not shown.
    #[serde(default)]
    pub version: String,
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
    /// Standard deviation of the per-statement times (0 in older snapshots).
    /// Used for the time-machine trend's error band.
    #[serde(default)]
    pub std: f64,
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

/// Time-machine history for one parser family (e.g. all benchmarked sqlparser-rs
/// versions). Shipped as a per-family file fetched on demand by the parser page,
/// so it stays off the initial load.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FamilyHistory {
    /// Display name of the family (matches `ParserPerf::parser` / the page name).
    pub family: String,
    /// One run per benchmarked version, oldest first.
    pub versions: Vec<VersionRun>,
}

/// One benchmarked version of a family, across the dialects it models.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VersionRun {
    pub version: String,
    /// ISO release date, used to order and place points on the trend x-axis.
    pub released: String,
    /// One entry per dialect this version models, in display order.
    pub dialects: Vec<DialectRun>,
}

/// One version's results in one dialect. The same per-parser shapes as the main
/// snapshot, so the parser page can render a selected version with the existing
/// charts and tables. Any axis not measured is `None`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DialectRun {
    pub dir_name: String,
    pub display_name: String,
    pub has_reference: bool,
    #[serde(default)]
    pub perf: Option<ParserPerf>,
    #[serde(default)]
    pub memory: Option<ParserMem>,
    #[serde(default)]
    pub batch: Option<ParserBatch>,
    #[serde(default)]
    pub correctness: Option<ParserMetrics>,
}

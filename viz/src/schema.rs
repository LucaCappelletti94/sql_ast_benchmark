//! The committed-snapshot JSON schema, shared by `sqlbench export` (serialize)
//! and the `web` viewer (deserialize).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Static source-feature scan of one parser's library `src/` (panic-inducing
/// constructs, unsafe usage, lint policy, dependency footprint). Produced by the
/// `featurescan` crate and baked into the web metadata. See that crate for the
/// counting rules and caveats (counts are a code-smell proxy, not a crash proof).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FeatureScan {
    pub note: String,
    pub parsers: Vec<ParserFeatures>,
}

/// One parser's static-scan results.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParserFeatures {
    /// Display name, matching the parser-page name.
    pub parser: String,
    pub package: String,
    pub version: String,
    pub counts: FeatureCounts,
    pub lints: LintPolicy,
    /// The crate sets `forbid(unsafe_code)`.
    pub forbids_unsafe: bool,
    /// Direct, non-dev, non-build dependencies.
    pub direct_deps: usize,
    /// The crate depends on serde (AST serialization is plausible).
    pub serde_dep: bool,
}

/// Library-source construct counts (panic families, unsafe, LOC). All counts
/// exclude tests, benches, examples, `#[cfg(test)]` items, and test-helper files.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct FeatureCounts {
    pub loc: usize,
    pub test_lines: usize,
    /// Non-test lines (`loc - test_lines`), the per-KLOC density denominator.
    pub code_loc: usize,
    pub files: usize,
    pub parse_failures: usize,
    pub panic: usize,
    pub unreachable: usize,
    pub unimplemented: usize,
    pub todo: usize,
    pub assert: usize,
    pub unwrap: usize,
    pub expect: usize,
    pub unwrap_unchecked: usize,
    pub index: usize,
    pub unsafe_blocks: usize,
    pub unsafe_fns: usize,
    pub unsafe_impls: usize,
    pub serde_derive: bool,
}

impl FeatureCounts {
    /// Hard, unconditional panics: `panic!`, `unreachable!`, `unimplemented!`, `todo!`.
    #[must_use]
    pub const fn hard_panics(&self) -> usize {
        self.panic + self.unreachable + self.unimplemented + self.todo
    }

    /// Total unsafe occurrences (blocks, functions, impls).
    #[must_use]
    pub const fn unsafe_total(&self) -> usize {
        self.unsafe_blocks + self.unsafe_fns + self.unsafe_impls
    }
}

/// A parser's own panic-relevant lint policy: which lints it bans by design.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LintPolicy {
    /// Lint bare name (e.g. `unwrap_used`) -> level (`forbid`/`deny`/`warn`/`allow`).
    pub lints: BTreeMap<String, String>,
    /// The crate inherits `[lints]` from a workspace not resolvable from the
    /// published package, so any policy there is invisible to the scan.
    pub workspace_inherited: bool,
}

impl LintPolicy {
    /// True if the lint is set to `deny` or `forbid` (a build-failing ban).
    #[must_use]
    pub fn is_banned(&self, lint: &str) -> bool {
        matches!(
            self.lints.get(lint).map(String::as_str),
            Some("deny" | "forbid")
        )
    }
}

/// Recursion-depth probe results across parsers. Produced by `featurescan-depth`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DepthScan {
    pub note: String,
    pub stack_bytes: usize,
    pub ceil: usize,
    pub parsers: Vec<DepthReport>,
}

/// One parser's recursion-depth result.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DepthReport {
    pub parser: String,
    pub dialect: String,
    /// Rejects deep input cleanly and never overflows up to the ceiling.
    pub guarded: bool,
    /// The parser does not accept the probe shape even at depth 1, so its graceful
    /// limit cannot be read from this shape (the crash depth is still valid).
    pub shape_rejected: bool,
    /// Smallest depth rejected instead of accepted (graceful recursion limit).
    pub limit_depth: Option<usize>,
    /// Smallest depth that overflows the stack (None = never, up to the ceiling).
    pub crash_depth: Option<usize>,
    pub ceil: usize,
}

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
/// parsed in one call, and the cost is divided by the statement count. This
/// complements the per-statement [`ParserPerf`]/[`ParserMem`], exposing the
/// amortization a batch API gains or loses (a grown `Vec` of statements, all
/// ASTs held at once). The values are means (total over count), so they compare
/// to the per-statement `mean`. Fields are `Option` because the batch time and
/// batch memory benches run separately and either may be absent (and the
/// `libpg_query` bindings have batch time but no Rust-visible batch memory).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParserBatch {
    pub parser: String,
    /// Statements eligible for batching (accepted, parse to one statement alone,
    /// and not input-consuming), the pool the random batches were drawn from.
    pub n_accepted: usize,
    /// Batch accuracy: the share of sampled multi-statement scripts the parser
    /// reparsed to exactly the expected statement count, as a percent. Lower than
    /// 100 means the parser mishandles a statement boundary in some scripts.
    #[serde(default)]
    pub accuracy_pct: Option<f64>,
    /// Per-statement parse time averaged over the batches that parsed correctly
    /// (ns). `None` when no batch parsed correctly.
    #[serde(default)]
    pub ns_per_stmt: Option<f64>,
    /// Peak live bytes per statement over the correctly parsed batches.
    #[serde(default)]
    pub peak_per_stmt: Option<f64>,
    /// Retained bytes per statement over the correctly parsed batches.
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
/// separately as a committed `.tsv.zst` file (see `download`). Only a short
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
    /// Provenance dialects: fraction of the corpus accepted.
    pub accept_pct: Option<f64>,
    /// Statements the parser attempted in this dialect (the panic-rate
    /// denominator), so per-parser rates can be aggregated across dialects. 0 in
    /// older snapshots.
    #[serde(default)]
    pub attempted: usize,
    /// Statements on which the parser threw a caught panic instead of returning a
    /// result (0 in older snapshots, and in historical time-machine versions whose
    /// panic rate is not measured).
    #[serde(default)]
    pub panicked: usize,
    /// Empirical panic rate: panics as a fraction of statements attempted in this
    /// dialect. `None` when nothing was attempted or the value is unmeasured.
    #[serde(default)]
    pub panic_pct: Option<f64>,
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

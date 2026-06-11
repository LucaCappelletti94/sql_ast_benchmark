//! The committed results snapshot, embedded at compile time and parsed once.
//!
//! Both the main bundle and the time-machine history are zstd-compressed and
//! embedded via `include_bytes!`, then decompressed in wasm with `ruzstd`.
//! Embedding (rather than a runtime fetch) keeps the viewer immune to GitHub
//! Pages base-path fetch pitfalls. Compressing keeps the wasm payload small
//! (the bundle is ~25x smaller compressed).

use std::sync::OnceLock;
use viz::{Bundle, DepthReport, DepthScan, FamilyHistory, FeatureScan, ParserFeatures};

/// The results bundle, zstd-compressed and embedded.
static BUNDLE_RAW: &[u8] = include_bytes!("../assets/bench.json.zst");

/// Combined time-machine history for every family, zstd-compressed and embedded.
static HISTORY_RAW: &[u8] = include_bytes!("../assets/history.json.zst");

/// Static source-feature scan (panic discipline, unsafe, lints, deps). Small and
/// committed uncompressed, so embedded as a string and parsed once.
static FEATURESCAN_RAW: &str = include_str!("../../featurescan/data/featurescan.json");

/// Recursion-depth probe results, committed uncompressed.
static DEPTH_RAW: &str = include_str!("../../featurescan/data/depth.json");

/// Decompress an embedded zstd blob to bytes.
fn unzstd(raw: &[u8]) -> Vec<u8> {
    let mut decoder = ruzstd::StreamingDecoder::new(raw).expect("embedded blob is valid zstd");
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut buf).expect("decompress embedded blob");
    buf
}

/// The parsed results bundle (decompressed and parsed once, then cached).
pub fn bundle() -> &'static Bundle {
    static CACHE: OnceLock<Bundle> = OnceLock::new();
    CACHE.get_or_init(|| {
        serde_json::from_slice(&unzstd(BUNDLE_RAW)).expect("web/assets/bench.json.zst is valid")
    })
}

/// All per-family time-machine histories (decompressed and parsed once).
fn histories() -> &'static [FamilyHistory] {
    static CACHE: OnceLock<Vec<FamilyHistory>> = OnceLock::new();
    CACHE.get_or_init(|| {
        serde_json::from_slice(&unzstd(HISTORY_RAW)).expect("history json is valid")
    })
}

/// The version history for one parser family, if the time-machine covers it.
#[must_use]
pub fn history(family: &str) -> Option<&'static FamilyHistory> {
    histories().iter().find(|h| h.family == family)
}

/// The static source-feature scan (parsed once).
fn featurescan() -> &'static FeatureScan {
    static CACHE: OnceLock<FeatureScan> = OnceLock::new();
    CACHE.get_or_init(|| serde_json::from_str(FEATURESCAN_RAW).expect("featurescan.json is valid"))
}

/// The static-scan features for one parser, by display name.
#[must_use]
pub fn parser_features(parser: &str) -> Option<&'static ParserFeatures> {
    featurescan().parsers.iter().find(|p| p.parser == parser)
}

/// The recursion-depth probe results (parsed once).
fn depth_scan() -> &'static DepthScan {
    static CACHE: OnceLock<DepthScan> = OnceLock::new();
    CACHE.get_or_init(|| serde_json::from_str(DEPTH_RAW).expect("depth.json is valid"))
}

/// The recursion-depth result for one parser, by display name.
#[must_use]
pub fn parser_depth(parser: &str) -> Option<&'static DepthReport> {
    depth_scan().parsers.iter().find(|p| p.parser == parser)
}

/// Aggregate empirical panic totals for one parser across every dialect it runs:
/// `(panicked, attempted)`. The per-parser panic rate is `panicked / attempted`.
/// Returns `None` if nothing was attempted (e.g. an older snapshot without the
/// panic fields), so the caller can show the badge as unmeasured rather than 0.
#[must_use]
pub fn panic_totals(parser: &str) -> Option<(usize, usize)> {
    let (mut panicked, mut attempted) = (0usize, 0usize);
    for dialect in &bundle().dialects {
        if let Some(m) = dialect.correctness.iter().find(|m| m.parser == parser) {
            panicked += m.panicked;
            attempted += m.attempted;
        }
    }
    (attempted > 0).then_some((panicked, attempted))
}

/// Aggregate failed-to-parse totals for one parser across every dialect:
/// `(rejected, expected)`. A statement counts as failed when the parser was
/// expected to accept it (reference-valid statements in reference dialects, every
/// statement in provenance dialects) but rejected it. This is the same set the
/// per-parser failures download lists. Returns `None` when nothing was expected
/// (e.g. an older snapshot without the failures section), so the caller can show
/// the badge as unmeasured rather than 0.
#[must_use]
pub fn failure_totals(parser: &str) -> Option<(usize, usize)> {
    let (mut rejected, mut expected) = (0usize, 0usize);
    for dialect in &bundle().dialects {
        if let Some(f) = dialect.failures.iter().find(|f| f.parser == parser) {
            rejected += f.rejected_total;
            expected += f.expected_total;
        }
    }
    (expected > 0).then_some((rejected, expected))
}

#[cfg(test)]
mod tests {
    /// The committed snapshot must decompress and deserialize into the shared
    /// schema. This fails the build if the snapshot and the `viz` schema drift.
    #[test]
    fn committed_snapshot_parses() {
        let b = super::bundle();
        assert!(!b.dialects.is_empty());
        assert!(!b.parsers.is_empty());
    }

    /// The committed feature-scan and depth snapshots must parse into the shared
    /// schema, failing the build if `featurescan` output and `viz` drift.
    #[test]
    fn committed_featurescan_and_depth_parse() {
        assert!(!super::featurescan().parsers.is_empty());
        assert!(!super::depth_scan().parsers.is_empty());
        // sqlparser-rs is covered by both scans.
        assert!(super::parser_features("sqlparser-rs").is_some());
        assert!(super::parser_depth("sqlparser-rs").is_some());
    }
}

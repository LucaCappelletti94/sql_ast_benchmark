//! The committed results snapshot, embedded at compile time and parsed once.
//!
//! Both the main bundle and the time-machine history are zstd-compressed and
//! embedded via `include_bytes!`, then decompressed in wasm with `ruzstd`.
//! Embedding (rather than a runtime fetch) keeps the viewer immune to GitHub
//! Pages base-path fetch pitfalls; compressing keeps the wasm payload small
//! (the bundle is ~25x smaller compressed).

use std::sync::OnceLock;
use viz::{Bundle, FamilyHistory};

/// The results bundle, zstd-compressed and embedded.
static BUNDLE_RAW: &[u8] = include_bytes!("../assets/bench.json.zst");

/// Combined time-machine history for every family, zstd-compressed and embedded.
static HISTORY_RAW: &[u8] = include_bytes!("../assets/history.json.zst");

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
}

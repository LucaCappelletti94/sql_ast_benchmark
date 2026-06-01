//! The committed results snapshot, embedded at compile time and parsed once.
//!
//! Embedding via `include_str!` (rather than a runtime fetch) means the viewer
//! needs no network request and is immune to GitHub Pages base-path fetch
//! pitfalls.

use std::sync::OnceLock;
use viz::Bundle;

static RAW: &str = include_str!("../assets/bench.json");

/// The parsed results bundle (parsed once, then cached).
pub fn bundle() -> &'static Bundle {
    static CACHE: OnceLock<Bundle> = OnceLock::new();
    CACHE.get_or_init(|| serde_json::from_str(RAW).expect("web/assets/bench.json is valid"))
}

#[cfg(test)]
mod tests {
    /// The committed snapshot must deserialize into the shared schema. This
    /// fails the build if `bench.json` and the `viz` schema drift apart.
    #[test]
    fn committed_snapshot_parses() {
        let b = super::bundle();
        assert!(!b.dialects.is_empty());
        assert!(!b.parsers.is_empty());
    }
}

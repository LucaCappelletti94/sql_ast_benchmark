//! Reads the committed real-engine validity cache produced by the `oracle`
//! crate.
//!
//! Each reference dialect has a file `oracle/labels/{dir}.tsv.zst`. Its first
//! line is the corpus statement count (for drift detection) and the remaining
//! lines are `hash\t0|1` where the hash is [`statement_hash`] and `1` means the
//! real database engine parsed the statement (valid). The cache is loaded once
//! and shared. Dialects without a file are not reference-graded.

use crate::datasets::Dialect;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Directory holding the committed per-dialect validity caches.
pub const LABELS_DIR: &str = "oracle/labels";

/// Stable 64-bit FNV-1a hash of a statement, used by both the `oracle` producer
/// and this reader so keys line up regardless of std hashing changes.
#[must_use]
pub fn statement_hash(sql: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in sql.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

type Labels = HashMap<&'static str, HashMap<u64, bool>>;

fn labels() -> &'static Labels {
    static CACHE: OnceLock<Labels> = OnceLock::new();
    CACHE.get_or_init(load_all)
}

fn load_all() -> Labels {
    let mut out = Labels::new();
    for d in Dialect::ALL {
        if let Some(map) = load_dialect(d) {
            out.insert(d.dir_name(), map);
        }
    }
    out
}

fn load_dialect(d: Dialect) -> Option<HashMap<u64, bool>> {
    let path = format!("{LABELS_DIR}/{}.tsv.zst", d.dir_name());
    let bytes = std::fs::read(&path).ok()?;
    let text = zstd::decode_all(&bytes[..]).ok()?;
    let text = String::from_utf8(text).ok()?;
    let mut map = HashMap::new();
    // Skip the first line (corpus count header).
    for line in text.lines().skip(1) {
        let mut it = line.split('\t');
        if let (Some(h), Some(b)) = (it.next(), it.next()) {
            if let Ok(h) = h.parse::<u64>() {
                map.insert(h, b == "1");
            }
        }
    }
    Some(map)
}

/// Whether `d` has a committed real-engine reference cache.
#[must_use]
pub fn has_reference(d: Dialect) -> bool {
    labels().contains_key(d.dir_name())
}

/// The real engine's verdict for `sql` in `d`.
///
/// `Some(true)` if the engine parsed it, `Some(false)` if it was a syntax error,
/// `None` if `d` has no cache or the statement is absent from it (a coverage
/// miss the caller should skip).
#[must_use]
pub fn reference_accepts(sql: &str, d: Dialect) -> Option<bool> {
    labels()
        .get(d.dir_name())?
        .get(&statement_hash(sql))
        .copied()
}

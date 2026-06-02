//! Process-wide allocation counters for the memory benchmark.
//!
//! The counters here are plain, safe atomics. The actual `GlobalAlloc` (which
//! needs `unsafe`, forbidden in this crate) lives in the separate `membench`
//! crate and feeds these via [`record_alloc`] / [`record_dealloc`]. Under any
//! other binary the counters simply stay at zero, so [`crate::BenchParser::measure_mem`]
//! reports nothing.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Live bytes: total requested minus total released.
static LIVE: AtomicUsize = AtomicUsize::new(0);
/// High-water mark of [`LIVE`] since the last [`reset_peak`].
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Record `size` bytes just allocated. Called from the `membench` allocator.
pub fn record_alloc(size: usize) {
    let now = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

/// Record `size` bytes just freed. Called from the `membench` allocator.
pub fn record_dealloc(size: usize) {
    LIVE.fetch_sub(size, Ordering::Relaxed);
}

/// Current live bytes (allocated minus freed).
#[must_use]
pub fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

/// Peak live bytes recorded since the last [`reset_peak`].
#[must_use]
pub fn peak() -> usize {
    PEAK.load(Ordering::Relaxed)
}

/// Drop the peak high-water mark back to the current live total, so the next
/// window measures only its own allocations.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

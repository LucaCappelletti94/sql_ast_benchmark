//! Process-wide allocation counters for the memory benchmark.
//!
//! [`CountingAlloc`] wraps the system allocator and feeds the counters. The
//! `membench` and `timemachine-mem` binaries install it as their global
//! allocator. Under any other binary the counters stay at zero, so every
//! [`Window`] reports zeros.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// System allocator that records every call into the counters of this module.
/// Install it with `#[global_allocator]` in a binary that measures memory.
pub struct CountingAlloc;

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged, so `System`'s guarantees on returned blocks carry over, and the
// added bookkeeping only touches atomics and never allocates or reenters.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller guarantees `layout` has a nonzero size, as
        // `GlobalAlloc::alloc` requires.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller guarantees `ptr` came from this allocator with
        // `layout`, and this allocator hands out only `System` blocks.
        unsafe { System.dealloc(ptr, layout) };
        record_dealloc(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller guarantees `ptr` came from this allocator with
        // `layout` (so from `System`), and that `new_size` is nonzero and does
        // not overflow `isize` once rounded to `layout.align()`.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            record_realloc(layout.size(), new_size);
        }
        new_ptr
    }
}

/// Live bytes: total requested minus total released.
static LIVE: AtomicUsize = AtomicUsize::new(0);
/// High-water mark of [`LIVE`] since the last [`reset_peak`].
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// Allocator calls that hand out memory (allocations and reallocations).
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
/// Bytes requested by those calls, each reallocation counting its new size.
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

/// Record `size` bytes just allocated.
fn record_alloc(size: usize) {
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    ALLOCATED.fetch_add(size, Ordering::Relaxed);
    let now = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

/// Record a block resized from `old` to `new` bytes. Counted as one allocator
/// call requesting `new` bytes, the way a heap profiler sees a reallocation.
fn record_realloc(old: usize, new: usize) {
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    ALLOCATED.fetch_add(new, Ordering::Relaxed);
    if new >= old {
        let grown = new - old;
        let now = LIVE.fetch_add(grown, Ordering::Relaxed) + grown;
        PEAK.fetch_max(now, Ordering::Relaxed);
    } else {
        LIVE.fetch_sub(old - new, Ordering::Relaxed);
    }
}

/// Record `size` bytes just freed.
fn record_dealloc(size: usize) {
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

/// What one measured parse cost the allocator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemSample {
    /// High-water mark of live bytes above the window's starting total.
    pub peak: usize,
    /// Bytes still live when the window closed.
    pub retained: usize,
    /// Allocator calls that handed out memory.
    pub allocs: usize,
    /// Bytes those calls requested, freed or not.
    pub allocated: usize,
}

/// A measurement window over the process-wide counters. Open it right before
/// the parse and [`finish`](Self::finish) it while the result is still alive,
/// so the retained bytes include the produced AST.
pub struct Window {
    live: usize,
    allocs: usize,
    allocated: usize,
}

impl Window {
    /// Snapshot the counters and reset the peak to the current live total.
    #[must_use]
    pub fn start() -> Self {
        let live = live();
        reset_peak();
        Self {
            live,
            allocs: ALLOCS.load(Ordering::Relaxed),
            allocated: ALLOCATED.load(Ordering::Relaxed),
        }
    }

    /// Everything allocated since [`start`](Self::start).
    #[must_use]
    pub fn finish(&self) -> MemSample {
        MemSample {
            peak: peak().saturating_sub(self.live),
            retained: live().saturating_sub(self.live),
            allocs: ALLOCS.load(Ordering::Relaxed).saturating_sub(self.allocs),
            allocated: ALLOCATED
                .load(Ordering::Relaxed)
                .saturating_sub(self.allocated),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The counters are process-wide and the test harness is multi-threaded, so
    // one test drives every transition to keep other tests' allocations out
    // (nothing in this test binary installs the counting allocator).
    #[test]
    fn window_tracks_peak_retained_and_pressure() {
        let w = Window::start();
        record_alloc(100);
        record_alloc(50);
        record_dealloc(100);
        record_realloc(50, 200);
        record_realloc(200, 20);
        let s = w.finish();
        assert_eq!(
            s,
            MemSample {
                peak: 200,
                retained: 20,
                allocs: 4,
                allocated: 100 + 50 + 200 + 20,
            }
        );
        record_dealloc(20);
    }
}

//! Time-machine memory runner.
//!
//! Installs a counting global allocator (like `membench`) and measures peak and
//! retained bytes per accepted statement for every registered version, writing a
//! per-family memory sidecar under `target/timemachine/` that the timing runner
//! merges into the final history. Single-threaded by design (the counters are
//! process-wide).

use std::alloc::{GlobalAlloc, Layout, System};

use sql_ast_benchmark::report::WORKER_STACK;

/// System allocator that records each allocation into `sql_ast_benchmark::mem`.
struct Counting;

// SAFETY: a thin pass-through to the system allocator that only adds atomic
// bookkeeping (no allocation of its own) around each call.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            sql_ast_benchmark::mem::record_alloc(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        sql_ast_benchmark::mem::record_dealloc(layout.size());
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                sql_ast_benchmark::mem::record_alloc(new_size - layout.size());
            } else {
                sql_ast_benchmark::mem::record_dealloc(layout.size() - new_size);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    if let Err(e) = sql_ast_benchmark::datasets::ensure_corpus() {
        eprintln!("ERROR: could not prepare datasets/: {e}");
        std::process::exit(1);
    }
    let args: Vec<String> = std::env::args().collect();
    let refresh = timemachine::run::parse_refresh(&args);
    let full = args.iter().any(|a| a == "--full");
    if refresh.is_none() && !full {
        eprintln!(
            "(smoke run: first {} statements per dialect; pass --full for the whole corpus)",
            timemachine::run::SMOKE_LIMIT
        );
    }
    std::thread::Builder::new()
        .stack_size(WORKER_STACK)
        .spawn(move || {
            let versions = timemachine::registry::all();
            if let Some((family, vers)) = refresh {
                eprintln!("refreshing memory for {family} versions {vers:?}");
                timemachine::run::run_memory_refresh(&versions, &family, &vers);
                eprintln!("memory refreshed for {family}");
            } else {
                timemachine::run::run_memory(&versions, full);
            }
        })
        .expect("spawn worker")
        .join()
        .expect("memory thread panicked");
}

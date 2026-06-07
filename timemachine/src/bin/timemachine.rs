//! Time-machine timing + batch + correctness runner.
//!
//! For every registered version, times each accepted statement, the whole-script
//! batch, and grades correctness, then merges the memory sidecar (written by
//! `timemachine-mem`) and writes the final `web/static/history/<family>.json`.
//!
//! Run after `timemachine-mem` so the memory column is filled:
//!   cargo run --release -p timemachine --bin timemachine-mem -- --full
//!   cargo run --release -p timemachine --bin timemachine -- --full
//!
//! Without `--full` only the first statements per dialect are used (a fast smoke
//! check that the pipeline produces a valid history file).

use sql_ast_benchmark::report::WORKER_STACK;

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    if let Err(e) = sql_ast_benchmark::datasets::ensure_corpus() {
        eprintln!("ERROR: could not prepare datasets/: {e}");
        std::process::exit(1);
    }
    let full = std::env::args().any(|a| a == "--full");
    if !full {
        eprintln!(
            "(smoke run: first {} statements per dialect; pass --full for the whole corpus)",
            timemachine::run::SMOKE_LIMIT
        );
    }
    // Deeply nested SQL can overflow recursive-descent parsers, so run on a large
    // stack (single-threaded keeps the timing clean).
    std::thread::Builder::new()
        .stack_size(WORKER_STACK)
        .spawn(move || {
            let versions = timemachine::registry::all();
            let written = timemachine::run::run_timing(&versions, full);
            eprintln!("history written for: {written:?}");
        })
        .expect("spawn worker")
        .join()
        .expect("timing thread panicked");
}

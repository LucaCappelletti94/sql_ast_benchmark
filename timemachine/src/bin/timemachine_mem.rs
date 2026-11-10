//! Time-machine memory runner.
//!
//! Installs the counting global allocator (like `membench`) and measures peak,
//! retained, and allocation pressure per accepted statement for every registered
//! version, writing a per-family memory sidecar under `target/timemachine/` that
//! the timing runner merges into the final history. Single-threaded by design
//! (the counters are process-wide).

use sql_ast_benchmark::mem::CountingAlloc;
use sql_ast_benchmark::report::WORKER_STACK;

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

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

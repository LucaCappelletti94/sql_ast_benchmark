# Contributing

See the [README](README.md) for what the benchmark measures, how to run it, and the build prerequisites.

## Development

```bash
git config core.hooksPath .githooks   # enable fmt + clippy pre-commit hooks
cargo fmt --all
cargo clippy --all-targets
```

No unsafe code is allowed (`unsafe_code = "forbid"`). Clippy runs with pedantic and nursery lints enabled. The workspace has three crates: the native benchmark (root), `viz` (wasm-clean shared schema + plotters chart renderers), and `web` (the Dioxus viewer). `default-members` keeps bare `cargo test`/`bench`/`clippy` on the native crate only.

## Results website

The site under `web/` is a Dioxus -> WASM app that renders a committed snapshot, `web/assets/bench.json`, produced by `sqlbench export`. CI (`.github/workflows/pages.yml`) only builds and deploys the committed crates, so regenerate the snapshot manually after changing the corpus or parsers:

```bash
cargo regen          # one command: timing benches + memory benches + export (long)
cd web && dx serve   # preview at http://127.0.0.1:8080/sql_ast_benchmark/
```

`cargo regen` (alias in `.cargo/config.toml` for `cargo run --release --bin sqlbench -- regen`) runs the producers in order and ends with the export. The memory benches install a counting global allocator, so they each run in their own process, separate from the timing bench and from export. That is the only reason this is a pipeline rather than a single binary. To run a stage on its own:

```bash
cargo bench                              # write target/bench_dist/ + target/batch_dist/ timings
cargo run --release -p membench          # write target/mem_dist/ per-statement memory
cargo run --release -p membench -- batch # write target/batch_mem_dist/ whole-script memory
cargo run --bin sqlbench -- export       # read all of the above, write web/assets/bench.json
```

`export` reads whatever timing, memory, and batch summaries are present under `target/` and warns (rather than fails) for any that are missing, so the memory and batch columns stay empty until their producers have been run.

The charts are rendered in the browser from the JSON by the shared `viz` crate (plotters, SVG backend), so no chart images are committed.

## Coverage

```bash
tar --zstd -xf datasets.tar.zst   # coverage runs the bench in smoke mode, which needs datasets/ present
cargo tarpaulin                    # LLVM engine, includes the bench
```

`tarpaulin.toml` runs the benchmark in verify-only mode (`--test`) under the LLVM engine, since the benchmark is the main exercise of the `BenchParser` layer. With the corpus present it covers `benches/parsing.rs` and `benches/batch_parsing.rs` (both in smoke mode) and the dialect-mapping / accept / reprint paths in `src/lib.rs`.

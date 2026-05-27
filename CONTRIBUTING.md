# Contributing

See the [README](README.md) for what the benchmark measures, how to run it, and the build prerequisites.

## Development

```bash
git config core.hooksPath .githooks   # enable fmt + clippy pre-commit hooks
cargo fmt --all
cargo clippy --all-targets
```

No unsafe code is allowed (`unsafe_code = "forbid"`). Clippy runs with pedantic and nursery lints enabled.

## Coverage

```bash
tar --zstd -xf datasets.tar.zst   # the bench needs datasets/ present
cargo tarpaulin                    # LLVM engine, includes the bench
```

`tarpaulin.toml` runs the benchmark in verify-only mode (`--test`) under the LLVM engine, since the benchmark is the main exercise of the `BenchParser` layer. With the corpus present it covers `benches/parsing.rs` and the dialect-mapping / accept / reprint paths in `src/lib.rs`.

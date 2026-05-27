# Contributing

Thanks for helping improve the SQL parser benchmark. See the [README](README.md) for what the benchmark measures and how to run it, and the build prerequisites under its Environment section.

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

`tarpaulin.toml` uses the LLVM engine and runs the benchmark in verify-only mode (`--test`), since it is the main exercise of the multi-dialect `BenchParser` layer. With the corpus present this covers `benches/parsing.rs` and the dialect-mapping / accept / reprint paths in `src/lib.rs`.

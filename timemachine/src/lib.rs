//! Time-machine: benchmark several historical versions of each pure-Rust parser.
//!
//! Each version is a `package`-renamed crate (see `Cargo.toml`) wrapped in a
//! [`sql_ast_benchmark::Parser`] impl, so the same grading, timing, and memory
//! drivers in the main crate serve the whole history. The runner binaries
//! (`timemachine`, `timemachine-mem`) produce per-family history under
//! `target/timemachine/`, which `sqlbench export` turns into the per-family
//! files the explorer fetches.

pub mod families {
    pub mod databend;
    pub mod orql;
    pub mod polyglot;
    pub mod qusql;
    pub mod sqlglot;
    pub mod sqlite3;
    pub mod sqlparser;
    pub mod turso;
}
pub mod registry;
pub mod run;

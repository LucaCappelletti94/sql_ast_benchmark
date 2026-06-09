//! Static source-feature scan of the benchmarked SQL parsers.
//!
//! Locates each parser's source on disk via `cargo metadata`, scans its library
//! `src/` for panic-inducing constructs and unsafe usage, reads its lint policy,
//! and writes a committed snapshot at `featurescan/data/featurescan.json` that the
//! web metadata bakes in. Run with `cargo run -p featurescan`.

mod lints;
mod scan;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;
use viz::{FeatureScan, ParserFeatures};

/// (display name as used by the web metadata, crate package name).
const PARSERS: &[(&str, &str)] = &[
    ("sqlparser-rs", "sqlparser"),
    ("pg_query.rs", "pg_query"),
    // The summary mode is the same libpg_query crate, so it shares the scan.
    ("pg_query (summary)", "pg_query"),
    ("qusql-parse", "qusql-parse"),
    ("polyglot-sql", "polyglot-sql"),
    ("databend-common-ast", "databend-common-ast"),
    ("sqlglot-rust", "sqlglot-rust"),
    ("sqlite3-parser", "sqlite3-parser"),
    ("turso_parser", "turso_parser"),
    ("orql", "orql"),
];

fn main() {
    let metadata = MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let by_name: BTreeMap<&str, &cargo_metadata::Package> = metadata
        .packages
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();

    let mut reports = Vec::new();
    for (display, package_name) in PARSERS {
        let Some(pkg) = by_name.get(package_name) else {
            eprintln!("warning: package `{package_name}` not in cargo metadata, skipping");
            continue;
        };
        let manifest_path: &Path = pkg.manifest_path.as_std_path();
        let src_dir = manifest_path
            .parent()
            .map(|p| p.join("src"))
            .unwrap_or_else(|| PathBuf::from("src"));

        let counts = scan::scan_src(&src_dir);
        let lint_info = lints::collect(manifest_path, &src_dir);
        let forbids_unsafe = lint_info.is_banned("unsafe_code");

        let direct_deps = pkg
            .dependencies
            .iter()
            .filter(|d| d.kind == cargo_metadata::DependencyKind::Normal)
            .count();
        let serde_dep = pkg
            .dependencies
            .iter()
            .any(|d| d.name == "serde" || d.name == "serde_derive");

        eprintln!(
            "{display:22} v{} : {} files ({} unparsed), unwrap={} expect={} panic={} unreachable={} todo={} unsafe={}",
            pkg.version,
            counts.files,
            counts.parse_failures,
            counts.unwrap,
            counts.expect,
            counts.panic,
            counts.unreachable,
            counts.todo,
            counts.unsafe_blocks + counts.unsafe_fns + counts.unsafe_impls,
        );

        reports.push(ParserFeatures {
            parser: (*display).to_string(),
            package: (*package_name).to_string(),
            version: pkg.version.to_string(),
            counts,
            lints: lint_info,
            forbids_unsafe,
            direct_deps,
            serde_dep,
        });
    }

    let snapshot = FeatureScan {
        note: "Static source scan of each parser's library src/ (panic families, \
               unsafe, lint policy). Counts exclude tests/benches/examples, \
               #[cfg(test)] items, and test-helper files (e.g. test_utils.rs). \
               Macro-body unwraps are not counted (opaque to syn). Counts are a \
               code-smell proxy, not a crash proof. Regenerate with `cargo run -p \
               featurescan`."
            .to_string(),
        parsers: reports,
    };

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    std::fs::create_dir_all(&out_dir).expect("create data dir");
    let out_path = out_dir.join("featurescan.json");
    let json = serde_json::to_string_pretty(&snapshot).expect("serialize snapshot");
    std::fs::write(&out_path, json).expect("write snapshot");
    eprintln!("wrote {}", out_path.display());
}

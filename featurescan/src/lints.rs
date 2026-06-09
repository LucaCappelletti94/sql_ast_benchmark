//! Reading a crate's own lint policy: the panic-relevant lints it bans by design.
//!
//! Two sources are merged: the `[lints]` table in the crate's `Cargo.toml` and the
//! crate-root inner attributes (`#![deny(...)]` / `#![forbid(...)]`) in `lib.rs` or
//! `main.rs`. A lint set to `deny` or `forbid` is treated as "banned by design",
//! because then a regression fails the build (or `cargo clippy`).

use std::path::Path;

use quote::ToTokens;
use viz::LintPolicy;

/// The panic-relevant lints we report on, keyed by their bare name (the part after
/// `clippy::`, or the rust lint name for `unsafe_code`).
pub const RELEVANT: &[&str] = &[
    "unwrap_used",
    "expect_used",
    "panic",
    "unreachable",
    "todo",
    "unimplemented",
    "indexing_slicing",
    "arithmetic_side_effects",
    "integer_arithmetic",
    "panic_in_result_fn",
    "unsafe_code",
];

/// Collect lint policy from the crate manifest and its crate root.
pub fn collect(manifest_path: &Path, src_dir: &Path) -> LintPolicy {
    let mut info = LintPolicy::default();
    read_manifest_lints(manifest_path, &mut info);
    for root in ["lib.rs", "main.rs"] {
        read_root_attrs(&src_dir.join(root), &mut info);
    }
    info
}

fn record(info: &mut LintPolicy, name: &str, level: &str) {
    let bare = name.rsplit("::").next().unwrap_or(name);
    if RELEVANT.contains(&bare) {
        // Keep the strongest level seen (forbid > deny > warn > allow).
        let strength = |l: &str| match l {
            "forbid" => 3,
            "deny" => 2,
            "warn" => 1,
            _ => 0,
        };
        let entry = info.lints.entry(bare.to_string()).or_default();
        if strength(level) >= strength(entry) {
            *entry = level.to_string();
        }
    }
}

fn read_manifest_lints(manifest_path: &Path, info: &mut LintPolicy) {
    let Ok(text) = std::fs::read_to_string(manifest_path) else {
        return;
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return;
    };
    let Some(lints) = value.get("lints") else {
        return;
    };
    if lints.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
        info.workspace_inherited = true;
    }
    for group in ["clippy", "rust"] {
        let Some(table) = lints.get(group).and_then(toml::Value::as_table) else {
            continue;
        };
        for (name, spec) in table {
            let level = match spec {
                toml::Value::String(s) => Some(s.clone()),
                toml::Value::Table(t) => t
                    .get("level")
                    .and_then(toml::Value::as_str)
                    .map(str::to_string),
                _ => None,
            };
            if let Some(level) = level {
                record(info, name, &level);
            }
        }
    }
}

fn read_root_attrs(path: &Path, info: &mut LintPolicy) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(file) = syn::parse_file(&text) else {
        return;
    };
    for attr in &file.attrs {
        let level = if attr.path().is_ident("forbid") {
            "forbid"
        } else if attr.path().is_ident("deny") {
            "deny"
        } else if attr.path().is_ident("warn") {
            "warn"
        } else if attr.path().is_ident("allow") {
            "allow"
        } else {
            continue;
        };
        let _ = attr.parse_nested_meta(|meta| {
            let name = meta
                .path
                .to_token_stream()
                .to_string()
                .replace(char::is_whitespace, "");
            record(info, &name, level);
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strongest_level_wins() {
        let mut info = LintPolicy::default();
        record(&mut info, "clippy::unwrap_used", "warn");
        record(&mut info, "clippy::unwrap_used", "deny");
        record(&mut info, "clippy::unwrap_used", "warn");
        assert!(info.is_banned("unwrap_used"));
        assert_eq!(info.lints.get("unwrap_used").unwrap(), "deny");
    }

    #[test]
    fn ignores_irrelevant_lints() {
        let mut info = LintPolicy::default();
        record(&mut info, "clippy::needless_return", "deny");
        assert!(info.lints.is_empty());
    }
}

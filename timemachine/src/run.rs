//! Shared driving logic for the time-machine runners.
//!
//! Builds the per-family [`FamilyHistory`] for the registry's versions, reusing
//! the main crate's grading ([`report::grade_chunk`]) and summary helpers
//! ([`stats::perf_from`], [`stats::dist_from`]) so the history is computed the
//! same way as the current snapshot. Timing and memory run as separate binaries
//! (the memory one installs a global allocator), each producing part of the
//! history. The timing binary merges in the memory sidecar and writes the final
//! per-family file.

use sql_ast_benchmark::batch::join_batch;
use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::report::{self, load_dialect};
use sql_ast_benchmark::{stats, Parser};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;
use viz::{DialectRun, FamilyHistory, ParserBatch, ParserMem, ParserMetrics, VersionRun};

/// Dialects in display order (matches the rest of the benchmark).
pub const DIALECTS: &[Dialect] = &[
    Dialect::Postgresql,
    Dialect::Sqlite,
    Dialect::Mysql,
    Dialect::Clickhouse,
    Dialect::Duckdb,
    Dialect::Hive,
    Dialect::SparkSql,
    Dialect::Trino,
    Dialect::Tsql,
    Dialect::Oracle,
    Dialect::Bigquery,
    Dialect::Redshift,
    Dialect::Multi,
];

/// Statements per dialect used in the default (smoke) run, so a local check is
/// quick. The real run passes `--full` for the whole corpus.
pub const SMOKE_LIMIT: usize = 200;

/// Committed, zstd-compressed combined history embedded by the wasm viewer.
pub const HISTORY_FILE: &str = "web/assets/history.json.zst";

/// Sidecar directory for the memory runner's partial output.
pub const SIDECAR_DIR: &str = "target/timemachine";

fn pct(n: usize, base: usize) -> Option<f64> {
    (base != 0).then(|| 100.0 * n as f64 / base as f64)
}

/// Web-style slug for the per-family file name, matching the parser-page route
/// (lowercase, runs of non-alphanumerics collapsed to a single `_`, trimmed).
#[must_use]
pub fn family_slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_us = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_us = false;
        } else if !prev_us {
            out.push('_');
            prev_us = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// Per-statement time (ns): adaptive iteration count, best of a few rounds.
/// A local copy of the main bench's timer (the bench lives in `benches/`, not a
/// library, so it cannot be imported).
fn time_stmt(mut f: impl FnMut() -> bool) -> f64 {
    const TARGET_NS: u128 = 100_000;
    const ROUNDS: usize = 5;
    black_box(f());
    let probe = Instant::now();
    black_box(f());
    let single = probe.elapsed().as_nanos().max(1);
    let iters = u64::try_from((TARGET_NS / single).clamp(3, 1_000_000)).unwrap_or(3);
    let mut best = f64::MAX;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        for _ in 0..iters {
            black_box(f());
        }
        let per = start.elapsed().as_nanos() as f64 / iters as f64;
        best = best.min(per);
    }
    best
}

/// Whole-script parse time (ns): best of a few rounds.
fn time_batch(mut f: impl FnMut() -> usize) -> f64 {
    const TARGET_NS: u128 = 2_000_000;
    const ROUNDS: usize = 5;
    black_box(f());
    let probe = Instant::now();
    black_box(f());
    let single = probe.elapsed().as_nanos().max(1);
    let iters = u64::try_from((TARGET_NS / single).clamp(1, 1_000)).unwrap_or(1);
    let mut best = f64::MAX;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        for _ in 0..iters {
            black_box(f());
        }
        let per = start.elapsed().as_nanos() as f64 / iters as f64;
        best = best.min(per);
    }
    best
}

/// Load every dialect's corpus once, truncated to `limit` unless `full`.
fn load_corpus(full: bool) -> BTreeMap<&'static str, Vec<String>> {
    let mut map = BTreeMap::new();
    for &d in DIALECTS {
        let mut stmts = load_dialect(d);
        if !full && stmts.len() > SMOKE_LIMIT {
            stmts.truncate(SMOKE_LIMIT);
        }
        map.insert(d.dir_name(), stmts);
    }
    map
}

/// `ParserMetrics` from a one-parser grading report.
fn metrics_of(report: &report::DialectReport) -> ParserMetrics {
    let s = &report.stats[0];
    let id = report.parsers[0];
    let reference = report.has_reference;
    ParserMetrics {
        parser: id.family.to_string(),
        version: id.version.to_string(),
        accepted_valid: s.accepted_valid,
        accepted_invalid: s.accepted_invalid,
        recall_pct: if reference {
            pct(s.accepted_valid, report.valid_total)
        } else {
            None
        },
        false_positive_pct: if reference && report.invalid_total > 0 {
            pct(s.accepted_invalid, report.invalid_total)
        } else {
            None
        },
        roundtrip_pct: if s.can_reprint {
            pct(s.roundtrip_ok, s.accepted_valid)
        } else {
            None
        },
        accept_pct: if reference {
            None
        } else {
            pct(s.accepted_valid, report.valid_total)
        },
        // The time machine does not measure the empirical panic rate (only the
        // current build does, via BenchParser's panic-detecting parse_outcome), so
        // it is left unmeasured rather than reported as a misleading zero.
        attempted: s.attempted,
        panicked: 0,
        panic_pct: None,
    }
}

/// Build the timing + batch + correctness part of one version's run (no memory).
fn timing_dialect_run(p: &dyn Parser, d: Dialect, stmts: &[String]) -> DialectRun {
    let accepted: Vec<&str> = stmts
        .iter()
        .filter(|s| p.accepts(s, d) == Some(true))
        .map(String::as_str)
        .collect();

    let perf = if accepted.is_empty() {
        None
    } else {
        let mut times: Vec<f64> = accepted
            .iter()
            .map(|s| time_stmt(|| p.parse_once(s, d)))
            .collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let roundtrip_pct = if p.can_reprint(d) {
            let ok = accepted
                .iter()
                .filter(|s| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        p.roundtrips(s, d) == Some(true)
                    }))
                    .unwrap_or(false)
                })
                .count();
            Some(100.0 * ok as f64 / accepted.len() as f64)
        } else {
            None
        };
        Some(stats::perf_from(
            p.id().family.to_string(),
            stmts.len(),
            accepted.len(),
            roundtrip_pct,
            &times,
        ))
    };

    let batch = if accepted.is_empty() || !p.can_batch() {
        None
    } else {
        let script = join_batch(&accepted);
        let n_parsed = p.parse_batch(&script, d).unwrap_or(0);
        // Only trust the batch number if the whole accepted set parsed.
        if n_parsed >= accepted.len() {
            let ns = time_batch(|| p.parse_batch(&script, d).unwrap_or(0));
            Some(ParserBatch {
                parser: p.id().family.to_string(),
                n_accepted: accepted.len(),
                ns_per_stmt: Some(ns / accepted.len() as f64),
                peak_per_stmt: None,
                retained_per_stmt: None,
            })
        } else {
            None
        }
    };

    let report = report::grade_chunk(stmts, d, &[p]);
    let correctness = Some(metrics_of(&report));

    DialectRun {
        dir_name: d.dir_name().to_string(),
        display_name: d.display_name().to_string(),
        has_reference: report.has_reference,
        perf,
        memory: None,
        batch,
        correctness,
    }
}

/// Build the memory part of one version's run for one dialect (peak + retained).
fn mem_dialect_run(p: &dyn Parser, d: Dialect, stmts: &[String]) -> Option<ParserMem> {
    let accepted: Vec<&str> = stmts
        .iter()
        .filter(|s| p.accepts(s, d) == Some(true))
        .map(String::as_str)
        .collect();
    if accepted.is_empty() || p.measure_mem(accepted[0], d).is_none() {
        return None;
    }
    // Warm up one-time allocations so they do not land on the first statement.
    let _ = p.measure_mem(accepted[0], d);
    let mut peak = Vec::with_capacity(accepted.len());
    let mut retained = Vec::with_capacity(accepted.len());
    for s in &accepted {
        if let Some((pk, rt)) = p.measure_mem(s, d) {
            peak.push(pk as f64);
            retained.push(rt as f64);
        }
    }
    peak.sort_by(|a, b| a.partial_cmp(b).unwrap());
    retained.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(ParserMem {
        parser: p.id().family.to_string(),
        n: peak.len(),
        peak: stats::dist_from(&peak),
        retained: stats::dist_from(&retained),
    })
}

/// Group the registry's versions by family, preserving registry (release) order.
fn by_family(versions: &[Box<dyn Parser>]) -> Vec<(&'static str, Vec<&dyn Parser>)> {
    let mut out: Vec<(&'static str, Vec<&dyn Parser>)> = Vec::new();
    for v in versions {
        let f = v.id().family;
        if let Some(entry) = out.iter_mut().find(|(name, _)| *name == f) {
            entry.1.push(v.as_ref());
        } else {
            out.push((f, vec![v.as_ref()]));
        }
    }
    out
}

/// Run the timing + batch + correctness pass, merge the memory sidecar (if any),
/// and write the final per-family history files. Returns the families written.
pub fn run_timing(versions: &[Box<dyn Parser>], full: bool) -> Vec<String> {
    let corpus = load_corpus(full);
    let mut histories = Vec::new();
    let mut written = Vec::new();
    for (family, vs) in by_family(versions) {
        let sidecar = read_sidecar(family);
        let mut version_runs = Vec::new();
        for p in vs {
            let id = p.id();
            let mut dialects = Vec::new();
            for &d in DIALECTS {
                if !p.supports(d) {
                    continue;
                }
                let stmts = &corpus[d.dir_name()];
                if stmts.is_empty() {
                    continue;
                }
                // Isolate each (version, dialect): an old parser can panic on
                // the no-`catch_unwind` timing path, and one panic must not abort
                // the whole multi-hour run. Skip the pair and carry on.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    timing_dialect_run(p, d, stmts)
                }));
                let Ok(mut run) = outcome else {
                    eprintln!(
                        "  [warn] time {family} {} {} panicked, skipping",
                        id.version,
                        d.dir_name()
                    );
                    continue;
                };
                run.memory = sidecar_lookup(sidecar.as_ref(), id.version, d.dir_name());
                eprintln!(
                    "time {family} {} {}: n={}",
                    id.version,
                    d.dir_name(),
                    run.perf.as_ref().map_or(0, |x| x.n_accepted),
                );
                dialects.push(run);
            }
            version_runs.push(VersionRun {
                version: id.version.to_string(),
                released: id.released.to_string(),
                dialects,
            });
        }
        histories.push(FamilyHistory {
            family: family.to_string(),
            versions: version_runs,
        });
        written.push(family.to_string());
    }
    write_combined(&histories);
    written
}

/// Run the memory pass and write per-family memory sidecars.
///
/// The sidecars double as per-family checkpoints: a family whose sidecar
/// already exists is skipped, so an interrupted run resumes where it stopped.
/// Delete `target/timemachine/` for a from-scratch measurement.
pub fn run_memory(versions: &[Box<dyn Parser>], full: bool) {
    let corpus = load_corpus(full);
    for (family, vs) in by_family(versions) {
        let checkpoint = sidecar_path(family);
        if checkpoint.exists() {
            eprintln!(
                "skipping {family}: checkpoint {} exists (delete it to re-measure)",
                checkpoint.display()
            );
            continue;
        }
        let mut version_runs = Vec::new();
        for p in vs {
            let id = p.id();
            let mut dialects = Vec::new();
            for &d in DIALECTS {
                if !p.supports(d) {
                    continue;
                }
                let stmts = &corpus[d.dir_name()];
                if stmts.is_empty() {
                    continue;
                }
                let memory = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    mem_dialect_run(p, d, stmts)
                }))
                .unwrap_or_else(|_| {
                    eprintln!(
                        "  [warn] mem {family} {} {} panicked, skipping",
                        id.version,
                        d.dir_name()
                    );
                    None
                });
                eprintln!(
                    "mem {family} {} {}: n={}",
                    id.version,
                    d.dir_name(),
                    memory.as_ref().map_or(0, |m| m.n),
                );
                dialects.push(DialectRun {
                    dir_name: d.dir_name().to_string(),
                    display_name: d.display_name().to_string(),
                    has_reference: false,
                    perf: None,
                    memory,
                    batch: None,
                    correctness: None,
                });
            }
            version_runs.push(VersionRun {
                version: id.version.to_string(),
                released: id.released.to_string(),
                dialects,
            });
        }
        let history = FamilyHistory {
            family: family.to_string(),
            versions: version_runs,
        };
        write_sidecar(&history);
    }
}

/// Serialize all families to one JSON array and zstd-compress it for embedding.
fn write_combined(histories: &[FamilyHistory]) {
    let path = PathBuf::from(HISTORY_FILE);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_vec(histories).expect("serialize history");
    let compressed = zstd::stream::encode_all(json.as_slice(), 19).expect("zstd compress");
    if let Err(e) = std::fs::write(&path, &compressed) {
        eprintln!("ERROR: writing {}: {e}", path.display());
    } else {
        eprintln!(
            "wrote {} ({} families, {} KB compressed from {} KB)",
            path.display(),
            histories.len(),
            compressed.len() / 1024,
            json.len() / 1024,
        );
    }
}

fn sidecar_path(family: &str) -> PathBuf {
    PathBuf::from(SIDECAR_DIR).join(format!("{}.mem.json", family_slug(family)))
}

fn write_sidecar(history: &FamilyHistory) {
    let _ = std::fs::create_dir_all(SIDECAR_DIR);
    let path = sidecar_path(&history.family);
    let json = serde_json::to_string(history).expect("serialize sidecar");
    if let Err(e) = std::fs::write(&path, json) {
        eprintln!("ERROR: writing {}: {e}", path.display());
    } else {
        eprintln!("wrote {}", path.display());
    }
}

fn read_sidecar(family: &str) -> Option<FamilyHistory> {
    let path = sidecar_path(family);
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Find the memory entry for a (version, dialect) in the sidecar.
fn sidecar_lookup(sidecar: Option<&FamilyHistory>, version: &str, dir: &str) -> Option<ParserMem> {
    sidecar?
        .versions
        .iter()
        .find(|v| v.version == version)?
        .dialects
        .iter()
        .find(|d| d.dir_name == dir)?
        .memory
        .clone()
}

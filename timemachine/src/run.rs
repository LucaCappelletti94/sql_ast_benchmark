//! Shared driving logic for the time-machine runners.
//!
//! Builds the per-family [`FamilyHistory`] for the registry's versions, reusing
//! the main crate's grading ([`report::grade_chunk`]) and summary helpers
//! ([`stats::perf_from`], [`stats::dist_from`]) so the history is computed the
//! same way as the current snapshot. Timing and memory run as separate binaries
//! (the memory one installs a global allocator), each producing part of the
//! history. The timing binary merges in the memory sidecar and writes the final
//! per-family file.

use sql_ast_benchmark::batch::{batch_eligible, evaluate_batches, reports_statement_count};
use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::report::{self, load_dialect};
use sql_ast_benchmark::{stats, Parser};
use std::collections::{BTreeMap, HashSet};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;
use viz::{
    DialectDelta, DialectRun, FamilyHistory, ParserBatch, ParserMem, ParserMetrics, VersionRun,
};

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
        accepted_valid_contentious: s.accepted_valid_contentious,
        // Recall over the non-contentious valid statements, the secondary metric
        // the main snapshot reports, now tracked across releases too.
        recall_excl_contentious_pct: if reference {
            pct(
                s.accepted_valid - s.accepted_valid_contentious,
                report.valid_total - report.contentious_valid,
            )
        } else {
            None
        },
        // Empirical panic rate: the adapters override `parse_outcome` to surface a
        // caught panic, so `grade_chunk` counts it here too.
        attempted: s.attempted,
        panicked: s.panicked,
        panic_pct: pct(s.panicked, s.attempted),
    }
}

/// Truncate `s` to at most `max` characters for a compact example, marking it.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head} ...")
    }
}

/// The per-dialect change from the `prev` accepted set to `cur`: exact gained and
/// lost counts, plus a few sorted, truncated example statements for each.
fn coverage_delta(dir: &str, prev: &HashSet<String>, cur: &HashSet<String>) -> DialectDelta {
    const EXAMPLES: usize = 3;
    const MAX_LEN: usize = 200;
    let mut gained: Vec<&String> = cur.difference(prev).collect();
    let mut lost: Vec<&String> = prev.difference(cur).collect();
    gained.sort();
    lost.sort();
    let sample = |v: &[&String]| -> Vec<String> {
        v.iter()
            .take(EXAMPLES)
            .map(|s| truncate(s, MAX_LEN))
            .collect()
    };
    DialectDelta {
        dir_name: dir.to_string(),
        gained: gained.len(),
        lost: lost.len(),
        examples_gained: sample(&gained),
        examples_lost: sample(&lost),
    }
}

/// The set of statements one version accepts in one dialect, without timing
/// (cheap: one parse per statement). Used to recompute deltas during a refresh.
fn accepted_set(p: &dyn Parser, d: Dialect, stmts: &[String]) -> HashSet<String> {
    stmts
        .iter()
        .filter(|s| p.accepts(s, d) == Some(true))
        .cloned()
        .collect()
}

/// Build the timing + batch + correctness part of one version's run (no memory),
/// plus the set of statements this version accepted in this dialect (for the
/// version-to-version coverage deltas).
fn timing_dialect_run(p: &dyn Parser, d: Dialect, stmts: &[String]) -> (DialectRun, Vec<String>) {
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

    let count = |s: &str| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            p.parse_batch(s, d).unwrap_or(0)
        }))
        .unwrap_or(0)
    };
    let eligible: Vec<&str> = if !p.can_batch() || !reports_statement_count(|s| count(s)) {
        Vec::new()
    } else {
        accepted
            .iter()
            .copied()
            .filter(|s| batch_eligible(s) && count(s) == 1)
            .collect()
    };
    let batch = if eligible.is_empty() {
        None
    } else {
        let label = format!("{}/{}", d.dir_name(), p.id().family);
        let eval = evaluate_batches(&eligible, &label, count);
        let ns_per_stmt = if eval.n_correct == 0 {
            None
        } else {
            let denom = (eval.n_correct * eval.effective_m) as f64;
            let ns = time_batch(|| eval.correct_scripts.iter().map(|s| count(s)).sum());
            Some(ns / denom)
        };
        Some(ParserBatch {
            parser: p.id().family.to_string(),
            n_accepted: eval.n_eligible,
            accuracy_pct: eval.accuracy_pct(),
            ns_per_stmt,
            peak_per_stmt: None,
            retained_per_stmt: None,
        })
    };

    let report = report::grade_chunk(stmts, d, &[p]);
    let correctness = Some(metrics_of(&report));

    let accepted_owned: Vec<String> = accepted.iter().map(|s| (*s).to_string()).collect();
    (
        DialectRun {
            dir_name: d.dir_name().to_string(),
            display_name: d.display_name().to_string(),
            has_reference: report.has_reference,
            perf,
            memory: None,
            batch,
            correctness,
        },
        accepted_owned,
    )
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
/// and write the final combined history. Returns the families written.
///
/// On a full run each finished family is checkpointed to a `.timing.json`, and a
/// family with a fresh checkpoint (no older than its memory sidecar) is loaded
/// rather than recomputed, so an interrupted run resumes family by family. Delete
/// `target/timemachine/` for a from-scratch run.
pub fn run_timing(versions: &[Box<dyn Parser>], full: bool) -> Vec<String> {
    let corpus = load_corpus(full);
    let mut histories = Vec::new();
    let mut written = Vec::new();
    for (family, vs) in by_family(versions) {
        // Resume: reuse a fresh checkpoint instead of recomputing the family.
        if full {
            if let Some(cached) = cached_timing(family) {
                eprintln!("time {family}: cached checkpoint, skipping");
                histories.push(cached);
                written.push(family.to_string());
                continue;
            }
        }
        let sidecar = read_sidecar(family);
        let mut version_runs = Vec::new();
        // Accepted sets of the previous version, per dialect, for coverage deltas.
        let mut prev_accepted: BTreeMap<String, HashSet<String>> = BTreeMap::new();
        for p in vs {
            let id = p.id();
            let mut dialects = Vec::new();
            let mut deltas = Vec::new();
            let mut cur_accepted: BTreeMap<String, HashSet<String>> = BTreeMap::new();
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
                let Ok((mut run, accepted)) = outcome else {
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
                let dir = d.dir_name().to_string();
                let acc_set: HashSet<String> = accepted.into_iter().collect();
                if let Some(prev) = prev_accepted.get(&dir) {
                    deltas.push(coverage_delta(&dir, prev, &acc_set));
                }
                cur_accepted.insert(dir, acc_set);
                dialects.push(run);
            }
            version_runs.push(VersionRun {
                version: id.version.to_string(),
                released: id.released.to_string(),
                dialects,
                deltas,
            });
            prev_accepted = cur_accepted;
        }
        let history = FamilyHistory {
            family: family.to_string(),
            versions: version_runs,
        };
        // Checkpoint the finished family so a later interruption can resume here.
        if full {
            write_timing(&history);
        }
        histories.push(history);
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
                // The memory pass writes only the memory sidecar, never the deltas
                // (the timing pass owns those), so leave them empty here.
                deltas: Vec::new(),
            });
        }
        let history = FamilyHistory {
            family: family.to_string(),
            versions: version_runs,
        };
        write_sidecar(&history);
    }
}

/// The registry's parsers for one family, in release order.
fn family_versions<'a>(versions: &'a [Box<dyn Parser>], family: &str) -> Vec<&'a dyn Parser> {
    by_family(versions)
        .into_iter()
        .find(|(f, _)| *f == family)
        .map(|(_, vs)| vs)
        .unwrap_or_default()
}

/// Incremental memory refresh: recompute the memory sidecar entries for only the
/// listed versions of `family`, splicing them into the existing sidecar. Run in
/// the memory binary (it installs the counting allocator). Other versions and
/// families are left untouched.
pub fn run_memory_refresh(versions: &[Box<dyn Parser>], family: &str, refresh: &[String]) {
    let corpus = load_corpus(true);
    let mut sidecar = read_sidecar(family).unwrap_or_else(|| FamilyHistory {
        family: family.to_string(),
        versions: Vec::new(),
    });
    for p in family_versions(versions, family) {
        let id = p.id();
        if !refresh.iter().any(|r| r == id.version) {
            continue;
        }
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
            .unwrap_or(None);
            eprintln!(
                "mem-refresh {family} {} {}: n={}",
                id.version,
                d.dir_name(),
                memory.as_ref().map_or(0, |m| m.n)
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
        let vr = VersionRun {
            version: id.version.to_string(),
            released: id.released.to_string(),
            dialects,
            deltas: Vec::new(),
        };
        match sidecar
            .versions
            .iter_mut()
            .find(|v| v.version == id.version)
        {
            Some(slot) => *slot = vr,
            None => sidecar.versions.push(vr),
        }
    }
    write_sidecar(&sidecar);
}

/// Incremental timing refresh: recompute the timing, correctness, and batch for
/// only the listed versions of `family` (merging their refreshed memory from the
/// sidecar), reuse every other version's metrics from the committed history, and
/// recompute all of the family's deltas from freshly determined accepted sets.
/// The result is identical to a full re-run, without re-measuring unchanged
/// points. Returns an error if the committed history cannot be read.
pub fn run_refresh(
    versions: &[Box<dyn Parser>],
    family: &str,
    refresh: &[String],
) -> Result<(), String> {
    let corpus = load_corpus(true);
    let mut history = read_combined().ok_or_else(|| format!("cannot read {HISTORY_FILE}"))?;
    let baseline = history
        .iter()
        .find(|h| h.family == family)
        .cloned()
        .ok_or_else(|| format!("{family} not present in {HISTORY_FILE}"))?;
    let sidecar = read_sidecar(family);

    let mut new_versions = Vec::new();
    let mut prev_accepted: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for p in family_versions(versions, family) {
        let id = p.id();
        let refreshing = refresh.iter().any(|r| r == id.version);
        let mut cur_accepted: BTreeMap<String, HashSet<String>> = BTreeMap::new();

        let dialects: Vec<DialectRun> = if refreshing {
            let mut ds = Vec::new();
            for &d in DIALECTS {
                if !p.supports(d) {
                    continue;
                }
                let stmts = &corpus[d.dir_name()];
                if stmts.is_empty() {
                    continue;
                }
                let Ok((mut run, accepted)) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        timing_dialect_run(p, d, stmts)
                    }))
                else {
                    eprintln!(
                        "  [warn] refresh {family} {} {} panicked, skipping",
                        id.version,
                        d.dir_name()
                    );
                    continue;
                };
                run.memory = sidecar_lookup(sidecar.as_ref(), id.version, d.dir_name());
                eprintln!(
                    "refresh {family} {} {}: n={}",
                    id.version,
                    d.dir_name(),
                    run.perf.as_ref().map_or(0, |x| x.n_accepted)
                );
                cur_accepted.insert(d.dir_name().to_string(), accepted.into_iter().collect());
                ds.push(run);
            }
            ds
        } else {
            // Unchanged version: reuse its committed metrics, but still determine
            // its accepted sets (cheaply) so neighbouring deltas stay correct.
            for &d in DIALECTS {
                if !p.supports(d) {
                    continue;
                }
                let stmts = &corpus[d.dir_name()];
                if stmts.is_empty() {
                    continue;
                }
                cur_accepted.insert(d.dir_name().to_string(), accepted_set(p, d, stmts));
            }
            baseline
                .versions
                .iter()
                .find(|v| v.version == id.version)
                .map(|v| v.dialects.clone())
                .unwrap_or_default()
        };

        let mut deltas = Vec::new();
        for &d in DIALECTS {
            let dir = d.dir_name().to_string();
            if let (Some(prev), Some(cur)) = (prev_accepted.get(&dir), cur_accepted.get(&dir)) {
                deltas.push(coverage_delta(&dir, prev, cur));
            }
        }
        new_versions.push(VersionRun {
            version: id.version.to_string(),
            released: id.released.to_string(),
            dialects,
            deltas,
        });
        prev_accepted = cur_accepted;
    }

    if let Some(slot) = history.iter_mut().find(|h| h.family == family) {
        slot.versions = new_versions;
    }
    write_combined(&history);
    Ok(())
}

/// Parse a `--refresh <family>:<v1>,<v2>,...` spec from the args, if present.
#[must_use]
pub fn parse_refresh(args: &[String]) -> Option<(String, Vec<String>)> {
    let i = args.iter().position(|a| a == "--refresh")?;
    let spec = args.get(i + 1)?;
    let (family, vers) = spec.split_once(':')?;
    let versions = vers.split(',').map(str::to_string).collect();
    Some((family.to_string(), versions))
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

/// Read the committed combined history back (for the incremental refresh mode).
fn read_combined() -> Option<Vec<FamilyHistory>> {
    let raw = std::fs::read(HISTORY_FILE).ok()?;
    let json = zstd::stream::decode_all(raw.as_slice()).ok()?;
    serde_json::from_slice(&json).ok()
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

/// Per-family timing checkpoint path (distinct from the `.mem.json` sidecar), so
/// the timing pass can resume after an interruption family by family.
fn timing_path(family: &str) -> PathBuf {
    PathBuf::from(SIDECAR_DIR).join(format!("{}.timing.json", family_slug(family)))
}

/// Write one family's finished timing result as a resume checkpoint.
fn write_timing(history: &FamilyHistory) {
    let _ = std::fs::create_dir_all(SIDECAR_DIR);
    let path = timing_path(&history.family);
    let json = serde_json::to_string(history).expect("serialize timing checkpoint");
    if let Err(e) = std::fs::write(&path, json) {
        eprintln!("ERROR: writing {}: {e}", path.display());
    }
}

/// The cached timing result for `family`, if its checkpoint exists and is at
/// least as new as the memory sidecar. A refreshed memory pass thus invalidates a
/// stale timing checkpoint automatically (the memory is merged into the timing
/// result, so an older checkpoint would carry outdated memory).
fn cached_timing(family: &str) -> Option<FamilyHistory> {
    let path = timing_path(family);
    let checkpoint_mtime = std::fs::metadata(&path).ok()?.modified().ok()?;
    if let Ok(mem_mtime) = std::fs::metadata(sidecar_path(family)).and_then(|m| m.modified()) {
        if mem_mtime > checkpoint_mtime {
            return None;
        }
    }
    serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()
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

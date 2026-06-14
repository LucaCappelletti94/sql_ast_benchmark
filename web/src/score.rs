//! A single composite "overall score" per parser, on a 0 to 100 scale, that
//! folds every dimension the benchmark measures into one correctness-first
//! number, alongside the five sub-scores it is built from.
//!
//! Methodology (kept deliberately transparent: the sub-scores are always shown):
//!
//! - In-scope only. A parser is judged solely on the dialects it actually
//!   models. It is never zeroed for a dialect it never claimed to support, and
//!   breadth is not itself rewarded: a focused parser that masters its scope can
//!   outrank a broad one that is mediocre everywhere.
//! - Five sub-scores, each 0 to 100: correctness, robustness, speed, memory, and
//!   project health. The overall is their weighted blend,
//!   `0.45 correctness + 0.20 robustness + 0.15 health + 0.12 speed + 0.08 memory`.
//!   Any sub-score that does not apply to a parser (for example memory for an FFI
//!   binding, whose allocations are invisible to the Rust allocator) is dropped
//!   and the remaining weights are renormalized, so nothing is penalized for a
//!   dimension that cannot be measured.
//! - Correctness and health are absolute: they read straight off the measured
//!   rates and the recorded project facts. Speed and memory are relative to the
//!   field within each dialect (parse times and footprints span orders of
//!   magnitude), so a parser is ranked against the peers it competes with on each
//!   dialect, then averaged over its dialects.

use crate::cadence::Cadence;
use crate::data::{bundle, panic_totals, parser_depth, parser_features};
use crate::metadata::{license_ok, maintained, parser_meta, Fuzz};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Weights of the five sub-scores in the overall blend. Correctness leads,
/// safety next, then project health, with speed and memory as tiebreakers.
const W_CORRECTNESS: f64 = 0.45;
const W_ROBUSTNESS: f64 = 0.20;
const W_HEALTH: f64 = 0.15;
const W_SPEED: f64 = 0.12;
const W_MEMORY: f64 = 0.08;

/// One parser's composite score and the sub-scores behind it, each 0 to 100.
/// A sub-score is `None` when the dimension does not apply to the parser.
#[derive(Clone, Copy, PartialEq)]
pub struct ParserScore {
    /// Weighted blend of the available sub-scores, 0 to 100.
    pub overall: f64,
    pub correctness: Option<f64>,
    pub robustness: Option<f64>,
    pub speed: Option<f64>,
    pub memory: Option<f64>,
    pub health: Option<f64>,
}

/// The composite score for one parser by display name, if it can be scored.
#[must_use]
pub fn parser_score(name: &str) -> Option<&'static ParserScore> {
    all_scores().get(name)
}

/// Every parser's score, computed once. Speed and memory need the whole field
/// (they are relative within each dialect), so all parsers are scored together.
#[must_use]
pub fn all_scores() -> &'static BTreeMap<String, ParserScore> {
    static CACHE: OnceLock<BTreeMap<String, ParserScore>> = OnceLock::new();
    CACHE.get_or_init(compute_all)
}

fn compute_all() -> BTreeMap<String, ParserScore> {
    let b = bundle();
    let mut out = BTreeMap::new();
    for parser in &b.parsers {
        let correctness = correctness_score(parser);
        let robustness = robustness_score(parser);
        let speed = speed_score(parser);
        let memory = memory_score(parser);
        let health = health_score(parser);

        // Weighted blend over the sub-scores that apply, weights renormalized.
        let parts = [
            (correctness, W_CORRECTNESS),
            (robustness, W_ROBUSTNESS),
            (health, W_HEALTH),
            (speed, W_SPEED),
            (memory, W_MEMORY),
        ];
        let mut sum = 0.0;
        let mut wsum = 0.0;
        for (v, w) in parts {
            if let Some(v) = v {
                sum += v * w;
                wsum += w;
            }
        }
        let overall = if wsum > 0.0 { sum / wsum } else { 0.0 };

        out.insert(
            parser.clone(),
            ParserScore {
                overall,
                correctness,
                robustness,
                speed,
                memory,
                health,
            },
        );
    }
    out
}

/// Correctness, 0 to 100: per dialect, blend the measured rates (primary
/// recall or acceptance, plus false-positive avoidance, round-trip, and
/// fidelity where they exist), then average over the dialects the parser models.
fn correctness_score(parser: &str) -> Option<f64> {
    let mut per_dialect = Vec::new();
    for d in &bundle().dialects {
        let Some(m) = d.correctness.iter().find(|m| m.parser == parser) else {
            continue;
        };
        // Primary signal: recall on reference dialects, acceptance elsewhere.
        let Some(primary) = m.recall_pct.or(m.accept_pct) else {
            continue;
        };
        let mut num = 0.5 * (primary / 100.0);
        let mut den = 0.5;
        if let Some(fp) = m.false_positive_pct {
            num += 0.2 * (1.0 - fp / 100.0);
            den += 0.2;
        }
        if let Some(rt) = m.roundtrip_pct {
            num += 0.15 * (rt / 100.0);
            den += 0.15;
        }
        if let Some(fid) = m.fidelity_pct {
            num += 0.15 * (fid / 100.0);
            den += 0.15;
        }
        per_dialect.push(num / den);
    }
    mean(&per_dialect).map(|v| v * 100.0)
}

/// Robustness, 0 to 100: how safely the parser behaves. Blends the observed
/// panic rate on the real corpus (weighted most, it is behavior not a proxy),
/// recursion-depth guarding, the unsafe surface, and static panic discipline.
fn robustness_score(parser: &str) -> Option<f64> {
    let mut num = 0.0;
    let mut den = 0.0;

    // Empirical panic safety: 1 minus the share of statements that panicked.
    if let Some((panicked, attempted)) = panic_totals(parser) {
        if attempted > 0 {
            num += 0.40 * (1.0 - panicked as f64 / attempted as f64);
            den += 0.40;
        }
    }

    // Recursion depth: full credit when the parser never overflows the stack up
    // to the probe ceiling, otherwise partial credit scaled by how deep it got
    // before crashing (a crash at 5000 is far less alarming than one at 200).
    if let Some(depth) = parser_depth(parser) {
        let v = match depth.crash_depth {
            None => 1.0,
            Some(c) => 0.5 * (c as f64 / depth.ceil.max(1) as f64).min(1.0),
        };
        num += 0.25 * v;
        den += 0.25;
    }

    if let Some(f) = parser_features(parser) {
        // Unsafe surface: clean when it forbids unsafe or has none, else it
        // decays with the count of unsafe blocks, fns, and impls.
        let unsafe_total = f.counts.unsafe_total();
        let unsafe_v = if f.forbids_unsafe || unsafe_total == 0 {
            1.0
        } else {
            (1.0 - unsafe_total as f64 / 50.0).max(0.0)
        };
        num += 0.20 * unsafe_v;
        den += 0.20;

        // Static panic discipline: full credit when the crate bans the panicking
        // lints by design, otherwise it decays with the density of panic-prone
        // constructs per thousand non-test lines.
        let banned = f.lints.is_banned("unwrap_used")
            || f.lints.is_banned("panic")
            || f.lints.is_banned("expect_used");
        let disc_v = if banned {
            1.0
        } else {
            let prone = (f.counts.hard_panics() + f.counts.unwrap + f.counts.expect) as f64;
            let per_kloc = prone / f.counts.code_loc.max(1) as f64 * 1000.0;
            (1.0 - per_kloc / 20.0).max(0.0)
        };
        num += 0.15 * disc_v;
        den += 0.15;
    }

    (den > 0.0).then(|| num / den * 100.0)
}

/// Speed, 0 to 100: the parser's median parse time ranked against the field
/// within each dialect on a log scale, averaged over its dialects.
fn speed_score(parser: &str) -> Option<f64> {
    relative_score(parser, |perf| (perf.n_accepted > 0).then_some(perf.median))
}

/// Memory, 0 to 100: peak and retained per-statement footprints, each ranked
/// against the field within each dialect on a log scale and averaged. `None`
/// for FFI parsers, whose C-side allocations the Rust allocator never sees.
fn memory_score(parser: &str) -> Option<f64> {
    let b = bundle();
    let mut per_dialect = Vec::new();
    for d in &b.dialects {
        if d.memory.iter().all(|m| m.parser != parser) {
            continue;
        }
        let peak: Vec<f64> = d.memory.iter().map(|m| m.peak.median).collect();
        let retained: Vec<f64> = d.memory.iter().map(|m| m.retained.median).collect();
        let mine = d.memory.iter().find(|m| m.parser == parser).unwrap();
        let rp = relative_log(mine.peak.median, &peak);
        let rr = relative_log(mine.retained.median, &retained);
        match (rp, rr) {
            (Some(a), Some(c)) => per_dialect.push((a + c) / 2.0),
            (Some(a), None) | (None, Some(a)) => per_dialect.push(a),
            (None, None) => {}
        }
    }
    mean(&per_dialect).map(|v| v * 100.0)
}

/// Project health, 0 to 100: an unweighted average of engineering-practice
/// indicators (maintenance, testing, fuzzing, sanitizers, supply-chain gates,
/// licensing, release cadence, contributor depth). Deliberately excludes
/// popularity proxies like stars and downloads: this is a merit signal.
fn health_score(parser: &str) -> Option<f64> {
    let m = parser_meta(parser)?;
    let fuzz = match m.fuzz {
        Fuzz::Yes => 1.0,
        Fuzz::Upstream => 0.7,
        Fuzz::No => 0.0,
    };
    let cadence = match m.cadence {
        Cadence::Rolling | Cadence::Monthly => 1.0,
        Cadence::Quarterly => 0.8,
        Cadence::Yearly => 0.5,
        Cadence::Irregular => 0.4,
        Cadence::Multiyear => 0.3,
        Cadence::Dormant => 0.0,
    };
    let indicators = [
        f64::from(maintained(m.last_release)),
        fuzz,
        f64::from(m.tests),
        f64::from(m.benches),
        f64::from(license_ok(m.license)),
        f64::from(m.crates_io),
        f64::from(!m.sanitizers.is_empty()),
        f64::from(m.cargo_audit),
        f64::from(m.cargo_deny),
        f64::from(m.cargo_mutants),
        cadence,
        // Bus-factor proxy: ten or more distinct contributors is full credit.
        (f64::from(m.contributors) / 10.0).min(1.0),
    ];
    mean(&indicators).map(|v| v * 100.0)
}

/// Average a parser's per-dialect relative rank for a timing field (lower is
/// better), over the dialects where it and at least one peer have the field.
fn relative_score(parser: &str, pick: impl Fn(&viz::ParserPerf) -> Option<f64>) -> Option<f64> {
    let b = bundle();
    let mut per_dialect = Vec::new();
    for d in &b.dialects {
        let field: Vec<f64> = d.perf.iter().filter_map(&pick).collect();
        let Some(mine) = d.perf.iter().find(|p| p.parser == parser).and_then(&pick) else {
            continue;
        };
        if let Some(v) = relative_log(mine, &field) {
            per_dialect.push(v);
        }
    }
    mean(&per_dialect).map(|v| v * 100.0)
}

/// Position of `value` within `field` on a log scale, where the smallest value
/// in the field scores 1.0 and the largest 0.0 (lower is better). Returns `None`
/// when the field has no spread (single competitor), so it does not distort the
/// average with an arbitrary 1.0.
fn relative_log(value: f64, field: &[f64]) -> Option<f64> {
    let lo = field.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = field.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo {
        return None;
    }
    let l = |x: f64| x.max(1.0).ln();
    Some(((l(hi) - l(value)) / (l(hi) - l(lo))).clamp(0.0, 1.0))
}

/// Mean of a slice, or `None` when empty.
fn mean(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parser_scores_in_range() {
        for (name, s) in all_scores() {
            assert!(
                (0.0..=100.0).contains(&s.overall),
                "{name} overall {} out of range",
                s.overall
            );
            for sub in [s.correctness, s.robustness, s.speed, s.memory, s.health] {
                if let Some(v) = sub {
                    assert!(
                        (0.0..=100.0).contains(&v),
                        "{name} sub-score {v} out of range"
                    );
                }
            }
            // Correctness and health apply to every benchmarked parser.
            assert!(s.correctness.is_some(), "{name} has no correctness score");
            assert!(s.health.is_some(), "{name} has no health score");
        }
    }

    #[test]
    fn print_leaderboard() {
        let mut rows: Vec<_> = all_scores().iter().collect();
        rows.sort_by(|a, b| b.1.overall.partial_cmp(&a.1.overall).unwrap());
        let f = |o: Option<f64>| o.map_or_else(|| "  n/a".to_string(), |v| format!("{v:5.1}"));
        println!(
            "\n{:<22} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6}",
            "parser", "overall", "corr", "robust", "speed", "mem", "health"
        );
        for (name, s) in rows {
            println!(
                "{:<22} {:>7.1} {} {} {} {} {}",
                name,
                s.overall,
                f(s.correctness),
                f(s.robustness),
                f(s.speed),
                f(s.memory),
                f(s.health)
            );
        }
    }
}

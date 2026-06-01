#![allow(clippy::cast_precision_loss, clippy::tuple_array_conversions)]

//! Exports the committed results snapshot (`web/assets/bench.json`) that the
//! Dioxus viewer renders.
//!
//! Reuses the threaded grading and per-file coverage in [`crate::report`], the
//! perf percentiles in `target/bench_dist/summary.csv`, the raw timings (for
//! downsampled eCDF points), and the shared [`viz`] schema. Run via `sqlbench
//! export` after `cargo bench` has produced the timing data.

use crate::datasets::Dialect;
use crate::report::{self, DialectReport};
use crate::{bench_dist, stats, BenchParser};
use std::cmp::Ordering;
use std::path::Path;
use viz::{Bundle, CoverageFile, CoverageMatrix, DialectData, ParserMetrics, ParserPerf};

/// Output path (relative to repo root, where `cargo run` runs from).
const OUT: &str = "web/assets/bench.json";

/// Reference-backed dialects first, then provenance, matching the CLI order.
const ORDER: [Dialect; 13] = [
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

fn pct(n: usize, base: usize) -> Option<f64> {
    (base != 0).then(|| 100.0 * n as f64 / base as f64)
}

/// One parsed `summary.csv` row (all 14 columns).
struct PerfRow {
    dialect: String,
    parser: String,
    n_total: usize,
    n_accepted: usize,
    /// min, p10, p25, median, p75, p90, p99, max, mean (ns).
    pct: [f64; 9],
    roundtrip_pct: Option<f64>,
}

fn read_summary() -> Vec<PerfRow> {
    let path = format!("{}/summary.csv", bench_dist::DIST_DIR);
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 13 {
                return None;
            }
            let n_total = f[2].trim().parse().ok()?;
            let n_accepted = f[3].trim().parse().ok()?;
            let mut p = [0.0_f64; 9];
            for (i, slot) in p.iter_mut().enumerate() {
                *slot = f
                    .get(4 + i)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0.0);
            }
            let roundtrip_pct = f
                .get(13)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| *v >= 0.0);
            Some(PerfRow {
                dialect: f[0].to_string(),
                parser: f[1].to_string(),
                n_total,
                n_accepted,
                pct: p,
                roundtrip_pct,
            })
        })
        .collect()
}

fn metrics(report: &DialectReport) -> Vec<ParserMetrics> {
    let reference = report.has_reference;
    report
        .parsers
        .iter()
        .zip(&report.stats)
        .map(|(p, s)| ParserMetrics {
            parser: p.name().to_string(),
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
            fidelity_pct: if reference && s.can_reprint {
                pct(s.fidelity_ok, s.accepted_valid)
            } else {
                None
            },
            accept_pct: if reference {
                None
            } else {
                pct(s.accepted_valid, report.valid_total)
            },
        })
        .collect()
}

/// Build the per-parser perf series for a dialect, sorted fastest-median first.
fn perf_for(dir: &str, rows: &[PerfRow]) -> Vec<ParserPerf> {
    let mut v: Vec<ParserPerf> = rows
        .iter()
        .filter(|r| r.dialect == dir)
        .map(|r| {
            let times = bench_dist::load_times(dir, &r.parser);
            let ecdf = stats::ecdf_points(&times, 200)
                .into_iter()
                .map(|(x, y)| [x, y])
                .collect();
            ParserPerf {
                parser: r.parser.clone(),
                n_total: r.n_total,
                n_accepted: r.n_accepted,
                min: r.pct[0],
                p10: r.pct[1],
                p25: r.pct[2],
                median: r.pct[3],
                p75: r.pct[4],
                p90: r.pct[5],
                p99: r.pct[6],
                max: r.pct[7],
                mean: r.pct[8],
                roundtrip_pct: r.roundtrip_pct,
                ecdf,
            }
        })
        .collect();
    v.sort_by(|a, b| a.median.partial_cmp(&b.median).unwrap_or(Ordering::Equal));
    v
}

fn coverage_for(dialect: Dialect, all_parsers: &[BenchParser]) -> CoverageMatrix {
    let (parsers, files) = report::coverage_dialect(dialect, all_parsers);
    let cols: Vec<String> = parsers.iter().map(|p| p.name().to_string()).collect();
    let mut subtotal_accepted = vec![0usize; cols.len()];
    let mut subtotal_total = 0usize;
    let files_out = files
        .iter()
        .map(|f| {
            subtotal_total += f.total;
            for (i, a) in f.accepted.iter().enumerate() {
                subtotal_accepted[i] += a;
            }
            CoverageFile {
                name: f.name.clone(),
                total: f.total,
                accepted: f.accepted.clone(),
            }
        })
        .collect();
    CoverageMatrix {
        parsers: cols,
        files: files_out,
        subtotal_total,
        subtotal_accepted,
    }
}

fn now_utc() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            format!("unix:{secs}")
        })
}

fn git_short() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Grade every dialect, gather perf + coverage, and write `web/assets/bench.json`.
///
/// # Errors
/// Returns an error if serialization or writing the output file fails.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let parsers = BenchParser::all();
    let summary = read_summary();
    if summary.is_empty() {
        eprintln!(
            "warning: no {}/summary.csv; perf charts will be empty. Run `cargo bench` first.",
            bench_dist::DIST_DIR
        );
    }

    let mut dialects = Vec::new();
    for &d in &ORDER {
        let Some(report) = report::grade_dialect(d, &parsers) else {
            continue;
        };
        eprintln!("exported {}", d.dir_name());
        dialects.push(DialectData {
            dir_name: d.dir_name().to_string(),
            display_name: d.display_name().to_string(),
            has_reference: report.has_reference,
            valid_total: report.valid_total,
            invalid_total: report.invalid_total,
            correctness: metrics(&report),
            perf: perf_for(d.dir_name(), &summary),
            coverage: coverage_for(d, &parsers),
        });
    }

    let bundle = Bundle {
        generated_utc: now_utc(),
        git_commit: git_short(),
        parsers: parsers.iter().map(|p| p.name().to_string()).collect(),
        dialects,
    };

    let json = serde_json::to_string_pretty(&bundle)?;
    let out = Path::new(OUT);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, json)?;
    println!("Wrote {OUT} ({} dialects)", bundle.dialects.len());
    Ok(())
}

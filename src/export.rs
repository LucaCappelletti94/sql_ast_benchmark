#![allow(clippy::cast_precision_loss, clippy::tuple_array_conversions)]

//! Exports the committed results snapshot (`web/assets/bench.json.zst`, a
//! zstd-compressed JSON bundle) that the Dioxus viewer embeds and decompresses.
//!
//! Reuses the threaded grading and per-file coverage in [`crate::report`], the
//! perf percentiles in `target/bench_dist/summary.csv`, the raw timings (for
//! downsampled eCDF points), and the shared [`viz`] schema. Run via `sqlbench
//! export` after `cargo bench` has produced the timing data.

use crate::datasets::Dialect;
use crate::report::{self, DialectReport};
use crate::{bench_dist, stats, BenchParser, Parser};
use std::cmp::Ordering;
use std::path::Path;
use viz::{
    Bundle, CoverageFile, CoverageMatrix, DialectData, MemDist, ParserBatch, ParserFailures,
    ParserMem, ParserMetrics, ParserPerf,
};

/// Output path (relative to repo root, where `cargo run` runs from).
const OUT: &str = "web/assets/bench.json.zst";

/// Directory (relative to the site root) for the rejected-statement downloads.
const FAILURES_DIR: &str = "web/static/failures";

/// Max rejected statements written to each `.tsv.zst` download (a sample when a
/// parser rejects more, with the true total reported separately).
const FAIL_CAP: usize = 1000;

/// Rejected statements shown inline as a preview on each parser page.
const FAIL_PREVIEW: usize = 10;

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
    std::fs::read_to_string(path).map_or_else(|_| Vec::new(), |c| parse_summary(&c))
}

/// Parse `summary.csv` content (header + rows) into [`PerfRow`]s, skipping rows
/// that are too short or have unparsable counts.
fn parse_summary(content: &str) -> Vec<PerfRow> {
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
            parser: p.family.to_string(),
            version: p.version.to_string(),
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
            fidelity_pct: if crate::has_canonical(report.dialect) && s.can_reprint {
                pct(s.fidelity_ok, s.accepted_valid)
            } else {
                None
            },
            accept_pct: if reference {
                None
            } else {
                pct(s.accepted_valid, report.valid_total)
            },
            attempted: s.attempted,
            panicked: s.panicked,
            panic_pct: pct(s.panicked, s.attempted),
        })
        .collect()
}

/// Build the per-parser perf series for a dialect, sorted fastest-median first.
fn perf_for(dir: &str, rows: &[PerfRow]) -> Vec<ParserPerf> {
    let mut v: Vec<ParserPerf> = rows
        .iter()
        .filter(|r| r.dialect == dir)
        .map(|r| {
            let raw = bench_dist::load_times(dir, &r.parser);
            let ecdf = stats::ecdf_points(&raw, 200)
                .into_iter()
                .map(|(x, y)| [x, y])
                .collect();
            perf_row_to_perf(r, stats::std_dev(&raw), ecdf)
        })
        .collect();
    v.sort_by(|a, b| a.median.partial_cmp(&b.median).unwrap_or(Ordering::Equal));
    v
}

/// Map a parsed `summary.csv` row, its standard deviation, and its eCDF points to
/// a `ParserPerf`. Pure, so the percentile-to-field wiring is testable without
/// the timing files.
fn perf_row_to_perf(r: &PerfRow, std: f64, ecdf: Vec<[f64; 2]>) -> ParserPerf {
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
        std,
        roundtrip_pct: r.roundtrip_pct,
        ecdf,
    }
}

/// Build a byte distribution from an ascending-sorted sample (empty -> zeros).
/// Thin wrapper over the shared [`stats::dist_from`].
fn dist_from(sorted: &[f64]) -> MemDist {
    stats::dist_from(sorted)
}

/// Per-parser memory distributions for a dialect, read from `target/mem_dist`.
/// Parsers with no measured file (e.g. the `libpg_query` bindings) are omitted.
fn mem_for(dir: &str, parsers: &[BenchParser]) -> Vec<ParserMem> {
    let mut out = Vec::new();
    for p in parsers {
        let name = p.name();
        let peak = bench_dist::load_mem(dir, name, "peak");
        if peak.is_empty() {
            continue;
        }
        let retained = bench_dist::load_mem(dir, name, "retained");
        out.push(ParserMem {
            parser: name.to_string(),
            n: peak.len(),
            peak: dist_from(&peak),
            retained: dist_from(&retained),
        });
    }
    out
}

/// One row of the batch time summary (`batch_dist/summary.csv`):
/// `dialect,parser,n_accepted,n_parsed,batch_bytes,batch_ns,ns_per_stmt`.
struct BatchPerfRow {
    dialect: String,
    parser: String,
    n_accepted: usize,
    n_parsed: usize,
    ns_per_stmt: f64,
}

/// One row of the batch memory summary (`batch_mem_dist/summary.csv`):
/// `dialect,parser,n_accepted,n_parsed,peak_bytes,retained_bytes,peak_per_stmt,retained_per_stmt`.
struct BatchMemRow {
    dialect: String,
    parser: String,
    n_accepted: usize,
    n_parsed: usize,
    peak_per_stmt: f64,
    retained_per_stmt: f64,
}

/// Whether a batch parse consumed the whole accepted set, so its normalized cost
/// can be trusted. A fail-fast parser that errors partway yields `n_parsed`
/// below `n_accepted`; statements with internal `;` only push `n_parsed` higher,
/// so `>=` is the right "fully consumed" test.
const fn batch_complete(n_parsed: usize, n_accepted: usize) -> bool {
    n_accepted > 0 && n_parsed >= n_accepted
}

fn parse_batch_perf(content: &str) -> Vec<BatchPerfRow> {
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 7 {
                return None;
            }
            Some(BatchPerfRow {
                dialect: f[0].to_string(),
                parser: f[1].to_string(),
                n_accepted: f[2].trim().parse().ok()?,
                n_parsed: f[3].trim().parse().ok()?,
                ns_per_stmt: f[6].trim().parse().ok()?,
            })
        })
        .collect()
}

fn parse_batch_mem(content: &str) -> Vec<BatchMemRow> {
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 8 {
                return None;
            }
            Some(BatchMemRow {
                dialect: f[0].to_string(),
                parser: f[1].to_string(),
                n_accepted: f[2].trim().parse().ok()?,
                n_parsed: f[3].trim().parse().ok()?,
                peak_per_stmt: f[6].trim().parse().ok()?,
                retained_per_stmt: f[7].trim().parse().ok()?,
            })
        })
        .collect()
}

fn read_batch_perf() -> Vec<BatchPerfRow> {
    let path = format!("{}/summary.csv", bench_dist::BATCH_DIST_DIR);
    std::fs::read_to_string(path).map_or_else(|_| Vec::new(), |c| parse_batch_perf(&c))
}

fn read_batch_mem() -> Vec<BatchMemRow> {
    let path = format!("{}/summary.csv", bench_dist::BATCH_MEM_DIR);
    std::fs::read_to_string(path).map_or_else(|_| Vec::new(), |c| parse_batch_mem(&c))
}

/// Merge batch time and batch memory rows for one dialect into per-parser
/// `ParserBatch`. A parser appears only if at least one axis parsed the whole
/// accepted set (see [`batch_complete`]); an axis whose batch bailed out early
/// is dropped to `None` so the explorer never shows a misleading number. Pure,
/// so the merge and the guard are testable.
fn batch_for(dir: &str, perf: &[BatchPerfRow], mem: &[BatchMemRow]) -> Vec<ParserBatch> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<&str, ParserBatch> = BTreeMap::new();
    let blank = |parser: &str, n: usize| ParserBatch {
        parser: parser.to_string(),
        n_accepted: n,
        ns_per_stmt: None,
        peak_per_stmt: None,
        retained_per_stmt: None,
    };
    for r in perf.iter().filter(|r| r.dialect == dir) {
        let e = map
            .entry(r.parser.as_str())
            .or_insert_with(|| blank(&r.parser, r.n_accepted));
        e.n_accepted = r.n_accepted;
        if batch_complete(r.n_parsed, r.n_accepted) {
            e.ns_per_stmt = Some(r.ns_per_stmt);
        }
    }
    for r in mem.iter().filter(|r| r.dialect == dir) {
        let e = map
            .entry(r.parser.as_str())
            .or_insert_with(|| blank(&r.parser, r.n_accepted));
        if batch_complete(r.n_parsed, r.n_accepted) {
            e.peak_per_stmt = Some(r.peak_per_stmt);
            e.retained_per_stmt = Some(r.retained_per_stmt);
        }
    }
    // Drop parsers whose every axis was incomplete (nothing trustworthy to show).
    map.into_values()
        .filter(|b| {
            b.ns_per_stmt.is_some() || b.peak_per_stmt.is_some() || b.retained_per_stmt.is_some()
        })
        .collect()
}

fn coverage_for(dialect: Dialect, all_parsers: &[&dyn Parser]) -> CoverageMatrix {
    let (parsers, files) = report::coverage_dialect(dialect, all_parsers);
    let cols: Vec<String> = parsers.iter().map(|p| p.family.to_string()).collect();
    build_coverage_matrix(cols, &files)
}

/// Assemble a `CoverageMatrix` from the column names and per-file counts,
/// computing the column subtotals. Pure, so the subtotal math is testable.
fn build_coverage_matrix(cols: Vec<String>, files: &[report::FileCoverage]) -> CoverageMatrix {
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

/// Collect each parser's rejected statements for `dir`, write the full set
/// (capped at [`FAIL_CAP`]) to `web/static/failures/{dir}__{parser}.tsv.zst`,
/// and return the per-parser previews + download paths for the JSON bundle.
///
/// The TSV has a header and one statement per row, with embedded tabs/newlines
/// escaped so each statement stays on a single line.
fn failures_for(dir: &str, parsers: &[&dyn Parser]) -> Vec<ParserFailures> {
    let Some(dialect) = Dialect::from_dir_name(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for f in report::failures_dialect(dialect, parsers) {
        let name = f.parser.family;
        if f.rejected.is_empty() {
            out.push(ParserFailures {
                parser: name.to_string(),
                rejected_total: 0,
                expected_total: f.total,
                preview_html: Vec::new(),
                preview_reasons: Vec::new(),
                download: None,
            });
            continue;
        }
        let preview = f
            .rejected
            .iter()
            .take(FAIL_PREVIEW)
            .map(|s| highlight_sql(s))
            .collect::<Vec<_>>();
        let preview_reasons = f
            .reasons
            .iter()
            .take(FAIL_PREVIEW)
            .cloned()
            .collect::<Vec<_>>();
        let file = format!("{dir}__{}.tsv.zst", stats::slug(name));
        match write_failure_tsv(&file, &f.rejected, &f.reasons) {
            Ok(()) => out.push(ParserFailures {
                parser: name.to_string(),
                rejected_total: f.rejected.len(),
                expected_total: f.total,
                preview_html: preview,
                preview_reasons,
                download: Some(format!("failures/{file}")),
            }),
            Err(e) => {
                eprintln!("warning: could not write failures/{file}: {e}");
                out.push(ParserFailures {
                    parser: name.to_string(),
                    rejected_total: f.rejected.len(),
                    expected_total: f.total,
                    preview_html: preview,
                    preview_reasons,
                    download: None,
                });
            }
        }
    }
    out
}

/// The shared syntax and theme sets, loaded once. Loading the bundled defaults
/// parses a compressed dump, so it is done lazily and reused across statements.
fn highlight_assets() -> &'static (syntect::parsing::SyntaxSet, syntect::highlighting::Theme) {
    use std::sync::OnceLock;
    use syntect::highlighting::ThemeSet;
    use syntect::parsing::SyntaxSet;
    static ASSETS: OnceLock<(SyntaxSet, syntect::highlighting::Theme)> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let ss = SyntaxSet::load_defaults_newlines();
        let theme = ThemeSet::load_defaults().themes["InspiredGitHub"].clone();
        (ss, theme)
    })
}

/// Render one SQL statement to static syntax-highlighted HTML (inner content of
/// a `<pre>`), so the web viewer ships no runtime highlighter. Colors are inlined
/// per span with no background, letting the page's own `.fail-code` style frame
/// it. On any highlighting error the escaped plain text is returned.
fn highlight_sql(sql: &str) -> String {
    use syntect::easy::HighlightLines;
    use syntect::html::{styled_line_to_highlighted_html, IncludeBackground};
    use syntect::util::LinesWithEndings;

    let (ss, theme) = highlight_assets();
    let syntax = ss
        .find_syntax_by_token("sql")
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut h = HighlightLines::new(syntax, theme);
    let mut out = String::new();
    for line in LinesWithEndings::from(sql) {
        let Ok(ranges) = h.highlight_line(line, ss) else {
            return html_escape(sql);
        };
        match styled_line_to_highlighted_html(&ranges, IncludeBackground::No) {
            Ok(html) => out.push_str(&html),
            Err(_) => return html_escape(sql),
        }
    }
    out
}

/// Minimal HTML escaping for the plain-text fallback.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Write up to [`FAIL_CAP`] rejected statements, each with the parser's error
/// message, to a zstd-compressed two-column TSV under [`FAILURES_DIR`]. Tabs and
/// newlines are escaped to keep one statement per row.
fn write_failure_tsv(file: &str, rejected: &[String], reasons: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(FAILURES_DIR)?;
    let path = Path::new(FAILURES_DIR).join(file);
    let tsv = format_failure_tsv(rejected, reasons, FAIL_CAP);

    let raw = std::fs::File::create(&path)?;
    let mut enc = zstd::stream::Encoder::new(raw, 19)?;
    std::io::Write::write_all(&mut enc, tsv.as_bytes())?;
    enc.finish()?;
    Ok(())
}

/// Build the TSV body for a failure download: a `statement\treason` header then
/// up to `cap` rows. Each row is the rejected statement, a tab, then the parser's
/// error message, with backslashes, tabs, and newlines escaped in both columns so
/// every row stays on a single line. `reasons` is aligned with `rejected`; a
/// missing reason is written as an empty cell.
fn format_failure_tsv(rejected: &[String], reasons: &[String], cap: usize) -> String {
    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
    }
    let mut tsv = String::from("statement\treason\n");
    for (i, s) in rejected.iter().take(cap).enumerate() {
        tsv.push_str(&escape(s));
        tsv.push('\t');
        tsv.push_str(&escape(reasons.get(i).map_or("", String::as_str)));
        tsv.push('\n');
    }
    tsv
}

/// Grade every dialect, gather perf + coverage, and write `web/assets/bench.json.zst`.
///
/// # Errors
/// Returns an error if serialization or writing the output file fails.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let parsers = BenchParser::all();
    // The grading/coverage/failure functions are generic over `&dyn Parser`.
    let dyn_parsers: Vec<&dyn Parser> = parsers.iter().map(|p| p as &dyn Parser).collect();
    let summary = read_summary();
    if summary.is_empty() {
        eprintln!(
            "warning: no {}/summary.csv; perf charts will be empty. Run `cargo bench` first.",
            bench_dist::DIST_DIR
        );
    }
    let batch_perf = read_batch_perf();
    let batch_mem = read_batch_mem();
    if batch_perf.is_empty() && batch_mem.is_empty() {
        eprintln!(
            "note: no batch summaries in {} / {}; batch columns will be empty. Run `cargo bench --bench batch_parsing` and `cargo run --release -p membench -- batch`.",
            bench_dist::BATCH_DIST_DIR,
            bench_dist::BATCH_MEM_DIR
        );
    }

    let mut dialects = Vec::new();
    for &d in &ORDER {
        let Some(report) = report::grade_dialect(d, &dyn_parsers) else {
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
            coverage: coverage_for(d, &dyn_parsers),
            failures: failures_for(d.dir_name(), &dyn_parsers),
            memory: mem_for(d.dir_name(), &parsers),
            batch: batch_for(d.dir_name(), &batch_perf, &batch_mem),
        });
    }

    let bundle = Bundle {
        generated_utc: now_utc(),
        git_commit: git_short(),
        parsers: parsers.iter().map(|p| p.name().to_string()).collect(),
        dialects,
    };

    // Compact JSON, zstd-compressed: the viewer embeds and decompresses it in
    // wasm (~25x smaller than raw, no runtime fetch).
    let json = serde_json::to_vec(&bundle)?;
    let compressed = zstd::stream::encode_all(json.as_slice(), 19)?;
    let out = Path::new(OUT);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, &compressed)?;
    println!(
        "Wrote {OUT} ({} dialects, {} KB compressed from {} KB)",
        bundle.dialects.len(),
        compressed.len() / 1024,
        json.len() / 1024,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        batch_complete, batch_for, build_coverage_matrix, format_failure_tsv, git_short, metrics,
        now_utc, parse_batch_mem, parse_batch_perf, parse_summary, pct, perf_row_to_perf, PerfRow,
    };
    use crate::datasets::Dialect;
    use crate::report::{DialectReport, FileCoverage};
    use crate::{BenchParser, Parser};

    fn perf_row(parser: &str) -> PerfRow {
        PerfRow {
            dialect: "postgresql".to_string(),
            parser: parser.to_string(),
            n_total: 10,
            n_accepted: 8,
            pct: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0],
            roundtrip_pct: Some(50.0),
        }
    }

    #[test]
    fn metrics_reference_dialect_sets_recall_and_fp() {
        let sp = BenchParser::Sqlparser;
        let parsers: [&dyn Parser; 1] = [&sp];
        let mut report = DialectReport::empty(Dialect::Postgresql, &parsers);
        report.has_reference = true; // exercise the reference metrics path directly
        report.valid_total = 10;
        report.invalid_total = 4;
        report.stats[0].accepted_valid = 8;
        report.stats[0].accepted_invalid = 1;
        let m = metrics(&report);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].recall_pct, Some(80.0));
        assert_eq!(m[0].false_positive_pct, Some(25.0));
        assert_eq!(m[0].accept_pct, None); // None on reference dialects
    }

    #[test]
    fn metrics_provenance_dialect_sets_accept_only() {
        let sp = BenchParser::Sqlparser;
        let parsers: [&dyn Parser; 1] = [&sp];
        let mut report = DialectReport::empty(Dialect::Multi, &parsers);
        report.has_reference = false; // provenance dialect: accept-rate only
        report.valid_total = 4;
        report.stats[0].accepted_valid = 3;
        let m = metrics(&report);
        assert_eq!(m[0].recall_pct, None);
        assert_eq!(m[0].false_positive_pct, None);
        assert_eq!(m[0].accept_pct, Some(75.0));
    }

    #[test]
    fn perf_row_maps_percentile_columns_in_order() {
        let p = perf_row_to_perf(&perf_row("sqlparser-rs"), 1.5, vec![[1.0, 0.5]]);
        assert_eq!(p.parser, "sqlparser-rs");
        assert_eq!(p.n_total, 10);
        assert!((p.min - 1.0).abs() < 1e-9);
        assert!((p.median - 4.0).abs() < 1e-9);
        assert!((p.p99 - 7.0).abs() < 1e-9);
        assert!((p.mean - 9.0).abs() < 1e-9);
        assert_eq!(p.ecdf.len(), 1);
    }

    #[test]
    fn coverage_matrix_sums_column_subtotals() {
        let files = vec![
            FileCoverage {
                name: "a.txt".to_string(),
                total: 10,
                accepted: vec![8, 6],
            },
            FileCoverage {
                name: "b.txt".to_string(),
                total: 5,
                accepted: vec![5, 1],
            },
        ];
        let cm = build_coverage_matrix(vec!["p1".to_string(), "p2".to_string()], &files);
        assert_eq!(cm.subtotal_total, 15);
        assert_eq!(cm.subtotal_accepted, vec![13, 7]);
        assert_eq!(cm.files.len(), 2);
    }

    #[test]
    fn now_utc_is_nonempty_iso_or_unix() {
        let s = now_utc();
        assert!(!s.is_empty());
        // Either an ISO Z timestamp or the unix: fallback.
        assert!(s.ends_with('Z') || s.starts_with("unix:"));
    }

    #[test]
    fn git_short_runs_without_panicking() {
        // In the repo it returns Some(hash); the point is it does not panic and
        // yields a non-empty string when present.
        if let Some(h) = git_short() {
            assert!(!h.is_empty());
        }
    }

    #[test]
    fn summary_parses_rows_and_skips_short_lines() {
        let csv = "dialect,parser,n_total,n_accepted,min,p10,p25,median,p75,p90,p99,max,mean,rt\n\
                   postgresql,sqlparser-rs,100,80,1,2,3,4,5,6,7,8,9,99.5\n\
                   too,short,row\n";
        let rows = parse_summary(csv);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.dialect, "postgresql");
        assert_eq!(r.parser, "sqlparser-rs");
        assert_eq!(r.n_total, 100);
        assert_eq!(r.n_accepted, 80);
        assert!((r.pct[3] - 4.0).abs() < 1e-9); // median column
        assert!(r.roundtrip_pct.is_some_and(|v| (v - 99.5).abs() < 1e-9));
    }

    #[test]
    fn summary_negative_roundtrip_is_none() {
        let csv = "h,h,h,h,h,h,h,h,h,h,h,h,h,h\n\
                   mysql,qusql-parse,10,5,1,1,1,1,1,1,1,1,1,-1\n";
        let rows = parse_summary(csv);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].roundtrip_pct, None);
    }

    #[test]
    fn pct_is_none_for_zero_base() {
        assert_eq!(pct(3, 0), None);
        assert_eq!(pct(0, 10), Some(0.0));
        assert_eq!(pct(1, 4), Some(25.0));
    }

    #[test]
    fn tsv_has_header_and_one_row_per_statement() {
        let rows = vec!["SELECT 1".to_string(), "SELECT 2".to_string()];
        let reasons = vec!["boom".to_string(), "bang".to_string()];
        let tsv = format_failure_tsv(&rows, &reasons, 1000);
        let lines: Vec<&str> = tsv.lines().collect();
        assert_eq!(lines[0], "statement\treason");
        assert_eq!(lines.len(), 3); // header + 2 rows
        assert_eq!(lines[1], "SELECT 1\tboom");
    }

    #[test]
    fn tsv_escapes_tabs_newlines_backslashes_in_both_columns() {
        let rows = vec!["a\tb\nc\\d".to_string()];
        let reasons = vec!["e\tf".to_string()];
        let tsv = format_failure_tsv(&rows, &reasons, 1000);
        // Statement and reason both stay on a single line with escapes.
        assert_eq!(tsv, "statement\treason\na\\tb\\nc\\\\d\te\\tf\n");
    }

    #[test]
    fn tsv_writes_empty_cell_for_a_missing_reason() {
        let rows = vec!["SELECT 1".to_string()];
        let tsv = format_failure_tsv(&rows, &[], 1000);
        assert_eq!(tsv, "statement\treason\nSELECT 1\t\n");
    }

    #[test]
    fn batch_perf_parses_and_skips_short_lines() {
        let csv = "dialect,parser,n_accepted,n_parsed,batch_bytes,batch_ns,ns_per_stmt\n\
                   postgresql,sqlparser-rs,100,100,5000,400000.0,4000.0\n\
                   short,row\n";
        let rows = parse_batch_perf(csv);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].parser, "sqlparser-rs");
        assert_eq!(rows[0].n_accepted, 100);
        assert_eq!(rows[0].n_parsed, 100);
        assert!((rows[0].ns_per_stmt - 4000.0).abs() < 1e-9);
    }

    #[test]
    fn batch_mem_parses_peak_and_retained_columns() {
        let csv = "dialect,parser,n_accepted,n_parsed,peak_bytes,retained_bytes,peak_per_stmt,retained_per_stmt\n\
                   sqlite,turso_parser,50,50,100000,40000,2000.0,800.0\n";
        let rows = parse_batch_mem(csv);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].n_parsed, 50);
        assert!((rows[0].peak_per_stmt - 2000.0).abs() < 1e-9);
        assert!((rows[0].retained_per_stmt - 800.0).abs() < 1e-9);
    }

    #[test]
    fn batch_complete_requires_full_consumption() {
        assert!(batch_complete(10, 10)); // exactly consumed
        assert!(batch_complete(12, 10)); // internal-semicolon inflation is fine
        assert!(!batch_complete(9, 10)); // bailed out early
        assert!(!batch_complete(0, 0)); // nothing accepted
    }

    #[test]
    fn batch_merge_combines_time_and_memory_and_filters_by_dialect() {
        let perf = parse_batch_perf(
            "h,h,h,h,h,h,h\n\
             postgresql,sqlparser-rs,10,10,1,100.0,10.0\n\
             postgresql,pg_query.rs,10,10,1,80.0,8.0\n",
        );
        let mem = parse_batch_mem(
            "h,h,h,h,h,h,h,h\n\
             postgresql,sqlparser-rs,10,10,1,1,500.0,200.0\n",
        );
        let merged = batch_for("postgresql", &perf, &mem);
        assert_eq!(merged.len(), 2);
        let sp = merged.iter().find(|x| x.parser == "sqlparser-rs").unwrap();
        assert_eq!(sp.ns_per_stmt, Some(10.0));
        assert_eq!(sp.peak_per_stmt, Some(500.0));
        assert_eq!(sp.retained_per_stmt, Some(200.0));
        // pg_query has batch time but no Rust-visible batch memory.
        let pg = merged.iter().find(|x| x.parser == "pg_query.rs").unwrap();
        assert_eq!(pg.ns_per_stmt, Some(8.0));
        assert_eq!(pg.peak_per_stmt, None);
        // A different dialect yields nothing from the same rows.
        assert!(batch_for("sqlite", &perf, &mem).is_empty());
    }

    #[test]
    fn batch_merge_drops_incomplete_parses() {
        // Time bailed out early (5 of 10): its ns_per_stmt is untrustworthy and
        // dropped. Memory parsed fully, so it survives and keeps the entry.
        let perf = parse_batch_perf("h,h,h,h,h,h,h\npostgresql,sqlparser-rs,10,5,1,100.0,10.0\n");
        let mem =
            parse_batch_mem("h,h,h,h,h,h,h,h\npostgresql,sqlparser-rs,10,10,1,1,500.0,200.0\n");
        let merged = batch_for("postgresql", &perf, &mem);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].ns_per_stmt, None); // dropped: incomplete
        assert_eq!(merged[0].peak_per_stmt, Some(500.0));

        // Both axes incomplete -> the parser is omitted entirely.
        let perf2 = parse_batch_perf("h,h,h,h,h,h,h\npostgresql,sqlparser-rs,10,2,1,100.0,10.0\n");
        let mem2 =
            parse_batch_mem("h,h,h,h,h,h,h,h\npostgresql,sqlparser-rs,10,3,1,1,500.0,200.0\n");
        assert!(batch_for("postgresql", &perf2, &mem2).is_empty());
    }

    #[test]
    fn tsv_respects_the_cap() {
        let rows: Vec<String> = (0..2000).map(|i| format!("SELECT {i}")).collect();
        let reasons: Vec<String> = (0..2000).map(|i| format!("err {i}")).collect();
        let tsv = format_failure_tsv(&rows, &reasons, 1000);
        // header + cap rows
        assert_eq!(tsv.lines().count(), 1001);
    }
}

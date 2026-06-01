#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

//! Renders the benchmark charts from the raw per-statement timings written by
//! the benchmark to `target/bench_dist/`.
//!
//! [`render`] produces two views, both one subplot per dialect with consistent
//! parser colors. Each subplot has its own legend listing only that dialect's
//! parsers, annotated with two quality metrics: `fail%` (share of the dialect
//! corpus the parser rejected) and `RT%` (Display round-trip rate among accepted
//! statements, "n/a" without a printer). The subplot title carries the proper
//! dialect name and total statement count. `benchmark_results.svg` is an
//! empirical CDF (eCDF) per parser (x = per-statement ns log, y = fraction of
//! accepted statements parsed within t). `benchmark_results_boxplot.svg` is a
//! box per parser (p25/median/p75, whiskers p10/p90), log-y.

use crate::datasets::Dialect;
use crate::stats::{ecdf_points, quantile, slug};
use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};
use plotters::style::RGBColor;
use std::fs;
use std::path::Path;

/// Directory the benchmark writes its raw timings and `summary.csv` to.
pub const DIST_DIR: &str = "target/bench_dist";
const ECDF_FILE: &str = "benchmark_results.svg";
const BOX_FILE: &str = "benchmark_results_boxplot.svg";

/// Per-subplot cell size (px) and the x offset within a cell that splits the
/// chart (left) from its legend (right).
const CELL_W: u32 = 620;
const CELL_H: u32 = 340;
const LEGEND_SPLIT: i32 = 420;

const DIALECT_ORDER: [Dialect; 13] = [
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

/// Parser color, shared with the web viewer via the `viz` palette.
const fn parser_color(name: &str) -> RGBColor {
    let (r, g, b) = viz::parser_rgb(name);
    RGBColor(r, g, b)
}

/// A parser's per-statement timing data and quality metrics within one dialect.
struct Series {
    parser: String,
    times: Vec<f64>, // sorted ascending, ns
    fail_pct: f64,   // % of the dialect corpus this parser did not accept
    rt_pct: f64,     // Display round-trip rate among accepted; < 0 => N/A
}

/// One dialect's subplot data: the dialect, its total corpus size, and a series
/// per parser.
struct Group {
    dialect: Dialect,
    total: usize,
    series: Vec<Series>,
}
type Svg = DrawingArea<SVGBackend<'static>, plotters::coord::Shift>;

/// Per-(dialect, parser) row parsed from summary.csv.
struct SummaryRow {
    dialect: String,
    parser: String,
    n_total: usize,
    n_accepted: usize,
    rt_pct: f64,
}

/// Rows from summary.csv (parsers that accepted nothing are dropped). The
/// `roundtrip_pct` column is the last (14th) field, and a row missing it reads as
/// N/A.
fn load_summary(path: &Path) -> Vec<SummaryRow> {
    let Ok(content) = fs::read_to_string(path) else {
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
            let n_total: usize = f[2].trim().parse().ok()?;
            let n_accepted: usize = f[3].trim().parse().ok()?;
            if n_accepted == 0 {
                return None;
            }
            let rt_pct: f64 = f
                .get(13)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(-1.0);
            Some(SummaryRow {
                dialect: f[0].to_string(),
                parser: f[1].to_string(),
                n_total,
                n_accepted,
                rt_pct,
            })
        })
        .collect()
}

/// Sorted ascending per-statement times (ns) for one (dialect, parser) from the
/// raw `target/bench_dist/{dialect}__{slug}.txt` file (empty if absent).
#[must_use]
pub fn load_times(dialect: &str, parser: &str) -> Vec<f64> {
    let path = format!("{DIST_DIR}/{dialect}__{}.txt", slug(parser));
    fs::read_to_string(path)
        .map(|c| parse_times(&c))
        .unwrap_or_default()
}

/// Parse one-value-per-line ns timings: drop blanks/unparsable/non-positive,
/// return ascending-sorted.
fn parse_times(content: &str) -> Vec<f64> {
    let mut v: Vec<f64> = content
        .lines()
        .filter_map(|l| l.trim().parse::<f64>().ok())
        .filter(|x| *x > 0.0)
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// Group `n` with commas (e.g. 92268 -> "92,268") for chart titles.
fn commas(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Per-subplot legend listing only the parsers in this dialect, each with its
/// color, name, and the two quality metrics (fail% = share of the corpus the
/// parser rejected, RT% = Display round-trip rate among accepted statements, or
/// "n/a" for parsers without a pretty-printer). Drawn at pixel coordinates in
/// the legend sub-area to the right of each chart.
fn draw_subplot_legend(area: &Svg, series: &[Series]) -> Result<(), Box<dyn std::error::Error>> {
    let line_h = 30;
    let start_y = 16;
    let grey = RGBColor(90, 90, 90);
    for (i, s) in series.iter().enumerate() {
        let y = start_y + i as i32 * line_h;
        area.draw(&PathElement::new(
            vec![(6, y + 6), (30, y + 6)],
            parser_color(&s.parser).stroke_width(3),
        ))?;
        area.draw(&Text::new(
            s.parser.clone(),
            (34, y),
            ("sans-serif", 11).into_font(),
        ))?;
        let rt = if s.rt_pct < 0.0 {
            "RT n/a".to_string()
        } else {
            format!("RT {:.0}%", s.rt_pct)
        };
        // Only a parser that accepted nothing should read as 100% fail. Otherwise
        // a value that rounds up to 100 (e.g. orql 99.68%) is shown as ">99" so it
        // is not mistaken for total failure beside its drawn curve.
        let fail = if s.fail_pct >= 100.0 {
            "100".to_string()
        } else if s.fail_pct.round() >= 100.0 {
            ">99".to_string()
        } else {
            format!("{:.0}", s.fail_pct)
        };
        area.draw(&Text::new(
            format!("fail {fail}%   {rt}"),
            (34, y + 14),
            ("sans-serif", 10).into_font().color(&grey),
        ))?;
    }
    Ok(())
}

fn render_ecdf(groups: &[Group]) -> Result<(), Box<dyn std::error::Error>> {
    // X range (ns, log): global min to a per-series p99, so rare multi-ms
    // outliers do not stretch the axis.
    let mut xmin = f64::MAX;
    let mut xmax = 0.0_f64;
    for g in groups {
        for s in &g.series {
            xmin = xmin.min(s.times[0]);
            xmax = xmax.max(quantile(&s.times, 0.99));
        }
    }
    let xmin = (xmin * 0.8).max(1.0);
    let xmax = xmax * 1.3;

    let (root, areas) = grid(ECDF_FILE, groups.len(),
        "Per-statement parse-time eCDF by dialect (x = ns/statement, log; y = fraction of accepted statements). Legend: fail% = corpus rejected, RT% = Display round-trip among accepted.")?;

    for (idx, g) in groups.iter().enumerate() {
        let (plot_area, legend_area) = areas[idx].split_horizontally(LEGEND_SPLIT);
        let mut chart = ChartBuilder::on(&plot_area)
            .caption(
                format!("{} (n={})", g.dialect.display_name(), commas(g.total)),
                ("sans-serif", 15),
            )
            .margin(8)
            .x_label_area_size(30)
            .y_label_area_size(40)
            .build_cartesian_2d((xmin..xmax).log_scale(), 0f64..1.02f64)?;
        chart
            .configure_mesh()
            .x_desc("ns / statement")
            .y_desc("frac <= t")
            .x_label_style(("sans-serif", 10))
            .y_label_style(("sans-serif", 10))
            .draw()?;
        for s in &g.series {
            let color = parser_color(&s.parser);
            chart.draw_series(LineSeries::new(
                ecdf_points(&s.times, 300),
                color.stroke_width(2),
            ))?;
        }
        draw_subplot_legend(&legend_area, &g.series)?;
    }
    root.present()?;
    Ok(())
}

fn render_box(groups: &[Group]) -> Result<(), Box<dyn std::error::Error>> {
    // Y range (ns, log) from p10..p90.
    let mut ymin = f64::MAX;
    let mut ymax = 0.0_f64;
    for g in groups {
        for s in &g.series {
            ymin = ymin.min(quantile(&s.times, 0.10));
            ymax = ymax.max(quantile(&s.times, 0.90));
        }
    }
    let ymin = (ymin * 0.7).max(1.0);
    let ymax = ymax * 1.5;

    let (root, areas) = grid(BOX_FILE, groups.len(),
        "Per-statement parse time by dialect (box = p25/median/p75, whiskers p10/p90). Legend: fail% = corpus rejected, RT% = Display round-trip among accepted.")?;

    for (idx, g) in groups.iter().enumerate() {
        let (plot_area, legend_area) = areas[idx].split_horizontally(LEGEND_SPLIT);
        let series = &g.series;
        let cnt = series.len();
        let mut chart = ChartBuilder::on(&plot_area)
            .caption(
                format!("{} (n={})", g.dialect.display_name(), commas(g.total)),
                ("sans-serif", 15),
            )
            .margin(8)
            .x_label_area_size(18)
            .y_label_area_size(52)
            .build_cartesian_2d(0f64..(cnt as f64), (ymin..ymax).log_scale())?;
        chart
            .configure_mesh()
            .disable_x_mesh()
            .x_labels(0)
            .y_desc("ns / statement")
            .y_label_style(("sans-serif", 10))
            .draw()?;

        for (i, s) in series.iter().enumerate() {
            let x = i as f64 + 0.5;
            let (l, r) = (x - 0.32, x + 0.32);
            let color = parser_color(&s.parser);
            let (p10, p25, med, p75, p90) = (
                quantile(&s.times, 0.10),
                quantile(&s.times, 0.25),
                quantile(&s.times, 0.50),
                quantile(&s.times, 0.75),
                quantile(&s.times, 0.90),
            );
            chart.draw_series(std::iter::once(Rectangle::new(
                [(l, p25), (r, p75)],
                color.mix(0.45).filled(),
            )))?;
            chart.draw_series(std::iter::once(Rectangle::new(
                [(l, p25), (r, p75)],
                color.stroke_width(1),
            )))?;
            chart.draw_series(std::iter::once(PathElement::new(
                vec![(l, med), (r, med)],
                color.stroke_width(2),
            )))?;
            for (a, b) in [(p10, p25), (p75, p90)] {
                chart.draw_series(std::iter::once(PathElement::new(
                    vec![(x, a), (x, b)],
                    color.stroke_width(1),
                )))?;
            }
            for y in [p10, p90] {
                chart.draw_series(std::iter::once(PathElement::new(
                    vec![(x - 0.15, y), (x + 0.15, y)],
                    color.stroke_width(1),
                )))?;
            }
        }
        draw_subplot_legend(&legend_area, series)?;
    }
    root.present()?;
    Ok(())
}

/// Build the SVG grid (one cell per dialect) and title.
fn grid(
    file: &'static str,
    num_plots: usize,
    title: &str,
) -> Result<(Svg, Vec<Svg>), Box<dyn std::error::Error>> {
    let cols = 3usize;
    let rows_n = num_plots.div_ceil(cols);
    let width = cols as u32 * CELL_W;
    let height = rows_n as u32 * CELL_H + 70;
    let root = SVGBackend::new(file, (width, height)).into_drawing_area();
    root.fill(&WHITE)?;
    root.draw(&Text::new(
        title.to_string(),
        (width as i32 / 2, 26),
        ("sans-serif", 14)
            .into_font()
            .color(&BLACK)
            .pos(Pos::new(HPos::Center, VPos::Center)),
    ))?;
    let areas = root.margin(45, 10, 10, 10).split_evenly((rows_n, cols));
    Ok((root, areas))
}

/// Render `benchmark_results.svg` (eCDF) and `benchmark_results_boxplot.svg`
/// (box plots) from `target/bench_dist/summary.csv` + the raw timing files.
///
/// # Errors
/// Returns an error if no benchmark data is present or SVG writing fails.
pub fn render() -> Result<(), Box<dyn std::error::Error>> {
    let summary = load_summary(&Path::new(DIST_DIR).join("summary.csv"));
    if summary.is_empty() {
        return Err(format!("no data in {DIST_DIR}/summary.csv; run `cargo bench` first").into());
    }

    let groups: Vec<Group> = DIALECT_ORDER
        .iter()
        .filter_map(|d| {
            let rows: Vec<&SummaryRow> = summary
                .iter()
                .filter(|r| r.dialect == d.dir_name())
                .collect();
            // The corpus size is the same across this dialect's parsers, so take it
            // from any row (max guards against a stray short row).
            let total = rows.iter().map(|r| r.n_total).max().unwrap_or(0);
            let mut series: Vec<Series> = rows
                .iter()
                .map(|r| Series {
                    parser: r.parser.clone(),
                    times: load_times(d.dir_name(), &r.parser),
                    fail_pct: if r.n_total == 0 {
                        0.0
                    } else {
                        100.0 * (r.n_total - r.n_accepted) as f64 / r.n_total as f64
                    },
                    rt_pct: r.rt_pct,
                })
                .filter(|s| !s.times.is_empty())
                .collect();
            series.sort_by(|a, b| a.parser.cmp(&b.parser));
            (!series.is_empty()).then_some(Group {
                dialect: *d,
                total,
                series,
            })
        })
        .collect();

    if groups.is_empty() {
        return Err(format!("no raw timing files found in {DIST_DIR}/").into());
    }

    render_ecdf(&groups)?;
    render_box(&groups)?;
    println!("Saved {ECDF_FILE} and {BOX_FILE}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{commas, load_summary, parse_times};
    use std::fs;
    use std::io::Write as _;

    #[test]
    fn parse_times_drops_junk_and_sorts() {
        let v = parse_times("3.0\n1.5\n\n  \nbad\n-2.0\n0\n2.0\n");
        assert_eq!(v, vec![1.5, 2.0, 3.0]);
    }

    #[test]
    fn commas_groups_thousands() {
        assert_eq!(commas(0), "0");
        assert_eq!(commas(42), "42");
        assert_eq!(commas(1000), "1,000");
        assert_eq!(commas(92268), "92,268");
        assert_eq!(commas(1_234_567), "1,234,567");
    }

    #[test]
    fn load_summary_parses_rows_skips_zero_and_short_lines() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sqlbench_summary_{}_{nanos}.csv",
            std::process::id()
        ));
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "dialect,parser,n_total,n_accepted,min_ns,p10_ns,p25_ns,median_ns,p75_ns,p90_ns,p99_ns,max_ns,mean_ns,roundtrip_pct").unwrap();
        writeln!(f, "postgresql,sqlparser-rs,100,80,1,2,3,4,5,6,7,8,9,95.0").unwrap();
        writeln!(f, "mysql,qusql-parse,50,40,1,2,3,4,5,6,7,8,9").unwrap(); // no rt col -> N/A
        writeln!(f, "mysql,orql,10,0,0,0,0,0,0,0,0,0,0,0").unwrap(); // n_accepted=0 -> skipped
        writeln!(f, "too,short,line").unwrap(); // < 13 columns -> skipped
        drop(f);

        let rows = load_summary(&path);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].dialect, "postgresql");
        assert_eq!(rows[0].parser, "sqlparser-rs");
        assert_eq!(rows[0].n_total, 100);
        assert_eq!(rows[0].n_accepted, 80);
        assert!((rows[0].rt_pct - 95.0).abs() < f64::EPSILON);
        // Row without the roundtrip column reads as N/A (-1).
        assert!(rows[1].rt_pct < 0.0);
        let _ = fs::remove_file(&path);
    }
}

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

//! Renders the benchmark charts from the raw per-statement timings written by
//! `cargo bench` to `target/bench_dist/`.
//!
//! Produces two views, both one subplot per dialect with consistent parser
//! colors and a legend cell. `benchmark_results.svg` is an empirical CDF (eCDF)
//! per parser (x = per-statement ns log, y = fraction of accepted statements
//! parsed within t). `benchmark_results_boxplot.svg` is a box per parser
//! (p25/median/p75, whiskers p10/p90), log-y per-statement time. In both, a
//! triangle / black tick marks the concatenated-body time normalized by
//! statement count (`concat/n`).

use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};
use plotters::style::RGBColor;
use sql_ast_benchmark::datasets::Dialect;
use std::fs;
use std::path::Path;

const DIST_DIR: &str = "target/bench_dist";
const ECDF_FILE: &str = "benchmark_results.svg";
const BOX_FILE: &str = "benchmark_results_boxplot.svg";

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

fn parser_color(name: &str) -> RGBColor {
    match name {
        "sqlparser-rs" => RGBColor(15, 76, 129),
        "pg_query.rs" => RGBColor(255, 111, 97),
        "pg_query (summary)" => RGBColor(214, 153, 150),
        "polyglot-sql" => RGBColor(230, 200, 40),
        "qusql-parse" => RGBColor(95, 75, 139),
        "databend-common-ast" => RGBColor(0, 155, 119),
        "sqlglot-rust" => RGBColor(237, 135, 45),
        "sqlite3-parser" => RGBColor(0, 128, 128),
        "orql" => RGBColor(139, 69, 19),
        _ => RGBColor(120, 120, 120),
    }
}

/// Same slug as the benchmark uses for raw-file names.
fn slug(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// A parser's per-statement timing data within one dialect.
struct Series {
    parser: String,
    times: Vec<f64>, // sorted ascending, ns
    concat: f64,     // concatenated-normalized, ns
}

impl Series {
    fn quantile(&self, q: f64) -> f64 {
        let n = self.times.len();
        if n == 0 {
            return 0.0;
        }
        let idx = ((q * (n - 1) as f64).round() as usize).min(n - 1);
        self.times[idx]
    }
}

type Group = (Dialect, Vec<Series>);
type Svg = DrawingArea<SVGBackend<'static>, plotters::coord::Shift>;

/// (dialect, parser, concat/n) rows from summary.csv.
fn load_summary(path: &Path) -> Vec<(String, String, f64)> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 14 {
                return None;
            }
            let n: usize = f[3].trim().parse().ok()?;
            if n == 0 {
                return None;
            }
            let concat: f64 = f[13].trim().parse().unwrap_or(0.0);
            Some((f[0].to_string(), f[1].to_string(), concat))
        })
        .collect()
}

fn load_times(dialect: &str, parser: &str) -> Vec<f64> {
    let path = format!("{DIST_DIR}/{dialect}__{}.txt", slug(parser));
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut v: Vec<f64> = content
        .lines()
        .filter_map(|l| l.trim().parse::<f64>().ok())
        .filter(|x| *x > 0.0)
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// Up to `max_pts` (x = time, y = cumulative fraction) points tracing the eCDF.
fn ecdf_points(sorted: &[f64], max_pts: usize) -> Vec<(f64, f64)> {
    let n = sorted.len();
    if n == 0 {
        return Vec::new();
    }
    if n <= max_pts {
        return sorted
            .iter()
            .enumerate()
            .map(|(i, &t)| (t, (i + 1) as f64 / n as f64))
            .collect();
    }
    (0..=max_pts)
        .map(|k| {
            let frac = k as f64 / max_pts as f64;
            let idx = ((frac * (n - 1) as f64).round() as usize).min(n - 1);
            (sorted[idx], frac)
        })
        .collect()
}

fn draw_legend(legend: &Svg, parsers: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let line_h = 22;
    let start_y = 20;
    for (i, name) in parsers.iter().enumerate() {
        let y = start_y + i as i32 * line_h;
        legend.draw(&PathElement::new(
            vec![(12, y + 6), (40, y + 6)],
            parser_color(name).stroke_width(3),
        ))?;
        legend.draw(&Text::new(
            name.clone(),
            (46, y),
            ("sans-serif", 12).into_font(),
        ))?;
    }
    let y = start_y + parsers.len() as i32 * line_h + 8;
    legend.draw(&TriangleMarker::new((26, y + 6), 6, BLACK.filled()))?;
    legend.draw(&Text::new(
        "concatenated / n",
        (46, y),
        ("sans-serif", 12).into_font(),
    ))?;
    Ok(())
}

fn render_ecdf(groups: &[Group], parsers: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // X range (ns, log): global min to a per-series p99, so rare multi-ms
    // outliers do not stretch the axis.
    let mut xmin = f64::MAX;
    let mut xmax = 0.0_f64;
    for (_, series) in groups {
        for s in series {
            xmin = xmin.min(s.times[0]).min(s.concat.max(1.0));
            xmax = xmax.max(s.quantile(0.99)).max(s.concat);
        }
    }
    let xmin = (xmin * 0.8).max(1.0);
    let xmax = xmax * 1.3;

    let (root, areas, n) = grid(ECDF_FILE, groups.len(),
        "Per-statement parse-time eCDF by dialect (x = ns/statement, log; y = fraction of accepted statements; triangle = concatenated/n)")?;

    for (idx, (dialect, series)) in groups.iter().enumerate() {
        let mut chart = ChartBuilder::on(&areas[idx])
            .caption(dialect.dir_name(), ("sans-serif", 15))
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
        for s in series {
            let color = parser_color(&s.parser);
            chart.draw_series(LineSeries::new(
                ecdf_points(&s.times, 300),
                color.stroke_width(2),
            ))?;
            if s.concat > 0.0 {
                chart.draw_series(std::iter::once(TriangleMarker::new(
                    (s.concat, 0.0),
                    6,
                    color.filled(),
                )))?;
            }
        }
    }
    if n < areas.len() {
        draw_legend(&areas[n], parsers)?;
    }
    root.present()?;
    Ok(())
}

fn render_box(groups: &[Group], parsers: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // Y range (ns, log) from p10..max(p90, concat).
    let mut ymin = f64::MAX;
    let mut ymax = 0.0_f64;
    for (_, series) in groups {
        for s in series {
            ymin = ymin.min(s.quantile(0.10)).min(s.concat.max(1.0));
            ymax = ymax.max(s.quantile(0.90)).max(s.concat);
        }
    }
    let ymin = (ymin * 0.7).max(1.0);
    let ymax = ymax * 1.5;

    let (root, areas, n) = grid(BOX_FILE, groups.len(),
        "Per-statement parse time by dialect (box = p25/median/p75, whiskers p10/p90; black tick = concatenated/n)")?;

    for (idx, (dialect, series)) in groups.iter().enumerate() {
        let cnt = series.len();
        let mut chart = ChartBuilder::on(&areas[idx])
            .caption(dialect.dir_name(), ("sans-serif", 15))
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
                s.quantile(0.10),
                s.quantile(0.25),
                s.quantile(0.50),
                s.quantile(0.75),
                s.quantile(0.90),
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
            if s.concat > 0.0 {
                chart.draw_series(std::iter::once(PathElement::new(
                    vec![(l, s.concat), (r, s.concat)],
                    BLACK.stroke_width(2),
                )))?;
            }
        }
    }
    if n < areas.len() {
        draw_legend(&areas[n], parsers)?;
    }
    root.present()?;
    Ok(())
}

/// Build the SVG grid (one cell per dialect plus a legend cell) and title.
fn grid(
    file: &'static str,
    num_plots: usize,
    title: &str,
) -> Result<(Svg, Vec<Svg>, usize), Box<dyn std::error::Error>> {
    let cols = 4usize;
    let rows_n = (num_plots + 1).div_ceil(cols);
    let width = cols as u32 * 440;
    let height = rows_n as u32 * 330 + 70;
    let root = SVGBackend::new(file, (width, height)).into_drawing_area();
    root.fill(&WHITE)?;
    root.draw(&Text::new(
        title.to_string(),
        (width as i32 / 2, 26),
        ("sans-serif", 17)
            .into_font()
            .color(&BLACK)
            .pos(Pos::new(HPos::Center, VPos::Center)),
    ))?;
    let areas = root.margin(45, 10, 10, 10).split_evenly((rows_n, cols));
    Ok((root, areas, num_plots))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let summary = load_summary(&Path::new(DIST_DIR).join("summary.csv"));
    if summary.is_empty() {
        eprintln!("No data in {DIST_DIR}/summary.csv. Run `cargo bench` first.");
        std::process::exit(1);
    }

    let groups: Vec<Group> = DIALECT_ORDER
        .iter()
        .filter_map(|d| {
            let mut series: Vec<Series> = summary
                .iter()
                .filter(|(dia, _, _)| dia == d.dir_name())
                .map(|(_, parser, concat)| Series {
                    parser: parser.clone(),
                    times: load_times(d.dir_name(), parser),
                    concat: *concat,
                })
                .filter(|s| !s.times.is_empty())
                .collect();
            series.sort_by(|a, b| a.parser.cmp(&b.parser));
            (!series.is_empty()).then_some((*d, series))
        })
        .collect();

    if groups.is_empty() {
        eprintln!("No raw timing files found in {DIST_DIR}/.");
        std::process::exit(1);
    }

    let mut all_parsers: Vec<String> = summary.iter().map(|(_, p, _)| p.clone()).collect();
    all_parsers.sort();
    all_parsers.dedup();

    println!("Per-statement median / p90 and concatenated-normalized (ns):");
    for (d, series) in &groups {
        println!("\n{}:", d.dir_name());
        for s in series {
            println!(
                "  {:<24} n={:<6} median={:>8.0}  p90={:>9.0}  concat/n={:>8.0}",
                s.parser,
                s.times.len(),
                s.quantile(0.5),
                s.quantile(0.9),
                s.concat
            );
        }
    }

    render_ecdf(&groups, &all_parsers)?;
    render_box(&groups, &all_parsers)?;
    println!("\nSaved {ECDF_FILE} and {BOX_FILE}");
    Ok(())
}

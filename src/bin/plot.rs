#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

//! Renders `benchmark_results.svg` from the raw per-statement timings written
//! by `cargo bench` to `target/bench_dist/`.
//!
//! One subplot per dialect. For each parser, an empirical CDF (eCDF) line:
//! x = per-statement parse time (ns, log scale), y = fraction of that parser's
//! accepted statements parsed within that time. A triangle on the x-axis marks
//! the concatenated-body time normalized by statement count (`concat/n`).
//! Parsers are colored consistently; a legend cell maps colors to names.

use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};
use plotters::style::RGBColor;
use sql_ast_benchmark::datasets::Dialect;
use std::fs;
use std::path::Path;

const DIST_DIR: &str = "target/bench_dist";
const OUTPUT_FILE: &str = "benchmark_results.svg";

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let summary = load_summary(&Path::new(DIST_DIR).join("summary.csv"));
    if summary.is_empty() {
        eprintln!("No data in {DIST_DIR}/summary.csv. Run `cargo bench` first.");
        std::process::exit(1);
    }

    // Group series by dialect in canonical order.
    let groups: Vec<(Dialect, Vec<Series>)> = DIALECT_ORDER
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

    // X range (ns, log): global min to a high percentile, so rare multi-ms
    // outliers do not stretch the axis. Use the 99th percentile per series.
    let mut xmin = f64::MAX;
    let mut xmax = 0.0_f64;
    for (_, series) in &groups {
        for s in series {
            xmin = xmin.min(s.times[0]).min(s.concat.max(1.0));
            let p99 =
                s.times[((0.99 * (s.times.len() - 1) as f64) as usize).min(s.times.len() - 1)];
            xmax = xmax.max(p99).max(s.concat);
        }
    }
    let xmin = (xmin * 0.8).max(1.0);
    let xmax = xmax * 1.3;

    // Text summary.
    println!("Per-statement median / p90 and concatenated-normalized (ns):");
    for (d, series) in &groups {
        println!("\n{}:", d.dir_name());
        for s in series {
            let q = |f: f64| {
                s.times[((f * (s.times.len() - 1) as f64) as usize).min(s.times.len() - 1)]
            };
            println!(
                "  {:<24} n={:<6} median={:>8.0}  p90={:>9.0}  concat/n={:>8.0}",
                s.parser,
                s.times.len(),
                q(0.5),
                q(0.9),
                s.concat
            );
        }
    }

    let num_plots = groups.len();
    let cols = 4usize;
    let rows_n = (num_plots + 1).div_ceil(cols);
    let sub_w = 440u32;
    let sub_h = 330u32;
    let width = cols as u32 * sub_w;
    let height = rows_n as u32 * sub_h + 70;

    let root = SVGBackend::new(OUTPUT_FILE, (width, height)).into_drawing_area();
    root.fill(&WHITE)?;
    root.draw(&Text::new(
        "Per-statement parse-time eCDF by dialect (x = ns/statement, log; y = fraction of accepted statements; triangle = concatenated/n)",
        (width as i32 / 2, 26),
        ("sans-serif", 17)
            .into_font()
            .color(&BLACK)
            .pos(Pos::new(HPos::Center, VPos::Center)),
    ))?;

    let plotting = root.margin(45, 10, 10, 10);
    let areas = plotting.split_evenly((rows_n, cols));

    for (idx, (dialect, series)) in groups.iter().enumerate() {
        let area = &areas[idx];
        let mut chart = ChartBuilder::on(area)
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
            // concat/n marker on the baseline.
            if s.concat > 0.0 {
                chart.draw_series(std::iter::once(TriangleMarker::new(
                    (s.concat, 0.0),
                    6,
                    color.filled(),
                )))?;
            }
        }
    }

    // Legend cell.
    if num_plots < areas.len() {
        let legend = &areas[num_plots];
        let line_h = 22;
        let start_y = 20;
        for (i, name) in all_parsers.iter().enumerate() {
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
        let y = start_y + all_parsers.len() as i32 * line_h + 8;
        legend.draw(&TriangleMarker::new((26, y + 6), 6, BLACK.filled()))?;
        legend.draw(&Text::new(
            "concatenated / n",
            (46, y),
            ("sans-serif", 12).into_font(),
        ))?;
    }

    root.present()?;
    println!("\nSVG plot saved to {OUTPUT_FILE}");
    Ok(())
}

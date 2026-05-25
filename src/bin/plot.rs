#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]

//! Renders `benchmark_results.svg` from Criterion output.
//!
//! Reads `target/criterion/{dialect}/{parser}/{size}/new/estimates.json`
//! (produced by `cargo bench`) and draws one log-log subplot per dialect, with
//! one line per parser. Dialects and parsers are discovered dynamically, so the
//! plot adapts to whatever the multi-dialect benchmark produced.

use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};
use plotters::style::RGBColor;
use sql_ast_benchmark::datasets::Dialect;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const BENCHMARK_BASE_DIR: &str = "target/criterion";
const OUTPUT_FILE: &str = "benchmark_results.svg";
const EXPECTED_SIZES: [usize; 4] = [1, 10, 100, 1000];

/// Dialect plotting order (only those with benchmark data are drawn).
const GROUP_ORDER: [Dialect; 13] = [
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

/// Stable color per parser display name (`BenchParser::name()`).
fn parser_color(name: &str) -> RGBColor {
    match name {
        "sqlparser-rs" => RGBColor(15, 76, 129), // classic blue
        "pg_query.rs" => RGBColor(255, 111, 97), // living coral
        "pg_query (summary)" => RGBColor(247, 202, 201), // rose quartz
        "polyglot-sql" => RGBColor(245, 223, 77), // illuminating
        "qusql-parse" => RGBColor(95, 75, 139),  // ultra violet
        "databend-common-ast" => RGBColor(0, 155, 119), // emerald
        "sqlglot-rust" => RGBColor(237, 135, 45), // orange
        "sqlite3-parser" => RGBColor(0, 128, 128), // teal
        "orql" => RGBColor(139, 69, 19),         // brown
        _ => RGBColor(120, 120, 120),            // senax / unknown: gray
    }
}

#[derive(Debug, Clone)]
struct BenchmarkResult {
    mean_ns: f64,
    std_dev_ns: f64,
}

type ParserResults = HashMap<usize, BenchmarkResult>;
/// parser display name -> results by size
type GroupResults = HashMap<String, ParserResults>;

/// Read the per-size results for one parser directory.
fn parse_parser_dir(dir: &Path) -> ParserResults {
    let mut out = ParserResults::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let size_name = entry.file_name().to_string_lossy().into_owned();
        let Ok(size) = size_name.parse::<usize>() else {
            continue;
        };
        if !EXPECTED_SIZES.contains(&size) {
            continue;
        }
        let estimates = entry.path().join("new").join("estimates.json");
        if let Ok(content) = fs::read_to_string(&estimates) {
            if let Some((mean, std_dev)) = extract_mean_and_std_dev(&content) {
                out.insert(
                    size,
                    BenchmarkResult {
                        mean_ns: mean,
                        std_dev_ns: std_dev,
                    },
                );
            }
        }
    }
    out
}

/// Read all parser results for one dialect group directory.
fn parse_group(dir: &Path) -> GroupResults {
    let mut group = GroupResults::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return group;
    };
    for entry in entries.filter_map(Result::ok) {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // Skip criterion's "report" dir and the stale numeric value dirs.
        if name == "report" || name.parse::<usize>().is_ok() {
            continue;
        }
        let results = parse_parser_dir(&entry.path());
        if !results.is_empty() {
            group.insert(name, results);
        }
    }
    group
}

fn extract_mean_and_std_dev(json_content: &str) -> Option<(f64, f64)> {
    let mean = extract_field(json_content, "\"mean\"");
    let std_dev = extract_field(json_content, "\"std_dev\"");
    match (mean, std_dev) {
        (Some(m), Some(s)) => Some((m, s)),
        (Some(m), None) => Some((m, 0.0)),
        _ => None,
    }
}

fn extract_field(json_content: &str, field_name: &str) -> Option<f64> {
    let start = json_content.find(field_name)?;
    let rest = &json_content[start..];
    let pe_start = rest.find("\"point_estimate\"")?;
    let pe_rest = &rest[pe_start + 17..];
    let end = pe_rest.find([',', '}'])?;
    pe_rest[..end].trim().parse::<f64>().ok()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = Path::new(BENCHMARK_BASE_DIR);
    if !base.exists() {
        eprintln!("Benchmark results not found at {BENCHMARK_BASE_DIR}");
        eprintln!("Run `cargo bench` first to generate benchmark data.");
        std::process::exit(1);
    }

    // Discover groups in plotting order, keeping only those with data.
    let groups: Vec<(Dialect, GroupResults)> = GROUP_ORDER
        .iter()
        .filter_map(|&d| {
            let g = parse_group(&base.join(d.dir_name()));
            (!g.is_empty()).then_some((d, g))
        })
        .collect();

    if groups.is_empty() {
        eprintln!("No benchmark results found. Run `cargo bench` first.");
        std::process::exit(1);
    }

    // Stable sorted list of all parser names seen (for the legend).
    let mut all_parsers: Vec<String> = groups.iter().flat_map(|(_, g)| g.keys().cloned()).collect();
    all_parsers.sort_unstable();
    all_parsers.dedup();

    // Text summary.
    println!("Benchmark results (median, ms):");
    for (dialect, group) in &groups {
        println!("\n{}:", dialect.dir_name());
        let mut sizes: Vec<usize> = group.values().flat_map(|p| p.keys().copied()).collect();
        sizes.sort_unstable();
        sizes.dedup();
        for size in &sizes {
            println!("  {size} statements:");
            let mut names: Vec<&String> = group.keys().collect();
            names.sort();
            for name in names {
                if let Some(r) = group[name].get(size) {
                    println!("    {name:24}: {:.4} ms", r.mean_ns / 1_000_000.0);
                }
            }
        }
    }

    // ── SVG grid: one subplot per dialect, plus a legend cell. ──
    let num_plots = groups.len();
    let cols = 4usize;
    let rows = (num_plots + 1).div_ceil(cols); // +1 reserves a legend cell
    let subplot_w = 380u32;
    let subplot_h = 290u32;
    let total_width = cols as u32 * subplot_w;
    let total_height = rows as u32 * subplot_h + 70;

    let root = SVGBackend::new(OUTPUT_FILE, (total_width, total_height)).into_drawing_area();
    root.fill(&WHITE)?;
    root.draw(&Text::new(
        "SQL Parser Benchmark (parse time, log-log, per dialect)",
        (total_width as i32 / 2, 28),
        ("sans-serif", 22)
            .into_font()
            .color(&BLACK)
            .pos(Pos::new(HPos::Center, VPos::Center)),
    ))?;

    // Global y-range (ms) for consistent axes.
    let all_means = || {
        groups
            .iter()
            .flat_map(|(_, g)| g.values())
            .flat_map(|p| p.values())
    };
    let global_min = all_means()
        .map(|r| r.mean_ns / 1_000_000.0)
        .fold(f64::MAX, f64::min)
        * 0.5;
    let global_max = all_means()
        .map(|r| (r.mean_ns + r.std_dev_ns) / 1_000_000.0)
        .fold(0.0_f64, f64::max)
        * 1.5;
    let global_min = if global_min.is_finite() && global_min > 0.0 {
        global_min
    } else {
        0.0001
    };

    let plotting_area = root.margin(45, 10, 10, 10);
    let areas = plotting_area.split_evenly((rows, cols));

    for (idx, (dialect, group)) in groups.iter().enumerate() {
        let area = &areas[idx];
        let mut chart = ChartBuilder::on(area)
            .caption(dialect.dir_name(), ("sans-serif", 15))
            .margin(8)
            .x_label_area_size(32)
            .y_label_area_size(52)
            .build_cartesian_2d(
                (1f64..1000f64).log_scale(),
                (global_min..global_max).log_scale(),
            )?;
        chart
            .configure_mesh()
            .x_desc("Statements")
            .y_desc("Time (ms)")
            .x_label_style(("sans-serif", 10))
            .y_label_style(("sans-serif", 10))
            .draw()?;

        let mut names: Vec<&String> = group.keys().collect();
        names.sort();
        for name in names {
            let color = parser_color(name);
            let mut points: Vec<(f64, f64)> = group[name]
                .iter()
                .map(|(&size, r)| (size as f64, r.mean_ns / 1_000_000.0))
                .collect();
            points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            chart.draw_series(LineSeries::new(
                points.iter().copied(),
                color.stroke_width(2),
            ))?;
            chart.draw_series(
                points
                    .iter()
                    .map(|&(x, y)| Circle::new((x, y), 3, color.filled())),
            )?;
        }
    }

    // Legend in the next free cell.
    if num_plots < areas.len() {
        let legend = &areas[num_plots];
        let (w, h) = legend.dim_in_pixel();
        let line_h = 22;
        let start_y = (h as i32 - all_parsers.len() as i32 * line_h) / 2;
        for (i, name) in all_parsers.iter().enumerate() {
            let y = start_y + i as i32 * line_h;
            legend.draw(&Rectangle::new(
                [(15, y), (35, y + 12)],
                parser_color(name).filled(),
            ))?;
            legend.draw(&Text::new(
                name.clone(),
                (42, y),
                ("sans-serif", 12).into_font(),
            ))?;
            let _ = w;
        }
    }

    root.present()?;
    println!("\nSVG plot saved to {OUTPUT_FILE}");
    Ok(())
}

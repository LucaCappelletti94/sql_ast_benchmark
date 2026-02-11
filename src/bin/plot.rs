#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::inefficient_to_string)]

use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};
use plotters::style::RGBColor;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const BENCHMARK_BASE_DIR: &str = "target/criterion";
const OUTPUT_FILE: &str = "benchmark_results.svg";
// Standard sizes plus max corpus sizes for INSERT/UPDATE/DELETE
const EXPECTED_SIZES: [usize; 9] = [1, 10, 50, 100, 500, 933, 983, 992, 1000];

// Pantone-inspired colors
const PANTONE_CLASSIC_BLUE: RGBColor = RGBColor(15, 76, 129); // Pantone 19-4052
const PANTONE_LIVING_CORAL: RGBColor = RGBColor(255, 111, 97); // Pantone 16-1546
const PANTONE_GREENERY: RGBColor = RGBColor(136, 176, 75); // Pantone 15-0343
const PANTONE_ULTRA_VIOLET: RGBColor = RGBColor(95, 75, 139); // Pantone 18-3838

// Parser configuration
const PARSERS: [(&str, &str, RGBColor); 4] = [
    ("sqlparser", "sqlparser-rs", PANTONE_CLASSIC_BLUE),
    ("pg_query", "pg_query.rs", PANTONE_LIVING_CORAL),
    ("pg_parse", "pg_parse", PANTONE_GREENERY),
    ("sql_parse", "sql-parse", PANTONE_ULTRA_VIOLET),
];

#[derive(Debug, Clone)]
struct BenchmarkResult {
    mean_ns: f64,
    std_dev_ns: f64,
}

type ParserResults = HashMap<usize, BenchmarkResult>;
type GroupResults = HashMap<String, ParserResults>;

fn parse_all_benchmark_groups(base_path: &Path) -> HashMap<String, GroupResults> {
    let mut all_results: HashMap<String, GroupResults> = HashMap::new();

    // Look for benchmark groups (directories under criterion/)
    let groups = ["select", "insert", "update", "delete", "dml"];

    for group_name in &groups {
        let group_path = base_path.join(group_name);
        if group_path.is_dir() {
            let mut group_results: GroupResults = HashMap::new();

            for (parser, _, _) in &PARSERS {
                let parser_path = group_path.join(parser);
                if parser_path.is_dir() {
                    let mut parser_results: ParserResults = HashMap::new();

                    // Only include expected benchmark sizes to avoid stale data
                    if let Ok(entries) = fs::read_dir(&parser_path) {
                        for entry in entries.filter_map(Result::ok) {
                            let size_name = entry.file_name().to_string_lossy().to_string();
                            if let Ok(size) = size_name.parse::<usize>() {
                                if !EXPECTED_SIZES.contains(&size) {
                                    continue;
                                }
                                let estimates_path =
                                    entry.path().join("new").join("estimates.json");
                                if estimates_path.exists() {
                                    if let Ok(content) = fs::read_to_string(&estimates_path) {
                                        if let Some((mean, std_dev)) =
                                            extract_mean_and_std_dev(&content)
                                        {
                                            parser_results.insert(
                                                size,
                                                BenchmarkResult {
                                                    mean_ns: mean,
                                                    std_dev_ns: std_dev,
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if !parser_results.is_empty() {
                        group_results.insert((*parser).to_string(), parser_results);
                    }
                }
            }

            if !group_results.is_empty() {
                all_results.insert((*group_name).to_string(), group_results);
            }
        }
    }

    all_results
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
    if let Some(start) = json_content.find(field_name) {
        let rest = &json_content[start..];
        if let Some(pe_start) = rest.find("\"point_estimate\"") {
            let pe_rest = &rest[pe_start + 17..];
            if let Some(end) = pe_rest.find([',', '}']) {
                if let Ok(value) = pe_rest[..end].trim().parse::<f64>() {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let benchmark_path = Path::new(BENCHMARK_BASE_DIR);

    if !benchmark_path.exists() {
        eprintln!("Benchmark results not found at {BENCHMARK_BASE_DIR}");
        eprintln!("Run `cargo bench` first to generate benchmark data.");
        std::process::exit(1);
    }

    let all_results = parse_all_benchmark_groups(benchmark_path);

    if all_results.is_empty() {
        eprintln!("No benchmark results found.");
        std::process::exit(1);
    }

    // Print results
    println!("Benchmark results:");
    println!("==================");

    for (group_name, group_results) in &all_results {
        println!("\n{group_name}:");
        println!("{}", "-".repeat(group_name.len() + 1));

        let mut sizes: Vec<usize> = group_results
            .values()
            .flat_map(|p| p.keys())
            .copied()
            .collect();
        sizes.sort_unstable();
        sizes.dedup();

        for size in &sizes {
            println!("\n  {size} statements:");
            for (parser_key, parser_name, _) in &PARSERS {
                if let Some(parser_results) = group_results.get(*parser_key) {
                    if let Some(result) = parser_results.get(size) {
                        println!(
                            "    {parser_name:12}: {:.3} ± {:.3} ms",
                            result.mean_ns / 1_000_000.0,
                            result.std_dev_ns / 1_000_000.0
                        );
                    }
                }
            }
        }
    }

    // Create SVG with subplots - one per benchmark group
    let group_order = ["select", "insert", "update", "delete", "dml"];
    let group_titles = ["SELECT", "INSERT", "UPDATE", "DELETE", "All DML"];

    #[allow(clippy::unnecessary_to_owned)]
    let num_plots = group_order
        .iter()
        .filter(|g| all_results.contains_key(&g.to_string()))
        .count();

    if num_plots == 0 {
        eprintln!("No valid benchmark groups found.");
        std::process::exit(1);
    }

    let cols = 3;
    let rows = num_plots.div_ceil(cols);
    let subplot_width = 400;
    let subplot_height = 300;
    let total_width = cols as u32 * subplot_width;
    let total_height = rows as u32 * subplot_height + 100;

    let root = SVGBackend::new(OUTPUT_FILE, (total_width, total_height)).into_drawing_area();
    root.fill(&WHITE)?;

    // Main title (centered)
    let title = "SQL Parser Benchmark Comparison";
    let title_style = ("sans-serif", 24).into_font().color(&BLACK);
    root.draw(&Text::new(
        title,
        (total_width as i32 / 2, 30),
        title_style.pos(Pos::new(HPos::Center, VPos::Center)),
    ))?;

    // Calculate global max time across all groups for consistent y-axis
    let global_max_time = all_results
        .values()
        .flat_map(|group| group.values())
        .flat_map(|parser| parser.values())
        .map(|r| (r.mean_ns + r.std_dev_ns) / 1_000_000.0)
        .fold(0.0_f64, f64::max)
        * 1.15;

    // Use fixed x-axis range (1 to 1000) for all plots
    let global_max_size = 1000.0_f64;

    // Create subplots
    let plotting_area = root.margin(50, 10, 20, 10);
    let areas = plotting_area.split_evenly((rows, cols));

    let mut plot_idx = 0;
    for (group_name, title) in group_order.iter().zip(group_titles.iter()) {
        #[allow(clippy::unnecessary_to_owned)]
        if let Some(group_results) = all_results.get(&group_name.to_string()) {
            if plot_idx >= areas.len() {
                break;
            }

            let area = &areas[plot_idx];
            plot_idx += 1;

            // Get all sizes for this group (for plotting points)
            let mut sizes: Vec<usize> = group_results
                .values()
                .flat_map(|p| p.keys())
                .copied()
                .collect();
            sizes.sort_unstable();
            sizes.dedup();

            if sizes.is_empty() {
                continue;
            }

            let mut chart = ChartBuilder::on(area)
                .caption(*title, ("sans-serif", 16))
                .margin(10)
                .x_label_area_size(35)
                .y_label_area_size(55)
                .build_cartesian_2d((1f64..global_max_size).log_scale(), 0f64..global_max_time)?;

            chart
                .configure_mesh()
                .x_desc("Statements")
                .y_desc("Time (ms)")
                .x_label_style(("sans-serif", 11))
                .y_label_style(("sans-serif", 11))
                .draw()?;

            // Draw lines for each parser
            for (parser_key, _, color) in &PARSERS {
                if let Some(parser_results) = group_results.get(*parser_key) {
                    let mut points: Vec<(f64, f64, f64)> = parser_results
                        .iter()
                        .map(|(&size, r)| {
                            (
                                size as f64,
                                r.mean_ns / 1_000_000.0,
                                r.std_dev_ns / 1_000_000.0,
                            )
                        })
                        .collect();
                    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

                    // Line
                    chart.draw_series(LineSeries::new(
                        points.iter().map(|&(x, y, _)| (x, y)),
                        color.stroke_width(2),
                    ))?;

                    // Markers
                    chart.draw_series(
                        points
                            .iter()
                            .map(|&(x, y, _)| Circle::new((x, y), 5, color.filled())),
                    )?;

                    // Error bars
                    for &(x, y, std_dev) in &points {
                        if std_dev > 0.0 {
                            let cap_width = x * 0.08;
                            chart.draw_series(std::iter::once(PathElement::new(
                                vec![(x, y - std_dev), (x, y + std_dev)],
                                color.stroke_width(1),
                            )))?;
                            chart.draw_series(std::iter::once(PathElement::new(
                                vec![(x - cap_width, y + std_dev), (x + cap_width, y + std_dev)],
                                color.stroke_width(1),
                            )))?;
                            chart.draw_series(std::iter::once(PathElement::new(
                                vec![(x - cap_width, y - std_dev), (x + cap_width, y - std_dev)],
                                color.stroke_width(1),
                            )))?;
                        }
                    }
                }
            }
        }
    }

    // Draw legend in the bottom-right empty cell (6th position in 2x3 grid)
    if num_plots < areas.len() {
        let legend_area = &areas[num_plots];
        let (width, height) = legend_area.dim_in_pixel();
        let center_x = width as i32 / 2;
        let center_y = height as i32 / 2;

        let num_parsers = PARSERS.len() as i32;
        let line_height = 30;
        let start_y = center_y - (num_parsers * line_height) / 2;

        for (i, (_, parser_name, color)) in PARSERS.iter().enumerate() {
            let y = start_y + (i as i32) * line_height;

            legend_area.draw(&Rectangle::new(
                [(center_x - 100, y), (center_x - 80, y + 12)],
                color.filled(),
            ))?;
            legend_area.draw(&Text::new(
                *parser_name,
                (center_x - 75, y),
                ("sans-serif", 14).into_font(),
            ))?;
        }
    }

    root.present()?;

    println!("\n\nSVG plot saved to {OUTPUT_FILE}");

    Ok(())
}

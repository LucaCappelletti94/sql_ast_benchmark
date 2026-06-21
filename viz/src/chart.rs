//! Chart rendering to SVG strings, reused by the native exporter and the wasm
//! viewer. A generic line/box renderer ([`ecdf_lines`], [`box_lines`]) draws a
//! set of [`Line`]s, so the same code serves both the per-dialect view (one line
//! per parser) and the per-parser view (one line per dialect). Output is an SVG
//! `String` via plotters' `SVGBackend::with_string`, so the browser renders
//! charts on demand from the JSON.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use crate::color::parser_rgb;
use crate::marker::{marker_for, Marker};
use crate::schema::{DialectData, ParserMetrics, ParserPerf};
use plotters::prelude::*;
use plotters::style::RGBColor;

type Res = Result<(), Box<dyn std::error::Error>>;

/// Radius in pixels of the per-series glyphs drawn on curves and in legends.
const MARKER_R: i32 = 4;

/// Draw a filled marker glyph at a data point on a chart. Generic over the
/// coordinate system so the same call serves the linear and log-scaled charts.
/// The glyph's vertices are pixel offsets from the anchor, composed onto an
/// [`EmptyElement`] at the data coordinate, so size stays constant regardless of
/// the axis scale.
fn draw_marker<DB, CT>(
    chart: &mut ChartContext<DB, CT>,
    marker: Marker,
    x: f64,
    y: f64,
    color: RGBColor,
) -> Res
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + 'static,
    CT: plotters::coord::CoordTranslate<From = (f64, f64)>,
{
    chart.draw_series(std::iter::once(
        EmptyElement::at((x, y)) + Polygon::new(marker.vertices(MARKER_R), color.filled()),
    ))?;
    Ok(())
}

/// Pixels reserved on the right of each chart for the legend, sized to the
/// widest label so short-label charts (e.g. a single-dialect parser page) do
/// not get a wide empty band while long-label charts still fit. The 34px swatch
/// indent precedes the text. ~5px/char approximates 11px sans-serif, plus a
/// 16px right margin. Clamped so even one short label keeps a sane band.
fn legend_width(lines: &[Line]) -> i32 {
    let max_chars = lines
        .iter()
        .map(|l| l.label.chars().count())
        .max()
        .unwrap_or(0) as i32;
    (34 + max_chars * 5 + 16).clamp(100, 220)
}

/// One series: a labelled distribution with a color, percentiles, and eCDF.
pub struct Line {
    pub label: String,
    pub rgb: (u8, u8, u8),
    /// Optional grey second legend line (e.g. "missed 12%  RT 100%").
    pub sub: Option<String>,
    pub min: f64,
    pub p10: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
    pub p99: f64,
    /// `[ns, fraction]` eCDF points, ascending.
    pub ecdf: Vec<[f64; 2]>,
}

fn rgb(c: (u8, u8, u8)) -> RGBColor {
    RGBColor(c.0, c.1, c.2)
}

fn commas(n: usize) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

fn draw_legend<DB>(area: &DrawingArea<DB, plotters::coord::Shift>, lines: &[Line]) -> Res
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + 'static,
{
    let grey = RGBColor(90, 90, 90);
    let line_h = if lines.len() > 9 { 24 } else { 30 };
    for (i, l) in lines.iter().enumerate() {
        let y = 14 + i as i32 * line_h;
        area.draw(&PathElement::new(
            vec![(6, y + 6), (30, y + 6)],
            rgb(l.rgb).stroke_width(3),
        ))?;
        // Marker glyph centered on the swatch so the legend matches the curve.
        let verts: Vec<(i32, i32)> = marker_for(&l.label)
            .vertices(MARKER_R)
            .into_iter()
            .map(|(dx, dy)| (18 + dx, y + 6 + dy))
            .collect();
        area.draw(&Polygon::new(verts, rgb(l.rgb).filled()))?;
        area.draw(&Text::new(
            l.label.clone(),
            (34, y),
            ("sans-serif", 11).into_font(),
        ))?;
        if let Some(sub) = &l.sub {
            area.draw(&Text::new(
                sub.clone(),
                (34, y + 13),
                ("sans-serif", 9).into_font().color(&grey),
            ))?;
        }
    }
    Ok(())
}

/// eCDF chart: x = value (log), y = fraction within t. One curve per [`Line`].
/// `x_desc` labels the x axis (e.g. "ns / statement" or "bytes / statement").
#[must_use]
pub fn ecdf_lines(title: &str, lines: &[Line], w: u32, h: u32, x_desc: &str) -> String {
    let mut buf = String::new();
    {
        let root = SVGBackend::with_string(&mut buf, (w, h)).into_drawing_area();
        let _: Res = (|| {
            root.fill(&WHITE)?;
            let (plot, legend) = root.split_horizontally(w as i32 - legend_width(lines));

            let mut xmin = f64::MAX;
            let mut xmax = 0.0_f64;
            for l in lines {
                if l.min > 0.0 {
                    xmin = xmin.min(l.min);
                }
                xmax = xmax.max(l.p99);
            }
            if !xmin.is_finite() || xmin <= 0.0 {
                xmin = 1.0;
            }
            let xmin = (xmin * 0.8).max(1.0);
            let xmax = (xmax * 1.3).max(xmin * 10.0);

            let mut chart = ChartBuilder::on(&plot)
                .caption(title, ("sans-serif", 16))
                .margin(10)
                .x_label_area_size(34)
                .y_label_area_size(44)
                .build_cartesian_2d((xmin..xmax).log_scale(), 0f64..1.02f64)?;
            chart
                .configure_mesh()
                .x_desc(x_desc)
                .y_desc("frac <= t")
                .x_label_style(("sans-serif", 11))
                .y_label_style(("sans-serif", 11))
                .draw()?;
            for l in lines {
                chart.draw_series(LineSeries::new(
                    l.ecdf.iter().map(|pt| (pt[0], pt[1])),
                    rgb(l.rgb).stroke_width(2),
                ))?;
                // A handful of glyphs along each curve so series stay
                // distinguishable without relying on color. Sampling (rather than
                // one per eCDF point) keeps the curves readable.
                let m = marker_for(&l.label);
                let n = l.ecdf.len();
                if n > 0 {
                    let count = 6.min(n);
                    for k in 0..count {
                        let idx = if count == 1 {
                            n / 2
                        } else {
                            k * (n - 1) / (count - 1)
                        };
                        let pt = l.ecdf[idx];
                        draw_marker(&mut chart, m, pt[0], pt[1], rgb(l.rgb))?;
                    }
                }
            }
            draw_legend(&legend, lines)?;
            root.present()?;
            Ok(())
        })();
    }
    buf
}

/// Box plot: box = p25/median/p75, whiskers = p10/p90, log-y. One box per line.
/// `y_desc` labels the y axis (e.g. "ns / statement" or "bytes / statement").
#[must_use]
pub fn box_lines(title: &str, lines: &[Line], w: u32, h: u32, y_desc: &str) -> String {
    let mut buf = String::new();
    {
        let root = SVGBackend::with_string(&mut buf, (w, h)).into_drawing_area();
        let _: Res = (|| {
            root.fill(&WHITE)?;
            let (plot, legend) = root.split_horizontally(w as i32 - legend_width(lines));

            let mut ymin = f64::MAX;
            let mut ymax = 0.0_f64;
            for l in lines {
                if l.p10 > 0.0 {
                    ymin = ymin.min(l.p10);
                }
                ymax = ymax.max(l.p90);
            }
            if !ymin.is_finite() || ymin <= 0.0 {
                ymin = 1.0;
            }
            let ymin = (ymin * 0.7).max(1.0);
            let ymax = (ymax * 1.5).max(ymin * 10.0);
            let cnt = lines.len().max(1);

            let mut chart = ChartBuilder::on(&plot)
                .caption(title, ("sans-serif", 16))
                .margin(10)
                .x_label_area_size(18)
                .y_label_area_size(52)
                .build_cartesian_2d(0f64..(cnt as f64), (ymin..ymax).log_scale())?;
            chart
                .configure_mesh()
                .disable_x_mesh()
                .x_labels(0)
                .y_desc(y_desc)
                .y_label_style(("sans-serif", 11))
                .draw()?;

            for (i, l) in lines.iter().enumerate() {
                let x = i as f64 + 0.5;
                let (lo, hi) = (x - 0.32, x + 0.32);
                let c = rgb(l.rgb);
                chart.draw_series(std::iter::once(Rectangle::new(
                    [(lo, l.p25), (hi, l.p75)],
                    c.mix(0.45).filled(),
                )))?;
                chart.draw_series(std::iter::once(Rectangle::new(
                    [(lo, l.p25), (hi, l.p75)],
                    c.stroke_width(1),
                )))?;
                chart.draw_series(std::iter::once(PathElement::new(
                    vec![(lo, l.median), (hi, l.median)],
                    c.stroke_width(2),
                )))?;
                for (a, b) in [(l.p10, l.p25), (l.p75, l.p90)] {
                    chart.draw_series(std::iter::once(PathElement::new(
                        vec![(x, a), (x, b)],
                        c.stroke_width(1),
                    )))?;
                }
                for y in [l.p10, l.p90] {
                    chart.draw_series(std::iter::once(PathElement::new(
                        vec![(x - 0.15, y), (x + 0.15, y)],
                        c.stroke_width(1),
                    )))?;
                }
                // Glyph on the median so each box maps to its legend entry
                // without relying on color or box order.
                draw_marker(&mut chart, marker_for(&l.label), x, l.median, c)?;
            }
            draw_legend(&legend, lines)?;
            root.present()?;
            Ok(())
        })();
    }
    buf
}

// ---- per-dialect convenience wrappers (series = parsers) ----

/// Fraction of valid statements the parser failed to accept (false negatives,
/// `1 - recall`). Prefers the reference-graded recall (or, on provenance dialects,
/// the acceptance rate) from `metrics`, falling back to the raw unaccepted
/// fraction when no correctness row exists. Excludes true negatives the parser
/// correctly rejected, so it matches the "missed %" column in the tables.
fn missed_pct(p: &ParserPerf, metrics: Option<&ParserMetrics>, has_reference: bool) -> f64 {
    let graded = metrics.and_then(|m| {
        if has_reference {
            m.recall_pct
        } else {
            m.accept_pct
        }
    });
    match graded {
        Some(r) => (100.0 - r).max(0.0),
        None if p.n_total == 0 => 0.0,
        None => 100.0 * (p.n_total - p.n_accepted) as f64 / p.n_total as f64,
    }
}

fn parser_sub(p: &ParserPerf, metrics: Option<&ParserMetrics>, has_reference: bool) -> String {
    let m = missed_pct(p, metrics, has_reference);
    let missed = if m >= 100.0 {
        "100".to_string()
    } else if m.round() >= 100.0 {
        ">99".to_string()
    } else {
        format!("{m:.0}")
    };
    let rt = match p.roundtrip_pct {
        Some(v) => format!("RT {v:.0}%"),
        None => "RT n/a".to_string(),
    };
    format!("missed {missed}%   {rt}")
}

fn lines_from_dialect(d: &DialectData) -> Vec<Line> {
    d.perf
        .iter()
        .map(|p| Line {
            label: p.parser.clone(),
            rgb: parser_rgb(&p.parser),
            sub: Some(parser_sub(
                p,
                d.correctness.iter().find(|m| m.parser == p.parser),
                d.has_reference,
            )),
            min: p.min,
            p10: p.p10,
            p25: p.p25,
            median: p.median,
            p75: p.p75,
            p90: p.p90,
            p99: p.p99,
            ecdf: p.ecdf.clone(),
        })
        .collect()
}

fn dialect_title(d: &DialectData) -> String {
    format!(
        "{} (n={})",
        d.display_name,
        commas(d.valid_total + d.invalid_total)
    )
}

/// eCDF chart for one dialect (one curve per parser).
#[must_use]
pub fn ecdf_svg(d: &DialectData, w: u32, h: u32) -> String {
    ecdf_lines(
        &dialect_title(d),
        &lines_from_dialect(d),
        w,
        h,
        "ns / statement",
    )
}

/// Box plot for one dialect (one box per parser).
#[must_use]
pub fn box_svg(d: &DialectData, w: u32, h: u32) -> String {
    box_lines(
        &dialect_title(d),
        &lines_from_dialect(d),
        w,
        h,
        "ns / statement",
    )
}

/// Build chart [`Line`]s from a labelled set of memory distributions (no eCDF
/// sub-label), for the per-parser or per-dialect memory charts.
#[must_use]
pub fn mem_line(label: String, rgb: (u8, u8, u8), dist: &crate::schema::MemDist) -> Line {
    Line {
        label,
        rgb,
        sub: None,
        min: dist.min,
        p10: dist.p10,
        p25: dist.p25,
        median: dist.median,
        p75: dist.p75,
        p90: dist.p90,
        p99: dist.p99,
        ecdf: dist.ecdf.clone(),
    }
}

/// Fractional year of an ISO `YYYY-MM-DD` date, for placing trend points on a
/// time x-axis. `None` if the string does not parse. The intra-month term keeps
/// releases days apart visibly distinct without needing a calendar library.
#[must_use]
pub fn year_frac(date: &str) -> Option<f64> {
    let mut it = date.split('-');
    let y: f64 = it.next()?.parse().ok()?;
    let m: f64 = it.next()?.parse().ok()?;
    let d: f64 = it.next().unwrap_or("1").parse().unwrap_or(1.0);
    Some(y + (m - 1.0) / 12.0 + (d - 1.0) / 31.0 / 12.0)
}

/// Format a fractional year as `YYYY-MM` for an axis tick.
fn frac_to_ym(f: f64) -> String {
    let year = f.floor();
    let mut month = ((f - year) * 12.0).round() as i64 + 1;
    let mut y = year as i64;
    if month > 12 {
        month -= 12;
        y += 1;
    }
    if month < 1 {
        month = 1;
    }
    format!("{y}-{month:02}")
}

/// One labelled series for a [`trend_lines`] chart: a robust summary at each
/// release. Each point is `(x, median, p25, p75)` with `x` a fractional year.
pub struct TrendSeries {
    pub label: String,
    pub rgb: (u8, u8, u8),
    /// `(x, median, p25, p75)` per release this series has data for, ascending x.
    pub points: Vec<(f64, f64, f64, f64)>,
}

/// Trend chart: x = release date, y = median on a log scale with an
/// interquartile (p25-p75) bar at each release, one line per series. Median and
/// IQR are used (not mean and std) because parse-time and memory distributions
/// are heavily right-skewed, so the mean is outlier-dominated.
#[must_use]
pub fn trend_lines(title: &str, series: &[TrendSeries], w: u32, h: u32, y_desc: &str) -> String {
    let legend: Vec<Line> = series
        .iter()
        .map(|s| Line {
            label: s.label.clone(),
            rgb: s.rgb,
            sub: None,
            min: 0.0,
            p10: 0.0,
            p25: 0.0,
            median: 0.0,
            p75: 0.0,
            p90: 0.0,
            p99: 0.0,
            ecdf: Vec::new(),
        })
        .collect();

    let mut buf = String::new();
    {
        let root = SVGBackend::with_string(&mut buf, (w, h)).into_drawing_area();
        let _: Res = (|| {
            root.fill(&WHITE)?;
            let (plot, legend_area) = root.split_horizontally(w as i32 - legend_width(&legend));

            let mut xmin = f64::MAX;
            let mut xmax = f64::MIN;
            let mut ymin = f64::MAX;
            let mut ymax = 0.0_f64;
            for s in series {
                for &(x, median, p25, p75) in &s.points {
                    xmin = xmin.min(x);
                    xmax = xmax.max(x);
                    if p25 > 0.0 {
                        ymin = ymin.min(p25);
                    }
                    if median > 0.0 {
                        ymin = ymin.min(median);
                    }
                    ymax = ymax.max(p75.max(median));
                }
            }
            if !xmin.is_finite() || !xmax.is_finite() {
                return Ok(()); // no data
            }
            // Pad the x range. A single-release family still gets a sane window.
            let xpad = ((xmax - xmin) * 0.08).max(0.08);
            let (xlo, xhi) = (xmin - xpad, xmax + xpad);
            if !ymin.is_finite() || ymin <= 0.0 {
                ymin = 1.0;
            }
            let ylo = (ymin * 0.8).max(1.0);
            let yhi = (ymax * 1.3).max(ylo * 10.0);

            let mut chart = ChartBuilder::on(&plot)
                .caption(title, ("sans-serif", 16))
                .margin(10)
                .x_label_area_size(40)
                .y_label_area_size(52)
                .build_cartesian_2d(xlo..xhi, (ylo..yhi).log_scale())?;
            chart
                .configure_mesh()
                .x_desc("release date")
                .y_desc(y_desc)
                .x_labels(6)
                .x_label_formatter(&|x| frac_to_ym(*x))
                .y_label_style(("sans-serif", 11))
                .x_label_style(("sans-serif", 10))
                .draw()?;

            for s in series {
                // Median line across the releases this series covers.
                chart.draw_series(LineSeries::new(
                    s.points.iter().map(|&(x, m, _, _)| (x, m)),
                    rgb(s.rgb).stroke_width(2),
                ))?;
                // Interquartile bar and a per-series glyph at each release.
                let m = marker_for(&s.label);
                for &(x, median, p25, p75) in &s.points {
                    let lo = p25.max(ylo);
                    let hi = p75.max(lo);
                    chart.draw_series(std::iter::once(PathElement::new(
                        vec![(x, lo), (x, hi)],
                        rgb(s.rgb).mix(0.5).stroke_width(1),
                    )))?;
                    draw_marker(&mut chart, m, x, median, rgb(s.rgb))?;
                }
            }
            draw_legend(&legend_area, &legend)?;
            root.present()?;
            Ok(())
        })();
    }
    buf
}

/// Percentage trend chart: x = release date, y = a rate in percent on a linear
/// axis, one line per series. Each point's `median` slot carries the value (the
/// p25/p75 slots are ignored, since a rate is a single number rather than a
/// distribution). Used for the accept/recall, false-positive, panic, round-trip,
/// and contentious-recall trends. The y range hugs the data, clamped to 0..102,
/// so small changes between releases stay visible.
#[must_use]
pub fn pct_trend_lines(
    title: &str,
    series: &[TrendSeries],
    w: u32,
    h: u32,
    y_desc: &str,
) -> String {
    linear_trend(title, series, w, h, y_desc, |ymin, ymax| {
        let ylo = (ymin - 2.0).max(0.0);
        let yhi = (ymax + 2.0).min(102.0).max(ylo + 1.0);
        (ylo, yhi)
    })
}

/// Count trend chart: like [`pct_trend_lines`] but on a linear axis anchored at
/// zero with no upper clamp, for absolute counts (such as accepted-statement
/// coverage) rather than rates.
#[must_use]
pub fn count_trend_lines(
    title: &str,
    series: &[TrendSeries],
    w: u32,
    h: u32,
    y_desc: &str,
) -> String {
    linear_trend(title, series, w, h, y_desc, |_ymin, ymax| {
        (0.0, (ymax * 1.08).max(1.0))
    })
}

/// Shared linear-axis trend renderer. `y_bounds` maps the data's `(ymin, ymax)`
/// to the drawn `(ylo, yhi)`, the one axis difference between the rate and count
/// variants. Each point's `median` slot carries the value.
fn linear_trend(
    title: &str,
    series: &[TrendSeries],
    w: u32,
    h: u32,
    y_desc: &str,
    y_bounds: impl Fn(f64, f64) -> (f64, f64),
) -> String {
    let legend: Vec<Line> = series
        .iter()
        .map(|s| Line {
            label: s.label.clone(),
            rgb: s.rgb,
            sub: None,
            min: 0.0,
            p10: 0.0,
            p25: 0.0,
            median: 0.0,
            p75: 0.0,
            p90: 0.0,
            p99: 0.0,
            ecdf: Vec::new(),
        })
        .collect();

    let mut buf = String::new();
    {
        let root = SVGBackend::with_string(&mut buf, (w, h)).into_drawing_area();
        let _: Res = (|| {
            root.fill(&WHITE)?;
            let (plot, legend_area) = root.split_horizontally(w as i32 - legend_width(&legend));

            let mut xmin = f64::MAX;
            let mut xmax = f64::MIN;
            let mut ymin = f64::MAX;
            let mut ymax = f64::MIN;
            for s in series {
                for &(x, v, _, _) in &s.points {
                    xmin = xmin.min(x);
                    xmax = xmax.max(x);
                    ymin = ymin.min(v);
                    ymax = ymax.max(v);
                }
            }
            if !xmin.is_finite() || !xmax.is_finite() || !ymin.is_finite() {
                return Ok(()); // no data
            }
            let xpad = ((xmax - xmin) * 0.08).max(0.08);
            let (xlo, xhi) = (xmin - xpad, xmax + xpad);
            let (ylo, yhi) = y_bounds(ymin, ymax);

            let mut chart = ChartBuilder::on(&plot)
                .caption(title, ("sans-serif", 16))
                .margin(10)
                .x_label_area_size(40)
                .y_label_area_size(52)
                .build_cartesian_2d(xlo..xhi, ylo..yhi)?;
            chart
                .configure_mesh()
                .x_desc("release date")
                .y_desc(y_desc)
                .x_labels(6)
                .x_label_formatter(&|x| frac_to_ym(*x))
                .x_label_style(("sans-serif", 10))
                .y_label_style(("sans-serif", 11))
                .draw()?;

            for s in series {
                chart.draw_series(LineSeries::new(
                    s.points.iter().map(|&(x, v, _, _)| (x, v)),
                    rgb(s.rgb).stroke_width(2),
                ))?;
                let m = marker_for(&s.label);
                for &(x, v, _, _) in &s.points {
                    draw_marker(&mut chart, m, x, v, rgb(s.rgb))?;
                }
            }
            draw_legend(&legend_area, &legend)?;
            root.present()?;
            Ok(())
        })();
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::{box_svg, ecdf_svg};
    use crate::schema::{CoverageMatrix, DialectData, ParserPerf};

    fn sample() -> DialectData {
        let perf = ParserPerf {
            parser: "sqlparser-rs".to_string(),
            n_total: 100,
            n_accepted: 80,
            min: 300.0,
            p10: 400.0,
            p25: 500.0,
            median: 700.0,
            p75: 1200.0,
            p90: 3000.0,
            p99: 9000.0,
            max: 50000.0,
            mean: 1500.0,
            std: 800.0,
            roundtrip_pct: Some(100.0),
            ecdf: (0..50)
                .map(|i| [300.0 + f64::from(i) * 100.0, f64::from(i) / 49.0])
                .collect(),
        };
        DialectData {
            dir_name: "postgresql".to_string(),
            display_name: "PostgreSQL".to_string(),
            has_reference: true,
            valid_total: 90,
            invalid_total: 10,
            contentious_valid: 0,
            correctness: vec![],
            perf: vec![perf],
            coverage: CoverageMatrix {
                parsers: vec![],
                files: vec![],
                subtotal_total: 0,
                subtotal_accepted: vec![],
            },
            failures: vec![],
            memory: vec![],
            batch: vec![],
        }
    }

    #[test]
    fn renders_valid_svg() {
        let d = sample();
        for svg in [ecdf_svg(&d, 760, 420), box_svg(&d, 760, 420)] {
            assert!(
                svg.starts_with("<?xml") || svg.contains("<svg"),
                "not svg: {}",
                &svg[..svg.len().min(40)]
            );
            assert!(svg.contains("</svg>"));
            assert!(svg.contains("PostgreSQL"));
            assert!(svg.len() > 500);
        }
    }
}

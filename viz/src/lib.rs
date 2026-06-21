//! Shared, wasm-clean code for the benchmark results site.
//!
//! This crate is the single source of truth for the JSON schema (produced by
//! the native `sqlbench export`, consumed by the `web` viewer) and for the chart
//! rendering. It renders charts to SVG strings with plotters' SVG backend, which
//! compiles to wasm32 (text is emitted as `<text>` and rendered by the browser,
//! so no font rasterization or fontconfig is needed). It deliberately depends on
//! nothing but `serde` and `plotters` so it stays wasm-clean.

pub mod badge;
pub mod chart;
pub mod color;
pub mod marker;
pub mod schema;

pub use chart::{
    box_lines, box_svg, ecdf_lines, ecdf_svg, mem_line, pct_trend_lines, trend_lines, year_frac,
    Line, TrendSeries,
};
pub use color::{parser_hex, parser_rgb};
pub use marker::{marker_for, Marker};
pub use schema::{
    Bundle, CoverageFile, CoverageMatrix, DepthReport, DepthScan, DialectData, DialectRun,
    FamilyHistory, FeatureCounts, FeatureScan, LintPolicy, MemDist, ParserBatch, ParserFailures,
    ParserFeatures, ParserMem, ParserMetrics, ParserPerf, RuleMeta, VersionRun,
};

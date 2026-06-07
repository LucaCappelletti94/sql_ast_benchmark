//! Shared, wasm-clean code for the benchmark results site.
//!
//! This crate is the single source of truth for the JSON schema (produced by
//! the native `sqlbench export`, consumed by the `web` viewer) and for the chart
//! rendering. It renders charts to SVG strings with plotters' SVG backend, which
//! compiles to wasm32 (text is emitted as `<text>` and rendered by the browser,
//! so no font rasterization or fontconfig is needed). It deliberately depends on
//! nothing but `serde` and `plotters` so it stays wasm-clean.

pub mod chart;
pub mod color;
pub mod schema;

pub use chart::{
    box_lines, box_svg, ecdf_lines, ecdf_svg, mem_line, trend_lines, year_frac, Line, TrendSeries,
};
pub use color::{parser_hex, parser_rgb};
pub use schema::{
    Bundle, CoverageFile, CoverageMatrix, DialectData, DialectRun, FamilyHistory, MemDist,
    ParserBatch, ParserFailures, ParserMem, ParserMetrics, ParserPerf, VersionRun,
};

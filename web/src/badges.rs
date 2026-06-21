//! Per-parser README badges, in two variants (overall rank and composite score).
//! Used by the parser page (inline previews and copy snippets) and by the
//! `badgegen` crate that writes the static `/badges/*.svg`.

use crate::components::slug;
use crate::data::bundle;
use crate::score::{parser_score, rank};

/// Left-hand label, shared by every badge.
pub const LABEL: &str = "sql ast benchmark";
/// Deployed site root, used to build the embed and link URLs.
pub const BASE: &str = "https://sql-ast-benchmark.luca.phd";

/// One renderable badge for a parser.
pub struct Variant {
    /// Stable id, also the file suffix: `dialect`, `overall`, or `score`.
    pub id: &'static str,
    /// Static file name under `/badges/`, e.g. `sqlparser_rs-overall.svg`.
    pub file: String,
    /// Right-hand text, e.g. `#1 SQLite`.
    pub message: String,
    /// Fill color for the message segment.
    pub color: &'static str,
    /// Self-contained SVG, identical to the committed static file.
    pub svg: String,
    /// Copy-paste Markdown linking the badge to the parser page.
    pub markdown: String,
}

/// Shields palette, brightest (best) to warmest (worst).
const PALETTE: [&str; 5] = ["#4c1", "#97ca00", "#a4a61d", "#dfb317", "#fe7d37"];

/// Color for a 1-based rank within a field: first place is brightest.
fn rank_color(rank: usize, total: usize) -> &'static str {
    if rank <= 1 {
        return PALETTE[0];
    }
    let f = (rank - 1) as f64 / total.saturating_sub(1).max(1) as f64;
    PALETTE[((f * 4.0).ceil() as usize).clamp(1, 4)]
}

/// Color for a 0 to 100 composite score.
fn score_color(v: f64) -> &'static str {
    match v {
        x if x >= 85.0 => PALETTE[0],
        x if x >= 70.0 => PALETTE[1],
        x if x >= 55.0 => PALETTE[2],
        x if x >= 40.0 => PALETTE[3],
        _ => PALETTE[4],
    }
}

/// Build one variant from its parts.
fn variant(parser: &str, id: &'static str, message: String, color: &'static str) -> Variant {
    let s = slug(parser);
    Variant {
        id,
        file: format!("{s}-{id}.svg"),
        markdown: format!("[![{LABEL}]({BASE}/badges/{s}-{id}.svg)]({BASE}/parser/{s})"),
        svg: viz::badge::render(LABEL, &message, color),
        message,
        color,
    }
}

/// The badge variants available for a parser, in display order.
#[must_use]
pub fn variants(parser: &str) -> Vec<Variant> {
    let mut out = Vec::new();
    if let Some((r, n)) = rank(parser, |s| Some(s.overall)) {
        out.push(variant(
            parser,
            "overall",
            format!("#{r} of {n}"),
            rank_color(r, n),
        ));
    }
    if let Some(s) = parser_score(parser) {
        out.push(variant(
            parser,
            "score",
            format!("{:.0}/100", s.overall),
            score_color(s.overall),
        ));
    }
    out
}

/// Every parser paired with its badge variants, for the static generator.
#[must_use]
pub fn all() -> Vec<(String, Vec<Variant>)> {
    bundle()
        .parsers
        .iter()
        .map(|p| (p.clone(), variants(p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parser_has_overall_and_score_variants() {
        for p in &bundle().parsers {
            let v = variants(p);
            assert_eq!(v.len(), 2, "{p} should have overall and score badges");
            assert!(v.iter().all(|x| x.svg.starts_with("<svg")));
            assert!(v.iter().any(|x| x.id == "overall") && v.iter().any(|x| x.id == "score"));
        }
    }

    #[test]
    fn markdown_links_to_the_parser_page() {
        let v = variants("sqlparser-rs");
        assert!(v[0].markdown.contains("/badges/sqlparser_rs-"));
        assert!(v[0].markdown.ends_with("/parser/sqlparser_rs)"));
    }
}

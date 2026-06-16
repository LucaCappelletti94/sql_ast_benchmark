//! Per-series marker shapes, shared by the SVG charts and the HTML legends so
//! they match. Color alone does not survive a grayscale print or a red-green
//! color-vision deficiency, so every parser and every dialect is also assigned a
//! distinct, stable glyph (triangle, square, diamond, ...). The glyph is keyed by
//! the same label the color is keyed by ([`crate::color::parser_rgb`] for parsers,
//! the dialect display name for dialects), so a series keeps one shape across all
//! charts and its legend entry.

/// A small filled glyph drawn at a data point. Every variant is rendered as a
/// single polygon (concave ones included) so one drawing path serves them all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marker {
    Circle,
    Square,
    TriangleUp,
    TriangleDown,
    Diamond,
    Pentagon,
    Hexagon,
    TriangleLeft,
    TriangleRight,
    Star,
    Plus,
    Cross,
}

/// The rotation-cycle of shapes used both for the explicit assignments below and
/// for the deterministic fallback, ordered so adjacent entries look distinct.
const CYCLE: [Marker; 12] = [
    Marker::Circle,
    Marker::Square,
    Marker::TriangleUp,
    Marker::Diamond,
    Marker::TriangleDown,
    Marker::Pentagon,
    Marker::Cross,
    Marker::Hexagon,
    Marker::TriangleLeft,
    Marker::Star,
    Marker::TriangleRight,
    Marker::Plus,
];

/// Stable marker for a series by its label. Known parsers and dialect display
/// names get an explicit, mutually distinct glyph; anything else falls back to a
/// deterministic hash over the label so unknown series still get a stable shape.
#[must_use]
pub fn marker_for(name: &str) -> Marker {
    match name {
        // Parsers (same keys as `parser_rgb`).
        "sqlparser-rs" => Marker::Circle,
        "pg_query.rs" => Marker::Square,
        "pg_query (summary)" => Marker::Plus,
        "polyglot-sql" => Marker::TriangleUp,
        "qusql-parse" => Marker::Diamond,
        "databend-common-ast" => Marker::TriangleDown,
        "sqlglot-rust" => Marker::Pentagon,
        "sqlite3-parser" => Marker::Hexagon,
        "turso_parser" => Marker::Star,
        "orql" => Marker::Cross,

        // Dialects (display names, as carried in the chart series label).
        "PostgreSQL" => Marker::Circle,
        "SQLite" => Marker::Square,
        "MySQL" => Marker::TriangleUp,
        "ClickHouse" => Marker::Diamond,
        "DuckDB" => Marker::TriangleDown,
        "Hive" => Marker::Pentagon,
        "Spark SQL" => Marker::Hexagon,
        "Trino" => Marker::TriangleLeft,
        "T-SQL" => Marker::Star,
        "Oracle" => Marker::Cross,
        "BigQuery" => Marker::TriangleRight,
        "Redshift" => Marker::Plus,

        _ => CYCLE[(fnv1a(name) % CYCLE.len() as u64) as usize],
    }
}

/// FNV-1a hash, used only to spread unknown labels across the shape cycle.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl Marker {
    /// Polygon vertices as integer pixel offsets from the marker center, for a
    /// glyph of radius `r`. plotters' backend has y increasing downward, so an
    /// "up" apex is at `-r`. Returned points wind once around the outline.
    #[must_use]
    pub fn vertices(self, r: i32) -> Vec<(i32, i32)> {
        use std::f64::consts::PI;
        let rf = f64::from(r);
        // A regular n-gon with its first vertex at angle `rot` (radians).
        let poly = |n: i32, rot: f64| -> Vec<(i32, i32)> {
            (0..n)
                .map(|i| {
                    let a = rot + f64::from(i) * 2.0 * PI / f64::from(n);
                    ((rf * a.cos()).round() as i32, (rf * a.sin()).round() as i32)
                })
                .collect()
        };
        // A k-pointed star alternating outer radius `rf` and inner `rf * inner`.
        let star = |k: i32, inner: f64, rot: f64| -> Vec<(i32, i32)> {
            (0..k * 2)
                .map(|i| {
                    let rad = if i % 2 == 0 { rf } else { rf * inner };
                    let a = rot + f64::from(i) * PI / f64::from(k);
                    (
                        (rad * a.cos()).round() as i32,
                        (rad * a.sin()).round() as i32,
                    )
                })
                .collect()
        };
        // A plus sign with arm half-width `a` reaching out to `r`.
        let plus = |rot: f64| -> Vec<(i32, i32)> {
            let a = (rf * 0.42).round();
            let base = [
                (-a, -rf),
                (a, -rf),
                (a, -a),
                (rf, -a),
                (rf, a),
                (a, a),
                (a, rf),
                (-a, rf),
                (-a, a),
                (-rf, a),
                (-rf, -a),
                (-a, -a),
            ];
            base.iter()
                .map(|&(x, y)| {
                    let (c, s) = (rot.cos(), rot.sin());
                    (
                        (x * c - y * s).round() as i32,
                        (x * s + y * c).round() as i32,
                    )
                })
                .collect()
        };
        match self {
            Marker::Circle => poly(12, 0.0),
            Marker::Square => poly(4, -PI / 4.0),
            Marker::Diamond => poly(4, -PI / 2.0),
            Marker::TriangleUp => poly(3, -PI / 2.0),
            Marker::TriangleDown => poly(3, PI / 2.0),
            Marker::TriangleLeft => poly(3, PI),
            Marker::TriangleRight => poly(3, 0.0),
            Marker::Pentagon => poly(5, -PI / 2.0),
            Marker::Hexagon => poly(6, -PI / 2.0),
            Marker::Star => star(5, 0.42, -PI / 2.0),
            Marker::Plus => plus(0.0),
            Marker::Cross => plus(PI / 4.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{marker_for, Marker, CYCLE};

    #[test]
    fn known_parsers_and_dialects_have_distinct_markers() {
        let parsers = [
            "sqlparser-rs",
            "pg_query.rs",
            "pg_query (summary)",
            "polyglot-sql",
            "qusql-parse",
            "databend-common-ast",
            "sqlglot-rust",
            "sqlite3-parser",
            "turso_parser",
            "orql",
        ];
        let mut seen: Vec<Marker> = parsers.iter().map(|p| marker_for(p)).collect();
        let n = seen.len();
        seen.sort_by_key(|m| format!("{m:?}"));
        seen.dedup();
        assert_eq!(seen.len(), n, "parser markers must be mutually distinct");

        let dialects = [
            "PostgreSQL",
            "SQLite",
            "MySQL",
            "ClickHouse",
            "DuckDB",
            "Hive",
            "Spark SQL",
            "Trino",
            "T-SQL",
            "Oracle",
            "BigQuery",
            "Redshift",
        ];
        let mut seen: Vec<Marker> = dialects.iter().map(|d| marker_for(d)).collect();
        let n = seen.len();
        seen.sort_by_key(|m| format!("{m:?}"));
        seen.dedup();
        assert_eq!(seen.len(), n, "dialect markers must be mutually distinct");
    }

    #[test]
    fn unknown_label_is_stable_and_in_cycle() {
        let a = marker_for("some-future-parser");
        let b = marker_for("some-future-parser");
        assert_eq!(a, b, "fallback must be deterministic");
        assert!(CYCLE.contains(&a));
    }

    #[test]
    fn vertices_are_nonempty_and_finite() {
        for m in CYCLE {
            let v = m.vertices(4);
            assert!(v.len() >= 3, "{m:?} needs at least 3 vertices");
        }
    }
}

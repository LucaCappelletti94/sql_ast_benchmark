//! Small numeric and formatting helpers shared by the harness and plotter.
//!
//! Kept here (rather than duplicated in `benches/` and `src/bin/`) so they can
//! be unit-tested.

/// Nearest-rank quantile of an ascending-sorted slice. `q` in `[0, 1]`.
/// Returns `0.0` for an empty slice.
#[must_use]
pub fn quantile(sorted: &[f64], q: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    let idx = ((q * (n - 1) as f64).round() as usize).min(n - 1);
    sorted[idx]
}

/// Points `(value, cumulative fraction)` tracing the empirical CDF of an
/// ascending-sorted slice.
///
/// For `n <= max_pts` every point is returned (`y = (i + 1) / n`), otherwise the
/// curve is sampled at `max_pts + 1` evenly spaced fractions. Empty input yields
/// no points.
#[must_use]
pub fn ecdf_points(sorted: &[f64], max_pts: usize) -> Vec<(f64, f64)> {
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

/// Filename-safe slug: every non-alphanumeric character becomes `_`.
/// Used to derive the `{dialect}__{parser}.txt` raw-timing file names, so the
/// benchmark (writer) and plotter (reader) must agree on it.
#[must_use]
pub fn slug(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ecdf_points, quantile, slug};

    #[test]
    fn quantile_endpoints_and_median() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((quantile(&v, 0.0) - 1.0).abs() < f64::EPSILON);
        assert!((quantile(&v, 1.0) - 5.0).abs() < f64::EPSILON);
        assert!((quantile(&v, 0.5) - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn quantile_edge_cases() {
        assert!((quantile(&[], 0.5) - 0.0).abs() < f64::EPSILON);
        assert!((quantile(&[7.0], 0.9) - 7.0).abs() < f64::EPSILON);
        // q beyond 1.0 is clamped to the last element rather than panicking.
        assert!((quantile(&[1.0, 2.0], 2.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ecdf_small_returns_all_points_ending_at_one() {
        let v = [1.0, 2.0, 3.0, 4.0];
        let pts = ecdf_points(&v, 100);
        assert_eq!(pts.len(), 4);
        assert!((pts[0].0 - 1.0).abs() < f64::EPSILON && (pts[0].1 - 0.25).abs() < f64::EPSILON);
        assert!((pts[3].1 - 1.0).abs() < f64::EPSILON);
        for w in pts.windows(2) {
            assert!(w[1].0 >= w[0].0, "x must be non-decreasing");
            assert!(w[1].1 >= w[0].1, "fraction must be non-decreasing");
        }
    }

    #[test]
    fn ecdf_large_downsamples_to_max_pts_plus_one() {
        let v: Vec<f64> = (0..1000).map(f64::from).collect();
        let pts = ecdf_points(&v, 50);
        assert_eq!(pts.len(), 51);
        assert!((pts.last().unwrap().1 - 1.0).abs() < f64::EPSILON);
        for w in pts.windows(2) {
            assert!(w[1].1 >= w[0].1);
        }
    }

    #[test]
    fn ecdf_empty_is_empty() {
        assert!(ecdf_points(&[], 10).is_empty());
    }

    #[test]
    fn slug_replaces_non_alphanumeric() {
        assert_eq!(slug("pg_query (summary)"), "pg_query__summary_");
        assert_eq!(slug("sqlparser-rs"), "sqlparser_rs");
        assert_eq!(slug("databend-common-ast"), "databend_common_ast");
        assert_eq!(slug("plain"), "plain");
    }
}

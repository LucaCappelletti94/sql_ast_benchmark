//! Per-dialect brand accent colors.
//!
//! These are each engine's recognizable brand color (a fact, not protected
//! artwork). They drive the accent on cards and dialect headers.

/// Accent color (hex string and RGB tuple) plus a foreground color that reads
/// accessibly on top of it.
pub struct Brand {
    pub accent: &'static str,
    pub accent_rgb: (u8, u8, u8),
    pub on_accent: &'static str,
}

/// Brand colors for a dialect's `dir_name`.
#[must_use]
pub const fn brand(dir: &str) -> Brand {
    let (accent, accent_rgb, on_accent) = match dir.as_bytes() {
        b"postgresql" => ("#336791", (0x33, 0x67, 0x91), "#ffffff"),
        b"sqlite" => ("#003b57", (0x00, 0x3b, 0x57), "#ffffff"),
        b"mysql" => ("#00758f", (0x00, 0x75, 0x8f), "#ffffff"),
        b"clickhouse" => ("#ffcc00", (0xff, 0xcc, 0x00), "#1c1c20"),
        b"duckdb" => ("#fff000", (0xff, 0xf0, 0x00), "#1c1c20"),
        b"hive" => ("#fdee21", (0xfd, 0xee, 0x21), "#1c1c20"),
        b"spark_sql" => ("#e25a1c", (0xe2, 0x5a, 0x1c), "#ffffff"),
        b"trino" => ("#dd00a1", (0xdd, 0x00, 0xa1), "#ffffff"),
        b"tsql" => ("#cc2927", (0xcc, 0x29, 0x27), "#ffffff"),
        b"oracle" => ("#c74634", (0xc7, 0x46, 0x34), "#ffffff"),
        b"bigquery" => ("#4285f4", (0x42, 0x85, 0xf4), "#ffffff"),
        b"redshift" => ("#8c4fff", (0x8c, 0x4f, 0xff), "#ffffff"),
        _ => ("#5f6b7a", (0x5f, 0x6b, 0x7a), "#ffffff"),
    };
    Brand {
        accent,
        accent_rgb,
        on_accent,
    }
}

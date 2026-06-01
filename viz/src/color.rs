//! Parser color palette, shared by the SVG charts and the HTML legends so they
//! match. Mirrors the palette in the native `plot.rs`.

/// RGB color for a parser's curves/boxes by display name.
#[must_use]
pub const fn parser_rgb(name: &str) -> (u8, u8, u8) {
    match name.as_bytes() {
        b"sqlparser-rs" => (15, 76, 129),
        b"pg_query.rs" => (255, 111, 97),
        b"pg_query (summary)" => (214, 153, 150),
        b"polyglot-sql" => (230, 200, 40),
        b"qusql-parse" => (95, 75, 139),
        b"databend-common-ast" => (0, 155, 119),
        b"sqlglot-rust" => (237, 135, 45),
        b"sqlite3-parser" => (0, 128, 128),
        b"orql" => (139, 69, 19),
        _ => (120, 120, 120),
    }
}

/// CSS hex string (e.g. `#0f4c81`) for a parser, for HTML legends.
#[must_use]
pub fn parser_hex(name: &str) -> String {
    let (r, g, b) = parser_rgb(name);
    format!("#{r:02x}{g:02x}{b:02x}")
}

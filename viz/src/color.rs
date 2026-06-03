//! Parser color palette, shared by the SVG charts and the HTML legends so they
//! match.

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
        b"turso_parser" => (198, 66, 133),
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

#[cfg(test)]
mod tests {
    use super::{parser_hex, parser_rgb};

    #[test]
    fn known_parser_has_its_palette_color() {
        assert_eq!(parser_rgb("sqlparser-rs"), (15, 76, 129));
        assert_eq!(parser_hex("sqlparser-rs"), "#0f4c81");
    }

    #[test]
    fn unknown_parser_falls_back_to_grey() {
        assert_eq!(parser_rgb("does-not-exist"), (120, 120, 120));
        assert_eq!(parser_hex("does-not-exist"), "#787878");
    }
}

//! Pure renderer for shields-style flat SVG badges, shared by the website and
//! the static `/badges/*.svg` generator so both emit identical output. Text is
//! sized from a Verdana width table and carries an SVG `textLength`, so the
//! browser scales each string to the computed width regardless of the font.

/// Approximate Verdana advance width (px at font-size 11) for one character.
fn char_width(c: char) -> f64 {
    match c {
        ' ' | '!' | '\'' | '.' | ',' => 3.8,
        '#' => 8.9,
        '$' | '0'..='9' => 7.0,
        '(' | ')' | '-' | '/' | ':' => 4.6,
        'i' | 'j' | 'l' => 3.0,
        'f' | 't' | 'r' => 4.4,
        'm' => 10.6,
        'w' => 9.0,
        'M' | 'W' => 10.6,
        'I' | 'J' => 4.6,
        'a'..='z' => 6.6,
        'A'..='Z' => 8.2,
        _ => 6.8,
    }
}

fn text_width(s: &str) -> f64 {
    s.chars().map(char_width).sum()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A flat badge: dark grey `label`, then `message` filled with `color` (e.g.
/// `#4c1`). Returns a self-contained 20px-tall SVG.
#[must_use]
pub fn render(label: &str, message: &str, color: &str) -> String {
    let lw = (text_width(label) + 10.0).round() as i64;
    let mw = (text_width(message) + 10.0).round() as i64;
    let total = lw + mw;
    let lcx = lw * 5;
    let mcx = lw * 10 + mw * 5;
    let ltl = (text_width(label) * 10.0).round() as i64;
    let mtl = (text_width(message) * 10.0).round() as i64;
    let (l, m, c) = (esc(label), esc(message), esc(color));
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total}\" height=\"20\" role=\"img\" \
aria-label=\"{l}: {m}\"><title>{l}: {m}</title>\
<linearGradient id=\"s\" x2=\"0\" y2=\"100%\"><stop offset=\"0\" stop-color=\"#bbb\" stop-opacity=\".1\"/>\
<stop offset=\"1\" stop-opacity=\".1\"/></linearGradient>\
<clipPath id=\"r\"><rect width=\"{total}\" height=\"20\" rx=\"3\" fill=\"#fff\"/></clipPath>\
<g clip-path=\"url(#r)\"><rect width=\"{lw}\" height=\"20\" fill=\"#555\"/>\
<rect x=\"{lw}\" width=\"{mw}\" height=\"20\" fill=\"{c}\"/>\
<rect width=\"{total}\" height=\"20\" fill=\"url(#s)\"/></g>\
<g fill=\"#fff\" text-anchor=\"middle\" font-family=\"Verdana,Geneva,DejaVu Sans,sans-serif\" \
text-rendering=\"geometricPrecision\" font-size=\"110\">\
<text aria-hidden=\"true\" x=\"{lcx}\" y=\"150\" fill=\"#010101\" fill-opacity=\".3\" transform=\"scale(.1)\" textLength=\"{ltl}\">{l}</text>\
<text x=\"{lcx}\" y=\"140\" transform=\"scale(.1)\" textLength=\"{ltl}\">{l}</text>\
<text aria-hidden=\"true\" x=\"{mcx}\" y=\"150\" fill=\"#010101\" fill-opacity=\".3\" transform=\"scale(.1)\" textLength=\"{mtl}\">{m}</text>\
<text x=\"{mcx}\" y=\"140\" transform=\"scale(.1)\" textLength=\"{mtl}\">{m}</text></g></svg>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_contains_both_segments() {
        let svg = render("sql ast benchmark", "#1 SQLite", "#4c1");
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert!(svg.contains("sql ast benchmark") && svg.contains("#1 SQLite"));
        assert!(svg.contains("fill=\"#4c1\""));
    }

    #[test]
    fn wider_message_yields_wider_badge() {
        let w = |s: &str| {
            let a = s.find("width=\"").unwrap() + 7;
            s[a..a + s[a..].find('"').unwrap()].parse::<i64>().unwrap()
        };
        assert!(
            w(&render("bench", "#1 of 99 parsers", "#4c1")) > w(&render("bench", "#1", "#4c1"))
        );
    }

    #[test]
    fn escapes_xml() {
        let svg = render("a&b", "x<y>", "#4c1");
        assert!(svg.contains("a&amp;b") && svg.contains("x&lt;y&gt;"));
    }
}

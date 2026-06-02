//! Page components, the shared shell layout, and table helpers.

use crate::brand::brand;
use crate::data::bundle;
use crate::Route;
use dioxus::prelude::*;
use dioxus_free_icons::icons::fa_brands_icons::{FaGit, FaGithub, FaRust};
use dioxus_free_icons::icons::fa_solid_icons::{
    FaArrowLeftLong, FaBox, FaBug, FaCalendarDays, FaChartLine, FaCode, FaCodeCommit, FaCodeFork,
    FaCopy, FaCube, FaDatabase, FaDownload, FaFlaskVial, FaHeartPulse, FaMicrochip,
    FaScaleBalanced, FaShieldHalved, FaStar, FaStopwatch, FaTableCells, FaTriangleExclamation,
    FaUsers, FaVial,
};
use dioxus_free_icons::Icon;
use std::cmp::Ordering;
use viz::{parser_hex, parser_rgb, DialectData, ParserMetrics, ParserPerf};

const REPO: &str = "https://github.com/LucaCappelletti94/sql_ast_benchmark";

/// The chart currently shown enlarged in the lightbox, if any: `(filename, svg)`
/// where `filename` is the download base name and `svg` is the chart markup. A
/// global signal so any [`chart_figure`] can open it and the [`Shell`] overlay
/// can render it (with its own download buttons). `None` means it is closed.
static ZOOM: GlobalSignal<Option<(String, String)>> = Signal::global(|| None);

/// A committed full-color logo file for a dialect, if one is shipped in
/// `web/assets/logos/`. `asset!` needs a literal path, hence the match.
fn logo_asset(dir: &str) -> Option<Asset> {
    Some(match dir {
        "oracle" => asset!("/assets/logos/oracle.svg"),
        "redshift" => asset!("/assets/logos/redshift.svg"),
        "tsql" => asset!("/assets/logos/tsql.svg"),
        _ => return None,
    })
}

/// The dialect's chip: a light chip with the full-color logo image where one is
/// shipped, otherwise a brand-color chip with the silhouette/monogram/glyph.
fn mark(dir: &str, glyph_size: u32, chip_extra: &str) -> Element {
    let br = brand(dir);
    if let Some(logo) = logo_asset(dir) {
        rsx! {
            span { class: "chip chip-light {chip_extra}", "aria-hidden": "true",
                img { class: "logo-img", src: logo, alt: "" }
            }
        }
    } else {
        rsx! {
            span {
                class: "chip {chip_extra}",
                style: "background: {br.accent}; color: {br.on_accent};",
                "aria-hidden": "true",
                {dialect_glyph(dir, glyph_size, br.on_accent)}
            }
        }
    }
}

/// The dialect's silhouette glyph (tinted `fill`): the openly-licensed brand
/// glyph where one exists, else a generic database icon. Decorative, so hidden
/// from assistive tech. (Engines with a shipped logo file are handled by
/// [`mark`] before this is reached.)
fn dialect_glyph(dir: &str, size: u32, fill: &str) -> Element {
    if let Some(d) = crate::logos::logo_path(dir) {
        rsx! {
            svg {
                xmlns: "http://www.w3.org/2000/svg",
                "viewBox": "0 0 24 24",
                width: "{size}",
                height: "{size}",
                "aria-hidden": "true",
                path { d: "{d}", fill: "{fill}" }
            }
        }
    } else {
        rsx! {
            Icon { width: size, height: size, fill: fill.to_string(), icon: FaDatabase }
        }
    }
}

/// A committed logo file for a parser, where one cleanly exists. `sqlparser-rs`
/// lives under the Apache umbrella (the feather is the ASF's project mark), and
/// `databend-common-ast` is part of Databend (Apache-2.0). `asset!` needs a
/// literal path, hence the match.
fn parser_logo_asset(name: &str) -> Option<Asset> {
    Some(match name {
        "sqlparser-rs" => asset!("/assets/logos/apache.svg"),
        "databend-common-ast" => asset!("/assets/logos/databend.svg"),
        "pg_query.rs" | "pg_query (summary)" => asset!("/assets/logos/postgresql.svg"),
        _ => return None,
    })
}

/// A short letter monogram for a parser that has no distinct logo. Kept
/// lowercase to echo the parser's own crate-name styling.
fn parser_monogram(name: &str) -> &'static str {
    match name {
        "pg_query.rs" | "pg_query (summary)" => "pg",
        "qusql-parse" => "qu",
        "polyglot-sql" => "px",
        "orql" => "or",
        "sqlglot-rust" => "sg",
        "sqlite3-parser" => "s3",
        _ => "sql",
    }
}

/// The parser's chip: a light chip with its full-color logo where one is
/// shipped, otherwise a palette-colored chip with a letter monogram.
/// `mono_px` sizes the monogram so it fits the chip (cards vs. the larger hero).
fn parser_mark(name: &str, mono_px: u32, chip_extra: &str) -> Element {
    if let Some(logo) = parser_logo_asset(name) {
        rsx! {
            span { class: "chip chip-light {chip_extra}", "aria-hidden": "true",
                img { class: "logo-img", src: logo, alt: "" }
            }
        }
    } else {
        rsx! {
            span {
                class: "chip {chip_extra}",
                style: "background: {parser_hex(name)}; color: {readable_on(parser_rgb(name))};",
                "aria-hidden": "true",
                span { class: "mono", style: "font-size: {mono_px}px;", "{parser_monogram(name)}" }
            }
        }
    }
}

/// Shared chrome wrapping every route: skip link, header, `main`, footer.
#[component]
pub fn Shell() -> Element {
    rsx! {
        a { class: "skip", href: "#content", "Skip to content" }
        header { class: "site",
            Link { class: "brand", to: Route::Overview {},
                img { class: "brand-logo", src: crate::FAVICON, alt: "", width: "26", height: "26" }
                span { "Rust SQL Parser Benchmark" }
            }
            a {
                class: "ghlink",
                href: REPO,
                "aria-label": "Source on GitHub",
                Icon { width: 22, height: 22, fill: "currentColor".to_string(), title: "GitHub".to_string(), icon: FaGithub }
            }
        }
        main { id: "content", Outlet::<Route> {} }
        footer { class: "site-foot",
            "Charts are rendered in your browser from a single committed "
            code { "bench.json" }
            ". "
            a { href: REPO, "Source on GitHub" }
        }
        if let Some((filename, svg)) = ZOOM() {
            // The enlarged copy carries its own id so the same download scripts
            // can target it, keeping the PNG/SVG buttons usable while zoomed.
            {
                let png_js = download_js("lightbox-fig", &filename, true);
                let svg_js = download_js("lightbox-fig", &filename, false);
                rsx! {
                    div {
                        class: "lightbox",
                        role: "dialog",
                        "aria-modal": "true",
                        "aria-label": "Enlarged chart",
                        tabindex: 0,
                        autofocus: true,
                        onclick: move |_| { *ZOOM.write() = None; },
                        onkeydown: move |e| {
                            if e.key() == Key::Escape {
                                *ZOOM.write() = None;
                            }
                        },
                        button {
                            class: "lightbox-close",
                            "aria-label": "Close enlarged chart",
                            onclick: move |e| {
                                e.stop_propagation();
                                *ZOOM.write() = None;
                            },
                            "\u{00d7}"
                        }
                        div {
                            class: "lightbox-inner",
                            onclick: move |e| e.stop_propagation(),
                            div {
                                id: "lightbox-fig",
                                class: "lightbox-svg",
                                dangerous_inner_html: "{svg}"
                            }
                            div { class: "chart-tools lightbox-tools",
                                button {
                                    class: "dl-btn",
                                    "aria-label": "Download {filename} as PNG",
                                    onclick: move |e| {
                                        e.stop_propagation();
                                        document::eval(&png_js);
                                    },
                                    Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaDownload }
                                    "PNG"
                                }
                                button {
                                    class: "dl-btn",
                                    "aria-label": "Download {filename} as SVG",
                                    onclick: move |e| {
                                        e.stop_propagation();
                                        document::eval(&svg_js);
                                    },
                                    Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaDownload }
                                    "SVG"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// URL-safe slug for a parser display name (matches the route param).
fn slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_us = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_us = false;
        } else if !prev_us {
            out.push('_');
            prev_us = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// A readable foreground (`#fff` or near-black) for text/icons over `rgb`.
fn readable_on(rgb: (u8, u8, u8)) -> &'static str {
    let lum = 0.2126 * f64::from(rgb.0) + 0.7152 * f64::from(rgb.1) + 0.0722 * f64::from(rgb.2);
    if lum > 150.0 {
        "#1c1c20"
    } else {
        "#ffffff"
    }
}

/// A small icon + text pill, used on cards to surface a primary library fact.
/// `desc` is a full sentence shown as a tooltip and read by assistive tech.
fn mini_pill(icon: Element, text: String, desc: String) -> Element {
    rsx! {
        span { class: "mini-pill", title: "{desc}", "aria-label": "{desc}",
            span { class: "mini-ico", "aria-hidden": "true", {icon} }
            span { "aria-hidden": "true", "{text}" }
        }
    }
}

/// A compact row of primary metadata pills for a parser card: stars, all-time
/// downloads, and license. Renders nothing for a parser with no recorded
/// metadata.
fn card_meta_pills(name: &str) -> Element {
    let Some(m) = crate::metadata::parser_meta(name) else {
        return rsx! {};
    };
    rsx! {
        span { class: "card-pills",
            {mini_pill(rsx! { Icon { width: 10, height: 10, fill: "currentColor".to_string(), icon: FaStar } }, commas(m.stars as usize), crate::metadata::stars_description(m.stars))}
            {mini_pill(rsx! { Icon { width: 10, height: 10, fill: "currentColor".to_string(), icon: FaDownload } }, m.downloads.to_string(), crate::metadata::downloads_description(m.downloads))}
            {mini_pill(rsx! { Icon { width: 10, height: 10, fill: "currentColor".to_string(), icon: FaScaleBalanced } }, m.license.to_string(), crate::metadata::license_description(m.license))}
        }
    }
}

/// How many dialects a parser appears in.
fn parser_dialect_count(parser: &str) -> usize {
    bundle()
        .dialects
        .iter()
        .filter(|d| {
            d.perf.iter().any(|p| p.parser == parser)
                || d.correctness.iter().any(|m| m.parser == parser)
        })
        .count()
}

/// How many distinct parsers were benchmarked on a dialect (any that produced
/// timing or correctness data for it).
fn dialect_parser_count(d: &DialectData) -> usize {
    let mut names: Vec<&str> = d.perf.iter().map(|p| p.parser.as_str()).collect();
    for m in &d.correctness {
        if !names.contains(&m.parser.as_str()) {
            names.push(m.parser.as_str());
        }
    }
    names.sort_unstable();
    names.dedup();
    names.len()
}

/// Landing page: intro + a card per dialect.
#[component]
pub fn Overview() -> Element {
    let b = bundle();
    rsx! {
        section { class: "intro",
            h1 { "Rust SQL Parser Benchmark" }
            p { class: "meta",
                "Snapshot {b.generated_utc}"
                if let Some(c) = &b.git_commit { " | commit {c}" }
            }
        }
        section { class: "abstract intro-abstract",
            p { class: "blurb",
                {rich_text("Choosing a SQL parser for a Rust project means weighing dialect coverage, correctness, and speed, yet those trade-offs are seldom measured on realistic input. This project benchmarks the actively maintained Rust SQL parsers on a large, multi-dialect corpus of real-world statements so the choice can rest on evidence rather than on each library's own claims.").into_iter()}
            }
            p { class: "blurb",
                {rich_text(&format!("The study evaluates eight parser libraries: [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) (Apache DataFusion), [pg_query.rs](https://github.com/pganalyze/pg_query.rs) and its faster summary mode (Rust bindings to [libpg_query](https://github.com/pganalyze/libpg_query), PostgreSQL's own parser), [databend-common-ast](https://crates.io/crates/databend-common-ast), [polyglot-sql](https://github.com/tobilg/polyglot), [sqlglot-rust](https://crates.io/crates/sqlglot-rust), [qusql-parse](https://crates.io/crates/qusql-parse), and [sqlite3-parser](https://crates.io/crates/sqlite3-parser) (lemon-rs), plus [orql](https://codeberg.org/xitep/orql) on Oracle. They run against a corpus of 311,594 statements spanning these {} dialects, drawn from each engine's own regression suites and official samples and committed compressed so every run is reproducible.", b.dialects.len())).into_iter()}
            }
            p { class: "blurb",
                {rich_text("Each parser is exercised in the dialect that matches the corpus under test. Where a ground-truth parser exists, [libpg_query](https://github.com/pganalyze/libpg_query) for PostgreSQL and [lemon-rs](https://github.com/gwenn/lemon-rs) for SQLite, it labels each statement valid or invalid, and the parsers are scored on recall (valid statements accepted), false positives (invalid statements wrongly accepted), display round-trip stability, and canonical-form fidelity. The other dialects have no such authority, so their statements count as provenance-valid and the metric is simply the acceptance rate. Across all dialects, speed is captured as a per-statement parse-time distribution over every accepted statement.").into_iter()}
            }
            p { class: "blurb",
                {rich_text("On their home dialect the reference bindings are exact by construction, so the more telling comparison is among the pure-Rust parsers. There, [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) is the most broadly capable, the permissive parsers such as [polyglot-sql](https://github.com/tobilg/polyglot) accept the most statements but pay for it with a high false-positive rate, and the stricter parsers reject more in exchange for precision. Speed spans more than an order of magnitude, from well under a microsecond per statement for the fastest parsers to the low single-digit microseconds for most, with [polyglot-sql](https://github.com/tobilg/polyglot) a clear outlier at roughly fifteen. No parser leads on every axis, so the right choice comes down to what a given project values most: broad coverage, few false positives, or raw speed.").into_iter()}
            }
        }
        div { class: "section-head",
            h2 {
                Icon { width: 18, height: 18, fill: "currentColor".to_string(), class: "h2-ico".to_string(), icon: FaDatabase }
                "Browse by dialect"
            }
        }
        ul { class: "cards", "aria-label": "SQL dialects",
            for d in &b.dialects {
                li { key: "{d.dir_name}",
                    Link {
                        class: "card",
                        style: "--accent: {brand(&d.dir_name).accent};",
                        to: Route::DialectView { dir: d.dir_name.clone() },
                        "aria-label": "{d.display_name} dialect, {count_noun(d.valid_total + d.invalid_total, \"statement\")}, benchmarked with {count_noun(dialect_parser_count(d), \"parser\")}",
                        {mark(&d.dir_name, 20, "")}
                        span { class: "card-body",
                            span { class: "card-title", "{d.display_name}" }
                            span { class: "card-meta",
                                span { class: "card-n", "{count_noun(d.valid_total + d.invalid_total, \"statement\")}" }
                                span {
                                    class: "badge badge-count",
                                    title: "{count_noun(dialect_parser_count(d), \"parser\")} were benchmarked on this dialect.",
                                    "aria-hidden": "true",
                                    "{count_noun(dialect_parser_count(d), \"parser\")}"
                                }
                                span {
                                    class: if d.has_reference { "badge badge-reference" } else { "badge badge-prov" },
                                    title: if d.has_reference { "Graded against a reference parser (libpg_query or lemon-rs): each statement is labelled valid or invalid as ground truth." } else { "No reference parser for this dialect, so acceptance is graded by provenance: statements come from the engine's own test suites and samples." },
                                    "aria-label": if d.has_reference { "Reference-graded dialect" } else { "Acceptance-rate graded dialect" },
                                    if d.has_reference { "reference" } else { "acceptance" }
                                }
                            }
                        }
                    }
                }
            }
        }

        div { class: "section-head",
            h2 {
                Icon { width: 18, height: 18, fill: "currentColor".to_string(), class: "h2-ico".to_string(), icon: FaCode }
                "Browse by parser"
            }
        }
        ul { class: "cards cards-3", "aria-label": "SQL parsers",
            for name in &b.parsers {
                li { key: "{name}",
                    Link {
                        class: "card",
                        style: "--accent: {parser_hex(name)};",
                        to: Route::ParserView { name: slug(name) },
                        "aria-label": "{name} parser, modelling {count_noun(parser_dialect_count(name), \"dialect\")}",
                        {parser_mark(name, 16, "")}
                        span { class: "card-body",
                            span { class: "card-title", "{name}" }
                            span { class: "card-meta",
                                span { class: "card-n", "{count_noun(parser_dialect_count(name), \"dialect\")}" }
                            }
                            {card_meta_pills(name)}
                        }
                    }
                }
            }
        }
    }
}

/// Browser script that serializes the inline `<svg>` inside a figure and saves
/// it verbatim as an `.svg` file. `__FIG__`/`__NAME__` are replaced with the
/// figure id and download base name (both app-controlled slugs).
const SVG_DL_JS: &str = r#"
(function(){
  const fig = document.getElementById('__FIG__');
  if (!fig) return;
  const svg = fig.querySelector('svg');
  if (!svg) return;
  const xml = new XMLSerializer().serializeToString(svg);
  const blob = new Blob([xml], {type: 'image/svg+xml;charset=utf-8'});
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = '__NAME__.svg';
  document.body.appendChild(a); a.click(); a.remove();
  setTimeout(function(){ URL.revokeObjectURL(a.href); }, 0);
})();
"#;

/// Browser script that rasterizes the inline `<svg>` to a 2x white-background
/// PNG via a canvas and saves it. Placeholders as in [`SVG_DL_JS`].
const PNG_DL_JS: &str = r#"
(function(){
  const fig = document.getElementById('__FIG__');
  if (!fig) return;
  const svg = fig.querySelector('svg');
  if (!svg) return;
  const vb = svg.viewBox.baseVal;
  const w = (vb && vb.width) ? vb.width : (svg.clientWidth || 760);
  const h = (vb && vb.height) ? vb.height : (svg.clientHeight || 420);
  const xml = new XMLSerializer().serializeToString(svg);
  const src = 'data:image/svg+xml;base64,' + btoa(unescape(encodeURIComponent(xml)));
  const img = new Image();
  img.onload = function(){
    const scale = 2;
    const c = document.createElement('canvas');
    c.width = Math.round(w * scale); c.height = Math.round(h * scale);
    const ctx = c.getContext('2d');
    ctx.fillStyle = '#ffffff'; ctx.fillRect(0, 0, c.width, c.height);
    ctx.scale(scale, scale);
    ctx.drawImage(img, 0, 0, w, h);
    c.toBlob(function(b){
      const a = document.createElement('a');
      a.href = URL.createObjectURL(b);
      a.download = '__NAME__.png';
      document.body.appendChild(a); a.click(); a.remove();
      setTimeout(function(){ URL.revokeObjectURL(a.href); }, 0);
    });
  };
  img.src = src;
})();
"#;

/// The download script for figure `fig_id`, saving as `filename.{png|svg}`.
fn download_js(fig_id: &str, filename: &str, png: bool) -> String {
    let tmpl = if png { PNG_DL_JS } else { SVG_DL_JS };
    tmpl.replace("__FIG__", fig_id)
        .replace("__NAME__", filename)
}

/// A chart figure: the inline SVG plus a caption and PNG/SVG download buttons.
/// `id` must be unique per figure on the page (the download script locates the
/// SVG by it); `filename` is the saved file's base name (no extension).
fn chart_figure(id: &str, svg: &str, aria_label: &str, caption: &str, filename: &str) -> Element {
    let png_js = download_js(id, filename, true);
    let svg_js = download_js(id, filename, false);
    let zoom = (filename.to_string(), svg.to_string());
    rsx! {
        figure { class: "chart", id: "{id}", role: "img", "aria-label": "{aria_label}",
            // The enlarge-button and the download tools are siblings inside a
            // positioned frame (buttons cannot nest), so the tools overlay the
            // chart image without being part of its clickable enlarge area.
            div { class: "chart-frame",
                button {
                    class: "chart-svg",
                    "aria-label": "Enlarge chart: {aria_label}",
                    onclick: move |_| {
                        *ZOOM.write() = Some(zoom.clone());
                        // Focus the overlay on the next frame so Escape-to-close
                        // works (a div's `autofocus` does not fire on dynamic insert).
                        document::eval(
                            "requestAnimationFrame(() => document.querySelector('.lightbox')?.focus());",
                        );
                    },
                    span { "aria-hidden": "true", dangerous_inner_html: "{svg}" }
                }
                div { class: "chart-tools",
                    button {
                        class: "dl-btn",
                        "aria-label": "Download {filename} as PNG",
                        onclick: move |_| { document::eval(&png_js); },
                        Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaDownload }
                        "PNG"
                    }
                    button {
                        class: "dl-btn",
                        "aria-label": "Download {filename} as SVG",
                        onclick: move |_| { document::eval(&svg_js); },
                        Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaDownload }
                        "SVG"
                    }
                }
            }
            figcaption { "{caption}" }
        }
    }
}

/// Per-dialect detail page.
#[component]
pub fn DialectView(dir: String) -> Element {
    let b = bundle();
    let Some(d) = b.dialects.iter().find(|x| x.dir_name == dir) else {
        return rsx! {
            section { class: "intro",
                h1 { "Unknown dialect" }
                p { "No data for \"{dir}\"." }
                Link { class: "back", to: Route::Overview {}, "Back to all dialects" }
            }
        };
    };
    let br = brand(&d.dir_name);
    let total = commas(d.valid_total + d.invalid_total);
    let ecdf = viz::ecdf_svg(d, 760, 420);
    let boxp = viz::box_svg(d, 760, 420);

    rsx! {
        section {
            class: "hero",
            style: "--accent: {br.accent};",
            div { class: "hero-row",
                {mark(&d.dir_name, 28, "lg")}
                div {
                    h1 { "{d.display_name}" }
                    p { class: "hero-stats",
                        span { class: "stat", strong { "{total}" } {if d.valid_total + d.invalid_total == 1 { " statement" } else { " statements" }} }
                        if d.has_reference {
                            span { class: "stat", strong { "{commas(d.valid_total)}" } " reference-valid" }
                            span { class: "stat", strong { "{commas(d.invalid_total)}" } " reference-invalid" }
                        } else {
                            span { class: "stat", "acceptance-rate graded (no reference)" }
                        }
                    }
                }
            }
        }

        {blurb(crate::descriptions::dialect_blurb(&d.dir_name))}

        section { class: "block",
            h2 {
                Icon { width: 17, height: 17, fill: "currentColor".to_string(), class: "h2-ico".to_string(), icon: FaChartLine }
                "Per-statement parse time"
            }
            div { class: "charts",
                {chart_figure(
                    &format!("chart-{}-ecdf", d.dir_name),
                    &ecdf,
                    &format!("Empirical CDF of per-statement parse time for {}, one curve per parser.", d.display_name),
                    "Parse time per statement, one curve per parser. X axis is ns per statement (log), Y axis is the fraction of accepted statements parsed within that time, so further left is faster. In the legend, \"missed\" is reference-valid statements not accepted (one minus recall, or the unaccepted fraction with no reference parser). \"RT\" is the round-trip rate, accepted statements that re-parse unchanged (n/a without a printer).",
                    &format!("{}-ecdf", d.dir_name),
                )}
                {chart_figure(
                    &format!("chart-{}-box", d.dir_name),
                    &boxp,
                    &format!("Box plot of per-statement parse time for {}, one box per parser.", d.display_name),
                    "Parse time per statement, one box per parser, log scale. Box spans the 25th to 75th percentile with the median inside, whiskers reach the 10th and 90th. In the legend, \"missed\" is reference-valid statements not accepted (one minus recall, or the unaccepted fraction with no reference parser). \"RT\" is the round-trip rate, accepted statements that re-parse unchanged (n/a without a printer).",
                    &format!("{}-boxplot", d.dir_name),
                )}
            }
        }

        {perf_table(d)}
        {correctness_table(d)}

        Link { class: "back", to: Route::Overview {},
            Icon { width: 14, height: 14, fill: "currentColor".to_string(), icon: FaArrowLeftLong }
            "All dialects"
        }
    }
}

/// Per-parser detail page: how one parser does across the dialects it supports.
#[component]
pub fn ParserView(name: String) -> Element {
    let b = bundle();
    let Some(parser) = b.parsers.iter().find(|p| slug(p) == name).cloned() else {
        return rsx! {
            section { class: "intro",
                h1 { "Unknown parser" }
                p { "No data for \"{name}\"." }
                Link { class: "back", to: Route::Overview {}, "Back to all dialects" }
            }
        };
    };
    let phex = parser_hex(&parser);

    let rows: Vec<(&DialectData, Option<&ParserMetrics>, Option<&ParserPerf>)> = b
        .dialects
        .iter()
        .filter_map(|d| {
            let m = d.correctness.iter().find(|m| m.parser == parser);
            let p = d.perf.iter().find(|p| p.parser == parser);
            (m.is_some() || p.is_some()).then_some((d, m, p))
        })
        .collect();

    let lines: Vec<viz::Line> = rows
        .iter()
        .filter_map(|&(d, _, p)| {
            p.map(|p| viz::Line {
                label: d.display_name.clone(),
                rgb: brand(&d.dir_name).accent_rgb,
                sub: Some(format!("median {} ns", commas(p.median as usize))),
                min: p.min,
                p10: p.p10,
                p25: p.p25,
                median: p.median,
                p75: p.p75,
                p90: p.p90,
                p99: p.p99,
                ecdf: p.ecdf.clone(),
            })
        })
        .collect();
    let has_charts = !lines.is_empty();
    let ecdf = if has_charts {
        viz::ecdf_lines(&parser, &lines, 760, 460)
    } else {
        String::new()
    };
    let boxp = if has_charts {
        viz::box_lines(&parser, &lines, 760, 460)
    } else {
        String::new()
    };

    let across_columns: Vec<String> = [
        "accept / recall",
        "false pos",
        "round-trip",
        "fidelity",
        "median ns",
        "p90 ns",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let across_rows: Vec<Row> = rows
        .iter()
        .map(|&(d, m, p)| Row {
            key: d.dir_name.clone(),
            head: Head::Dialect {
                dir: d.dir_name.clone(),
                name: d.display_name.clone(),
            },
            cells: vec![
                Cell::pct(m.and_then(|m| {
                    if d.has_reference {
                        m.recall_pct
                    } else {
                        m.accept_pct
                    }
                })),
                Cell::pct(m.and_then(|m| m.false_positive_pct)),
                Cell::pct(m.and_then(|m| m.roundtrip_pct)),
                Cell::pct(m.and_then(|m| m.fidelity_pct)),
                Cell::ns(p.map(|p| p.median)),
                Cell::ns(p.map(|p| p.p90)),
            ],
        })
        .collect();

    rsx! {
        section { class: "hero", style: "--accent: {phex};",
            div { class: "hero-row",
                {parser_mark(&parser, 22, "lg")}
                div {
                    h1 { "{parser}" }
                    p { class: "hero-stats",
                        span { class: "stat", strong { "{rows.len()}" } " of {b.dialects.len()} dialects" }
                    }
                }
                {parser_meta_pills(&parser)}
            }
        }

        {blurb(crate::descriptions::parser_blurb(&parser))}

        if has_charts {
            section { class: "block",
                h2 {
                    Icon { width: 17, height: 17, fill: "currentColor".to_string(), class: "h2-ico".to_string(), icon: FaChartLine }
                    "Parse time across dialects"
                }
                div { class: "charts",
                    {chart_figure(
                        &format!("chart-{}-ecdf", slug(&parser)),
                        &ecdf,
                        &format!("Empirical CDF of {parser} parse time, one curve per dialect."),
                        "Parse time per statement, one curve per dialect this parser models. X axis is ns per statement (log), Y axis is the fraction of accepted statements parsed within that time, so further left is faster. In the legend, \"missed\" is reference-valid statements not accepted (one minus recall, or the unaccepted fraction with no reference parser). \"RT\" is the round-trip rate, accepted statements that re-parse unchanged (n/a without a printer).",
                        &format!("{}-ecdf", slug(&parser)),
                    )}
                    {chart_figure(
                        &format!("chart-{}-box", slug(&parser)),
                        &boxp,
                        &format!("Box plot of {parser} parse time, one box per dialect."),
                        "Parse time per statement, one box per dialect this parser models, log scale. Box spans the 25th to 75th percentile with the median inside, whiskers reach the 10th and 90th. In the legend, \"missed\" is reference-valid statements not accepted (one minus recall, or the unaccepted fraction with no reference parser). \"RT\" is the round-trip rate, accepted statements that re-parse unchanged (n/a without a printer).",
                        &format!("{}-boxplot", slug(&parser)),
                    )}
                }
            }
        }

        section { class: "block",
            h2 {
                Icon { width: 17, height: 17, fill: "currentColor".to_string(), class: "h2-ico".to_string(), icon: FaTableCells }
                "Results by dialect"
            }
            SortTable {
                caption: format!("Per-dialect results for {}", parser),
                corner: "dialect".to_string(),
                columns: across_columns.clone(),
                rows: across_rows.clone(),
                footer: None,
            }
        }

        {failures_section(b, &parser)}

        Link { class: "back", to: Route::Overview {},
            Icon { width: 14, height: 14, fill: "currentColor".to_string(), icon: FaArrowLeftLong }
            "All dialects & parsers"
        }
    }
}

/// The "Failing statements" section for a parser: per dialect it models, the
/// rejected-statement count, a short syntax-highlighted preview, and a download
/// link to the full capped `.tsv.zst`. Renders nothing if the parser rejected
/// nothing anywhere (or has no recorded failure data).
fn failures_section(b: &viz::Bundle, parser: &str) -> Element {
    // Gather (dialect display name, failures) for dialects where this parser
    // has at least one rejected statement.
    let entries: Vec<(&str, &viz::ParserFailures)> = b
        .dialects
        .iter()
        .filter_map(|d| {
            d.failures
                .iter()
                .find(|f| f.parser == parser && f.rejected_total > 0)
                .map(|f| (d.display_name.as_str(), f))
        })
        .collect();
    if entries.is_empty() {
        return rsx! {};
    }

    rsx! {
        section { class: "block",
            h2 {
                Icon { width: 17, height: 17, fill: "currentColor".to_string(), class: "h2-ico".to_string(), icon: FaTriangleExclamation }
                "Failing statements"
            }
            p { class: "fail-intro",
                "Statements this parser was expected to accept but rejected. Each dialect links to the full set (capped at 1,000) as a compressed TSV so the cases can be downloaded and addressed."
            }
            for (di , (dialect , f)) in entries.into_iter().enumerate() {
                div { class: "fail-dialect", key: "{dialect}",
                    div { class: "fail-head",
                        span { class: "fail-title",
                            strong { "{dialect}" }
                            span { class: "fail-count", "{commas(f.rejected_total)} of {commas(f.expected_total)} rejected" }
                        }
                        if let Some(path) = &f.download {
                            a {
                                class: "dl-btn",
                                href: "/{path}",
                                download: true,
                                Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaDownload }
                                "TSV"
                            }
                        }
                    }
                    for (i , html) in f.preview_html.iter().enumerate() {
                        div { class: "fail-code-wrap", key: "{i}",
                            button {
                                class: "copy-btn",
                                r#type: "button",
                                aria_label: "Copy this statement to the clipboard",
                                title: "Copy statement",
                                onclick: move |_| {
                                    document::eval(&format!(
                                        "{{ const el = document.getElementById('fail-{di}-{i}'); if (el) {{ navigator.clipboard.writeText(el.textContent); }} }}"
                                    ));
                                },
                                Icon { width: 13, height: 13, fill: "currentColor".to_string(), icon: FaCopy }
                            }
                            pre { id: "fail-{di}-{i}", class: "fail-code", dangerous_inner_html: "{html}" }
                            if let Some(reason) = f.preview_reasons.get(i).filter(|r| !r.is_empty()) {
                                div { class: "fail-reason", "{reason}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- formatting helpers ----

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

/// A comma-grouped count with a regular-plural noun: "1 dialect", "13 dialects".
fn count_noun(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{} {noun}s", commas(n))
    }
}

fn fmt_pct(v: Option<f64>) -> String {
    v.map_or_else(|| "N/A".to_string(), |x| format!("{x:.1}%"))
}

/// A fragment of blurb prose: plain text, or an inline link parsed from the
/// lightweight `[label](url)` markup used in the blurb strings.
enum Frag {
    Text(String),
    Link { label: String, url: String },
}

/// Split blurb text into [`Frag`]s, pulling out `[label](url)` links. Any `[`
/// without a complete following `](url)` is treated as literal text.
fn parse_frags(text: &str) -> Vec<Frag> {
    let mut frags = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        if let Some(close_rel) = rest[open..].find("](") {
            let close = open + close_rel;
            if let Some(end_rel) = rest[close + 2..].find(')') {
                let end = close + 2 + end_rel;
                if open > 0 {
                    frags.push(Frag::Text(rest[..open].to_string()));
                }
                frags.push(Frag::Link {
                    label: rest[open + 1..close].to_string(),
                    url: rest[close + 2..end].to_string(),
                });
                rest = &rest[end + 1..];
                continue;
            }
        }
        // No complete link here: keep text through this '[' and scan onward.
        frags.push(Frag::Text(rest[..=open].to_string()));
        rest = &rest[open + 1..];
    }
    if !rest.is_empty() {
        frags.push(Frag::Text(rest.to_string()));
    }
    frags
}

/// Render blurb text as a sequence of text nodes and inline anchors. Links open
/// in a new tab with `rel="noopener"` since they leave the site.
fn rich_text(text: &str) -> Vec<Element> {
    parse_frags(text)
        .into_iter()
        .enumerate()
        .map(|(i, frag)| match frag {
            Frag::Text(t) => rsx! {
                "{t}"
            },
            Frag::Link { label, url } => rsx! {
                a {
                    key: "{i}",
                    class: "inline-link",
                    href: "{url}",
                    target: "_blank",
                    rel: "noopener noreferrer",
                    "{label}"
                }
            },
        })
        .collect()
}

/// Editorial paragraph under a detail-page hero, with inline source links woven
/// into the prose. Renders nothing when the text is empty.
fn blurb(text: &str) -> Element {
    if text.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "about",
            p { class: "blurb", {rich_text(text).into_iter()} }
        }
    }
}

/// One labelled figure in the parser metadata block, as a compact pill: a small
/// icon, the value, and the label inline. `icon` is a decorative icon element.
/// `desc` is a full sentence shown as a tooltip and read by assistive tech.
fn meta_item(icon: Element, label: &str, value: String, desc: String) -> Element {
    rsx! {
        span { class: "meta-item", title: "{desc}", "aria-label": "{desc}",
            span { class: "meta-ico", "aria-hidden": "true", {icon} }
            span { class: "meta-val", "aria-hidden": "true", "{value}" }
            span { class: "meta-key", "aria-hidden": "true", "{label}" }
        }
    }
}

/// A metadata pill whose value flags a problem: when `ok` is false the whole
/// pill is marked red (a missing test suite, fuzzer, or benchmark suite). `desc`
/// is a full sentence shown as a tooltip and read by assistive tech.
fn meta_flag(icon: Element, label: &str, value: String, ok: bool, desc: &str) -> Element {
    let class = if ok { "meta-item" } else { "meta-item bad" };
    rsx! {
        span { class: "{class}", title: "{desc}", "aria-label": "{desc}",
            span { class: "meta-ico", "aria-hidden": "true", {icon} }
            span { class: "meta-val", "aria-hidden": "true", "{value}" }
            span { class: "meta-key", "aria-hidden": "true", "{label}" }
        }
    }
}

/// A metadata pill that links somewhere (e.g. the source repository). Same shape
/// as [`meta_item`] but rendered as an anchor opening in a new tab.
fn meta_link(icon: Element, label: &str, value: String, href: &str, desc: String) -> Element {
    rsx! {
        a {
            class: "meta-item meta-link",
            href: "{href}",
            target: "_blank",
            rel: "noopener noreferrer",
            title: "{desc}",
            "aria-label": "{desc}",
            span { class: "meta-ico", "aria-hidden": "true", {icon} }
            span { class: "meta-val", "aria-hidden": "true", "{value}" }
            span { class: "meta-key", "aria-hidden": "true", "{label}" }
        }
    }
}

/// The host's brand glyph for a repository URL: GitHub's mark where applicable,
/// else the generic git logo (Codeberg and the like have no dedicated icon).
fn repo_icon(url: &str) -> Element {
    if url.contains("github.com") {
        rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaGithub } }
    } else {
        rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaGit } }
    }
}

/// Repository and crate metadata pills for a parser, shown inside the parser
/// hero banner. Renders nothing for a parser with no recorded metadata. Figures
/// are a dated snapshot (see `metadata::SNAPSHOT`).
fn parser_meta_pills(parser: &str) -> Element {
    use crate::metadata::{parser_meta, SNAPSHOT};
    let Some(m) = parser_meta(parser) else {
        return rsx! {};
    };
    rsx! {
        div {
            class: "meta-grid",
            title: "Repository and crate figures as of {SNAPSHOT}.",
            {meta_link(repo_icon(m.repo), "repo", crate::metadata::repo_host(m.repo).to_string(), m.repo, crate::metadata::repo_description(m.repo))}
            {meta_item(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaStar } }, "stars", commas(m.stars as usize), crate::metadata::stars_description(m.stars))}
            {meta_item(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaCodeFork } }, "forks", commas(m.forks as usize), crate::metadata::forks_description(m.forks))}
            {meta_item(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaCodeCommit } }, "commits", commas(m.commits as usize), crate::metadata::commits_description(m.commits))}
            {meta_item(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaUsers } }, "contributors", commas(m.contributors as usize), crate::metadata::contributors_description(m.contributors))}
            {meta_item(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaCalendarDays } }, "since", m.since.to_string(), crate::metadata::since_description(m.since))}
            {meta_item(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaDownload } }, "downloads", m.downloads.to_string(), crate::metadata::downloads_description(m.downloads))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaBox } }, "crates.io", if m.crates_io { "yes".to_string() } else { "no".to_string() }, m.crates_io, crate::metadata::crates_io_description(m.crates_io))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaScaleBalanced } }, "license", m.license.to_string(), crate::metadata::license_ok(m.license), &crate::metadata::license_description(m.license))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaBug } }, "fuzzed", m.fuzz.label().to_string(), m.fuzz.is_ok(), m.fuzz.description())}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaVial } }, "tests", if m.tests { "yes".to_string() } else { "no".to_string() }, m.tests, crate::metadata::tests_description(m.tests))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaStopwatch } }, "benches", if m.benches { "yes".to_string() } else { "no".to_string() }, m.benches, crate::metadata::benches_description(m.benches))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaMicrochip } }, "no_std", if m.no_std { "yes".to_string() } else { "no".to_string() }, m.no_std, crate::metadata::no_std_description(m.no_std))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaCube } }, "wasm", if m.wasm { "yes".to_string() } else { "no".to_string() }, m.wasm, crate::metadata::wasm_description(m.wasm))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaRust } }, "impl", if m.pure_rust { "pure Rust".to_string() } else { "C FFI".to_string() }, m.pure_rust, crate::metadata::pure_rust_description(m.pure_rust))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaShieldHalved } }, "unsafe", if m.unsafe_note.is_empty() { "none".to_string() } else { "uses".to_string() }, m.unsafe_note.is_empty(), &crate::metadata::unsafe_description(m.unsafe_note))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaHeartPulse } }, "maintained", if crate::metadata::maintained(m.last_release) { "active".to_string() } else { "stale".to_string() }, crate::metadata::maintained(m.last_release), &crate::metadata::maintenance_description(m.last_release))}
            {meta_flag(rsx! { Icon { width: 12, height: 12, fill: "currentColor".to_string(), icon: FaFlaskVial } }, "miri/san", if m.sanitizers.is_empty() { "no".to_string() } else { m.sanitizers.to_string() }, !m.sanitizers.is_empty(), &crate::metadata::sanitizer_description(m.sanitizers))}
        }
    }
}

fn ratio_pct(n: usize, base: usize) -> String {
    if base == 0 {
        "N/A".to_string()
    } else {
        format!("{:.1}%", 100.0 * n as f64 / base as f64)
    }
}

/// Fraction of reference-valid statements the parser failed to accept (false
/// negatives, i.e. `1 - recall`). On reference dialects this excludes true
/// negatives the parser correctly rejected. On provenance dialects every
/// statement is treated as valid, so this reduces to the unaccepted fraction.
fn missed_pct(d: &DialectData, p: &ParserPerf) -> String {
    d.correctness
        .iter()
        .find(|m| m.parser == p.parser)
        .and_then(|m| m.recall_pct)
        .map_or_else(
            || ratio_pct(p.n_total.saturating_sub(p.n_accepted), p.n_total),
            |r| format!("{:.1}%", (100.0 - r).max(0.0)),
        )
}

/// Shared parser ordering for a dialect's tables: the perf order (fastest
/// median first), then any parser only present in the correctness or coverage
/// data. Keeping all three tables in this order lets the eye track one parser
/// down the page.
fn display_order(d: &DialectData) -> Vec<&str> {
    let mut order: Vec<&str> = d.perf.iter().map(|p| p.parser.as_str()).collect();
    for name in d
        .correctness
        .iter()
        .map(|m| m.parser.as_str())
        .chain(d.coverage.parsers.iter().map(String::as_str))
    {
        if !order.contains(&name) {
            order.push(name);
        }
    }
    order
}

/// The first-column row header for a sortable table.
#[derive(Clone, PartialEq)]
enum Head {
    /// Parser name: color swatch + link to the parser page.
    Parser(String),
    /// Dialect: link to the dialect page.
    Dialect { dir: String, name: String },
}

impl Head {
    /// The text used when sorting by the first column.
    fn sort_key(&self) -> &str {
        match self {
            Head::Parser(s) => s,
            Head::Dialect { name, .. } => name,
        }
    }
}

/// One value cell: the text shown plus an optional numeric sort key. A `None`
/// key always sorts to the bottom (e.g. an "N/A" cell), regardless of direction.
#[derive(Clone, PartialEq)]
struct Cell {
    text: String,
    num: Option<f64>,
}

impl Cell {
    /// Percentage cell from an optional ratio, formatted like the rest of the UI.
    fn pct(v: Option<f64>) -> Cell {
        Cell {
            text: fmt_pct(v),
            num: v,
        }
    }
    /// Nanosecond cell from an optional value (comma-grouped, "N/A" if missing).
    fn ns(v: Option<f64>) -> Cell {
        Cell {
            text: v.map_or_else(|| "N/A".to_string(), |x| commas(x as usize)),
            num: v,
        }
    }
    /// Cell with explicit text and a numeric sort key.
    fn with(text: String, num: Option<f64>) -> Cell {
        Cell { text, num }
    }
}

/// A sortable table row: a header cell plus value cells.
#[derive(Clone, PartialEq)]
struct Row {
    key: String,
    head: Head,
    cells: Vec<Cell>,
}

/// Order two rows by column `col` (0 = the header column, 1.. = `cells`),
/// honoring direction. Missing numeric values always sink to the bottom.
fn cmp_rows(a: &Row, b: &Row, col: usize, asc: bool) -> Ordering {
    let base = if col == 0 {
        a.head.sort_key().cmp(b.head.sort_key())
    } else {
        match (
            a.cells.get(col - 1).and_then(|c| c.num),
            b.cells.get(col - 1).and_then(|c| c.num),
        ) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    };
    if asc {
        base
    } else {
        base.reverse()
    }
}

/// A small inline dialect mark for a table row: the shipped full-color logo
/// where one exists, otherwise the brand-colored glyph (matching the dialect
/// cards and hero), so dialect identity carries into the per-parser tables.
fn dialect_row_mark(dir: &str) -> Element {
    let br = brand(dir);
    if let Some(logo) = logo_asset(dir) {
        rsx! {
            span { class: "row-ico chip-light", "aria-hidden": "true",
                img { class: "logo-img", src: logo, alt: "" }
            }
        }
    } else {
        rsx! {
            span {
                class: "row-ico",
                style: "background: {br.accent}; color: {br.on_accent};",
                "aria-hidden": "true",
                {dialect_glyph(dir, 13, br.on_accent)}
            }
        }
    }
}

/// Render a row's header cell.
fn render_head(head: &Head) -> Element {
    match head {
        Head::Parser(p) => rsx! {
            th { scope: "row", class: "pname",
                span { class: "dot", style: "background: {parser_hex(p)}", "aria-hidden": "true" }
                Link { to: Route::ParserView { name: slug(p) }, "{p}" }
            }
        },
        Head::Dialect { dir, name } => rsx! {
            th { scope: "row", class: "dname",
                {dialect_row_mark(dir)}
                Link { to: Route::DialectView { dir: dir.clone() }, "{name}" }
            }
        },
    }
}

/// A generic click-to-sort data table. `corner` labels the first (header)
/// column; `columns` are the value-column labels. Clicking any header toggles
/// ascending / descending on that column. `footer`, if present, is a row
/// pinned below the sorted rows (used for the coverage subtotal).
#[component]
fn SortTable(
    caption: String,
    corner: String,
    columns: Vec<String>,
    rows: Vec<Row>,
    footer: Option<(String, Vec<Cell>)>,
) -> Element {
    let mut sort = use_signal(|| None::<(usize, bool)>);
    let current = sort();

    let mut ordered = rows.clone();
    if let Some((col, asc)) = current {
        ordered.sort_by(|a, b| cmp_rows(a, b, col, asc));
    }

    let heads: Vec<(usize, String)> = std::iter::once((0usize, corner.clone()))
        .chain(columns.iter().enumerate().map(|(i, n)| (i + 1, n.clone())))
        .collect();

    rsx! {
        div { class: "scroll",
            table { class: "data",
                caption { class: "sr-only", "{caption}" }
                thead {
                    tr {
                        for (idx, name) in heads {
                            {
                                let dir = current.and_then(|(c, a)| (c == idx).then_some(a));
                                let active = dir.is_some();
                                let asc = dir.unwrap_or(true);
                                let aria = if !active { "none" } else if asc { "ascending" } else { "descending" };
                                let arrow = if !active { "\u{2195}" } else if asc { "\u{25b2}" } else { "\u{25bc}" };
                                let first = idx == 0;
                                rsx! {
                                    th { scope: "col", key: "{idx}", "aria-sort": aria,
                                        button {
                                            class: if first { "sort-btn first" } else { "sort-btn" },
                                            onclick: move |_| {
                                                let next = match sort() {
                                                    Some((c, a)) if c == idx => (idx, !a),
                                                    _ => (idx, true),
                                                };
                                                sort.set(Some(next));
                                            },
                                            "{name}"
                                            span {
                                                class: if active { "sort-ind active" } else { "sort-ind" },
                                                "aria-hidden": "true",
                                                "{arrow}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                tbody {
                    for row in ordered.iter() {
                        tr { key: "{row.key}",
                            {render_head(&row.head)}
                            for cell in row.cells.iter() {
                                td { "{cell.text}" }
                            }
                        }
                    }
                    if let Some((label, cells)) = &footer {
                        tr { class: "subtotal",
                            th { scope: "row", "{label}" }
                            for cell in cells.iter() {
                                td { "{cell.text}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Numeric counterpart of [`missed_pct`] for sorting (None when unknown).
fn missed_val(d: &DialectData, p: &ParserPerf) -> Option<f64> {
    match d
        .correctness
        .iter()
        .find(|m| m.parser == p.parser)
        .and_then(|m| m.recall_pct)
    {
        Some(r) => Some((100.0 - r).max(0.0)),
        None => {
            if p.n_total == 0 {
                None
            } else {
                Some(100.0 * p.n_total.saturating_sub(p.n_accepted) as f64 / p.n_total as f64)
            }
        }
    }
}

// ---- tables ----

fn perf_table(d: &DialectData) -> Element {
    let columns = ["median ns", "p90 ns", "missed %", "RT %"]
        .iter()
        .map(ToString::to_string)
        .collect();
    let rows = d
        .perf
        .iter()
        .map(|p| Row {
            key: p.parser.clone(),
            head: Head::Parser(p.parser.clone()),
            cells: vec![
                Cell::ns(Some(p.median)),
                Cell::ns(Some(p.p90)),
                Cell::with(missed_pct(d, p), missed_val(d, p)),
                Cell::pct(p.roundtrip_pct),
            ],
        })
        .collect();
    rsx! {
        section { class: "block",
            h2 {
                Icon { width: 17, height: 17, fill: "currentColor".to_string(), class: "h2-ico".to_string(), icon: FaTableCells }
                "Speed"
            }
            SortTable {
                caption: format!("Per-parser parse time in nanoseconds for {}", d.display_name),
                corner: "parser".to_string(),
                columns,
                rows,
                footer: None,
            }
        }
    }
}

fn correctness_table(d: &DialectData) -> Element {
    let reference = d.has_reference;
    let columns: Vec<String> = if reference {
        ["recall", "false pos", "round-trip", "fidelity"]
            .iter()
            .map(ToString::to_string)
            .collect()
    } else {
        ["accept", "round-trip"]
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    let rows = display_order(d)
        .iter()
        .filter_map(|name| d.correctness.iter().find(|m| m.parser.as_str() == *name))
        .map(|m| Row {
            key: m.parser.clone(),
            head: Head::Parser(m.parser.clone()),
            cells: if reference {
                vec![
                    Cell::pct(m.recall_pct),
                    Cell::pct(m.false_positive_pct),
                    Cell::pct(m.roundtrip_pct),
                    Cell::pct(m.fidelity_pct),
                ]
            } else {
                vec![Cell::pct(m.accept_pct), Cell::pct(m.roundtrip_pct)]
            },
        })
        .collect();
    rsx! {
        section { class: "block",
            h2 {
                Icon { width: 17, height: 17, fill: "currentColor".to_string(), class: "h2-ico".to_string(), icon: FaTableCells }
                "Correctness"
            }
            SortTable {
                caption: format!("Per-parser correctness for {}", d.display_name),
                corner: "parser".to_string(),
                columns,
                rows,
                footer: None,
            }
        }
    }
}

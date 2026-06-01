//! Dioxus -> WASM viewer for the sql_ast_benchmark results. Reads the committed
//! `assets/bench.json` snapshot and renders interactive per-dialect pages: the
//! eCDF and box charts (rendered in-browser by `viz` via plotters) plus the
//! correctness, perf, and per-file coverage tables.

use dioxus::prelude::*;
use manganis::AssetOptions;

mod brand;
mod components;
mod data;
mod descriptions;
mod logos;
mod metadata;

use components::{DialectView, Overview, ParserView, Shell};

/// The site stylesheet, emitted into the static `index.html` `<head>` at build
/// time (`with_static_head`) so it is present on first paint. Injecting it via
/// `document::Stylesheet` from a component instead links it only after wasm
/// boots and first-renders, which flashes unstyled content (FOUC).
const MAIN_CSS: Asset = asset!(
    "/assets/main.css",
    AssetOptions::css().with_static_head(true)
);

/// The site favicon: an abstract-syntax-tree mark in the accent color.
pub const FAVICON: Asset = asset!("/assets/favicon.svg");

/// Client-side routes. Variant names map to the components of the same name.
/// `Shell` wraps every page with the shared header, skip link, and footer.
#[derive(Clone, PartialEq, Routable)]
enum Route {
    #[layout(Shell)]
    #[route("/")]
    Overview {},
    #[route("/dialect/:dir")]
    DialectView { dir: String },
    #[route("/parser/:name")]
    ParserView { name: String },
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    // MAIN_CSS is force-referenced so the linker keeps the asset; the actual
    // <link> is emitted into the static <head> via with_static_head above.
    let _ = MAIN_CSS;
    rsx! {
        // SVG icon for modern browsers, plus the root .ico (copied to the site
        // root by the Pages workflow) for the /favicon.ico browsers auto-probe.
        document::Link { rel: "icon", r#type: "image/svg+xml", href: FAVICON }
        document::Link { rel: "icon", r#type: "image/x-icon", href: "/favicon.ico" }
        Router::<Route> {}
    }
}

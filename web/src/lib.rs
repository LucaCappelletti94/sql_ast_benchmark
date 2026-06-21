//! Dioxus -> WASM viewer for the sql_ast_benchmark results. Reads the committed
//! `assets/bench.json.zst` snapshot and renders interactive per-dialect pages.
//! Exposed as a library so the badge generator can reuse the scoring and
//! metadata; `main.rs` is the thin wasm entry point.

use dioxus::prelude::*;
use manganis::AssetOptions;

pub mod badges;
pub mod brand;
pub mod cadence;
pub mod components;
pub mod data;
pub mod descriptions;
pub mod dialect_meta;
pub mod logos;
pub mod metadata;
pub mod score;

use components::{DialectView, Overview, ParserView, Shell};

/// The site stylesheet, emitted into the static `index.html` `<head>` at build
/// time so it is present on first paint (avoids a flash of unstyled content).
const MAIN_CSS: Asset = asset!(
    "/assets/main.css",
    AssetOptions::css().with_static_head(true)
);

/// The site favicon: an abstract-syntax-tree mark in the accent color.
pub const FAVICON: Asset = asset!("/assets/favicon.svg");

/// Client-side routes. `Shell` wraps every page with the shared chrome.
#[derive(Clone, PartialEq, Routable)]
pub enum Route {
    #[layout(Shell)]
    #[route("/")]
    Overview {},
    #[route("/dialect/:dir")]
    DialectView { dir: String },
    #[route("/parser/:name")]
    ParserView { name: String },
}

/// Launch the viewer (called from the wasm entry point).
pub fn launch() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let _ = MAIN_CSS;
    rsx! {
        document::Link { rel: "icon", r#type: "image/svg+xml", href: FAVICON }
        document::Link { rel: "icon", r#type: "image/x-icon", href: "/favicon.ico" }
        Router::<Route> {}
    }
}

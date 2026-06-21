//! Writes the static per-parser README badges into `web/static/badges/`, one SVG
//! per parser per variant, from the same scoring the website uses. Run after
//! `sqlbench export` refreshes the snapshot the `web` crate embeds.

use sql_ast_benchmark_web::badges;
use std::{fs, path::Path};

fn main() -> std::io::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/static/badges");
    fs::create_dir_all(&dir)?;
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "svg") {
            fs::remove_file(path)?;
        }
    }

    let mut written = 0;
    for (_parser, variants) in badges::all() {
        for v in variants {
            fs::write(dir.join(&v.file), v.svg)?;
            written += 1;
        }
    }
    println!("wrote {written} badge SVGs to {}", dir.display());
    Ok(())
}

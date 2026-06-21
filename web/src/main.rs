//! Thin wasm entry point. The viewer lives in the library crate so the badge
//! generator can reuse its scoring and metadata.

fn main() {
    sql_ast_benchmark_web::launch();
}

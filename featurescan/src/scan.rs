//! AST-level counting of panic-inducing constructs and unsafe usage in a crate's
//! library source, using `syn`.
//!
//! Limitations, stated plainly because the numbers are a code-smell proxy and not
//! a crash proof:
//! - Only library `src/` is scanned. `tests/`, `benches/`, `examples/`, inline
//!   `#[cfg(test)]` / `#[test]` items, and well-known test-helper files (e.g. a
//!   `test_utils.rs` exposed as a non-gated `pub mod` for integration tests) are
//!   skipped, so test-only unwraps do not count.
//! - Macro bodies are opaque token streams to `syn`, so an `unwrap()` written
//!   inside a `format!`/`vec!` argument is not counted. This undercounts slightly.
//! - Index expressions count every `a[i]`, including provably-safe constant indices.

use std::path::Path;

use quote::ToTokens;
use syn::visit::{self, Visit};
use viz::FeatureCounts;
use walkdir::WalkDir;

/// Scan every `.rs` file under `src_dir`, accumulating construct counts into the
/// shared [`FeatureCounts`] schema.
pub fn scan_src(src_dir: &Path) -> FeatureCounts {
    let mut counts = FeatureCounts::default();
    for entry in WalkDir::new(src_dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        if is_test_path(path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        counts.files += 1;
        counts.loc += text.lines().count();
        match syn::parse_file(&text) {
            Ok(file) => {
                let mut scanner = Scanner {
                    counts: &mut counts,
                };
                scanner.visit_file(&file);
            }
            Err(_) => counts.parse_failures += 1,
        }
    }
    counts.code_loc = counts.loc.saturating_sub(counts.test_lines);
    counts
}

/// Number of source lines a node spans (requires proc-macro2 span-locations).
fn span_lines(node: &impl syn::spanned::Spanned) -> usize {
    let span = node.span();
    span.end().line.saturating_sub(span.start().line) + 1
}

/// True if a `src/` file is a test fixture or test-helper rather than library code:
/// it sits under a `tests`/`benches`/`examples` directory, or its name is a known
/// test-helper file. These often are not `#[cfg(test)]`-gated (a `pub mod test_utils`
/// shared with integration tests), so the AST cfg filter alone would not skip them.
fn is_test_path(path: &Path) -> bool {
    let in_test_dir = path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s == "tests" || s == "test" || s == "benches" || s == "examples"
    });
    let test_stem = matches!(
        path.file_stem().and_then(|s| s.to_str()),
        Some("tests" | "test_utils" | "test_util" | "test_helpers" | "test_helper" | "testing")
    );
    in_test_dir || test_stem
}

struct Scanner<'a> {
    counts: &'a mut FeatureCounts,
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if has_cfg_test(&node.attrs) {
            self.counts.test_lines += span_lines(node);
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if is_test_item(&node.attrs) {
            self.counts.test_lines += span_lines(node);
            return;
        }
        if node.sig.unsafety.is_some() {
            self.counts.unsafe_fns += 1;
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if is_test_item(&node.attrs) {
            self.counts.test_lines += span_lines(node);
            return;
        }
        if node.sig.unsafety.is_some() {
            self.counts.unsafe_fns += 1;
        }
        visit::visit_impl_item_fn(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if is_test_item(&node.attrs) {
            self.counts.test_lines += span_lines(node);
            return;
        }
        if node.unsafety.is_some() {
            self.counts.unsafe_impls += 1;
        }
        visit::visit_item_impl(self, node);
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.counts.unsafe_blocks += 1;
        visit::visit_expr_unsafe(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if let Some(seg) = node.path.segments.last() {
            match seg.ident.to_string().as_str() {
                "panic" => self.counts.panic += 1,
                "unreachable" => self.counts.unreachable += 1,
                "unimplemented" => self.counts.unimplemented += 1,
                "todo" => self.counts.todo += 1,
                "assert" | "assert_eq" | "assert_ne" => self.counts.assert += 1,
                _ => {}
            }
        }
        visit::visit_macro(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        // Disambiguate the std panicking methods from user-defined combinators of
        // the same name (e.g. a parser's `self.expect(Token)` that returns Result).
        // `Option/Result::unwrap` takes no args, and `expect` takes a single string
        // message. syn has no type info, so the argument shape is the best signal.
        match node.method.to_string().as_str() {
            "unwrap" | "unwrap_err" if node.args.is_empty() => self.counts.unwrap += 1,
            "expect" | "expect_err" if is_message_call(&node.args) => self.counts.expect += 1,
            "unwrap_unchecked" | "unwrap_err_unchecked" if node.args.is_empty() => {
                self.counts.unwrap_unchecked += 1;
            }
            _ => {}
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_index(&mut self, node: &'ast syn::ExprIndex) {
        self.counts.index += 1;
        visit::visit_expr_index(self, node);
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if node.path().is_ident("derive") {
            let _ = node.parse_nested_meta(|meta| {
                if meta
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "Serialize")
                {
                    self.counts.serde_derive = true;
                }
                Ok(())
            });
        }
        visit::visit_attribute(self, node);
    }
}

/// True if a method-call argument list looks like an `Option/Result::expect`
/// message: exactly one argument that is a string-ish expression. This excludes
/// user-defined `expect(Token)` combinators while accepting `.expect("msg")`,
/// `.expect(&format!(...))`, and similar.
fn is_message_call(args: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>) -> bool {
    args.len() == 1 && is_message_expr(&args[0])
}

fn is_message_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Lit(lit) => matches!(lit.lit, syn::Lit::Str(_)),
        syn::Expr::Reference(r) => is_message_expr(&r.expr),
        syn::Expr::Macro(m) => m
            .mac
            .path
            .segments
            .last()
            .is_some_and(|s| matches!(s.ident.to_string().as_str(), "format" | "concat")),
        _ => false,
    }
}

/// True if these attributes mark the item as test-only (`#[test]` or `#[cfg(test)]`,
/// including nested forms like `#[cfg(all(test, ...))]`).
fn is_test_item(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test")) || has_cfg_test(attrs)
}

/// True if any attribute is a `cfg` whose predicate mentions `test`.
fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg") && {
            // Stringify so nested `all(test, ...)` / `any(test, ...)` are caught.
            let tokens = a.meta.to_token_stream().to_string();
            tokens.contains("test")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_constructs_and_skips_tests() {
        let src = r#"
            fn real() {
                let a = foo().unwrap();
                let b = bar().expect("msg");
                let c = arr[3];
                // A user-defined combinator named like the std methods: must NOT count.
                self.expect(TokenType::RParen);
                let d = self.unwrap(node, extra);
                if x { panic!("no") }
                unreachable!();
                todo!();
                unsafe { do_thing(); }
            }

            #[derive(Clone, Serialize)]
            struct Ast;

            #[cfg(test)]
            mod tests {
                fn helper() {
                    let _ = thing().unwrap();
                    let _ = other().expect("nope");
                }
            }

            #[test]
            fn a_test() {
                let _ = z().unwrap();
            }
        "#;
        let file = syn::parse_file(src).expect("fixture parses");
        let mut counts = FeatureCounts::default();
        let mut scanner = Scanner {
            counts: &mut counts,
        };
        scanner.visit_file(&file);

        // Only the unwraps/expects in `real()` count, not the test module or #[test] fn.
        assert_eq!(counts.unwrap, 1, "unwrap");
        assert_eq!(counts.expect, 1, "expect");
        assert_eq!(counts.index, 1, "index");
        assert_eq!(counts.panic, 1, "panic");
        assert_eq!(counts.unreachable, 1, "unreachable");
        assert_eq!(counts.todo, 1, "todo");
        assert_eq!(counts.unsafe_blocks, 1, "unsafe block");
        assert!(counts.serde_derive, "serde derive detected");
    }
}

#![allow(
    clippy::doc_markdown,
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::option_if_let_else,
    clippy::single_match_else,
    clippy::unnested_or_patterns,
    clippy::unnecessary_map_or
)]

/// Scrapes SQL statements from the sqlparser-rs test suite and writes them to
/// text files (one SQL per line) for use in the correctness benchmark.
///
/// Targets:
///   sqlparser_test_postgres.txt   - from sqlparser_postgres.rs
///   sqlparser_test_common.txt     - from sqlparser_common.rs
///   sqlparser_test_regression.txt - from sqlparser_regression.rs
///
/// Extraction patterns (all are "expected to succeed" cases):
///   .verified_stmt("SQL")
///   .verified_query("SQL")
///   .verified_only_select("SQL")
///   .one_statement_parses_to(sql_var, "CANONICAL")
///   .one_statement_parses_to("INPUT", "CANONICAL")  -> extracts both
///   .statements_parse_to(sql_var, "CANONICAL")
///   let <name> = "SQL";  (variable assignments)
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Finds the sqlparser-rs test directory inside ~/.cargo/git/checkouts/
fn find_sqlparser_test_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let checkouts = PathBuf::from(&home).join(".cargo/git/checkouts");

    let entries = fs::read_dir(&checkouts).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("sqlparser-rs-") {
            continue;
        }
        let Ok(subdirs) = fs::read_dir(entry.path()) else {
            continue;
        };
        for sub in subdirs.flatten() {
            let tests_path = sub.path().join("tests");
            if tests_path.is_dir() {
                return Some(tests_path);
            }
        }
    }
    None
}

/// Processes a raw string literal value (between the quotes) by applying
/// Rust escape sequences and normalizing whitespace to a single line.
///
/// Rust escape sequences handled:
///   `\<newline>` — line continuation (backslash + newline + following spaces removed)
///   `\"` → `"`
///   `\\` → `\`
///   `\n`, `\r`, `\t` → normalized to single space
///   other `\X` → kept as-is
///
/// Actual newlines in the raw content (multiline string literals) are also
/// normalized to spaces, then multiple spaces are collapsed.
fn process_rust_string(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                // Line continuation: `\` + newline → drop both + following whitespace
                Some('\n') => {
                    while chars.peek().map_or(false, |&ch| ch == ' ' || ch == '\t') {
                        chars.next();
                    }
                }
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                // \n, \r, \t in string literals → normalize to space
                Some('n') | Some('r') | Some('t') => result.push(' '),
                // \0 → keep as-is (not useful for SQL but correct)
                Some('0') => {}
                // anything else → keep both chars
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => {}
            },
            // Actual newlines/tabs in multiline string literals → space
            '\n' | '\r' | '\t' => result.push(' '),
            other => result.push(other),
        }
    }

    // Collapse multiple spaces into one and trim
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The core string literal pattern: matches everything between unescaped quotes,
/// including escaped characters (`\\` or `\"`) and newlines (multiline literals).
/// Note: `[^"\\]` in the regex crate matches newlines by default (unlike `.`).
const STR_LIT: &str = r#"(?:[^"\\]|\\[\s\S])*"#;

/// Extracts candidate SQL strings from a sqlparser-rs test file's content.
///
/// Returns raw (unprocessed) captured strings from inside the quote pairs.
/// Callers should run each through `process_rust_string`.
fn extract_raw_strings(content: &str) -> Vec<String> {
    // Build patterns using the shared string-literal inner pattern.

    // Pattern A: single-arg verified helpers
    // .verified_stmt("SQL"), .verified_query("SQL"), .verified_only_select("SQL")
    let single_fn = format!(
        r#"\.(?:verified_stmt|verified_query|verified_only_select)\s*\(\s*"({STR_LIT})"\s*[,)]"#
    );

    // Pattern B: .parse_sql_statements("SQL") — captures both success and error
    // test cases; the sqlparser-rs pre-filter in correctness.rs automatically
    // discards any SQL that fails to parse.
    let parse_stmts_fn = format!(r#"\.parse_sql_statements\s*\(\s*"({STR_LIT})"\s*[,)]"#);

    // Pattern C: two-arg with a *string* first arg — extract both.
    // .one_statement_parses_to("INPUT", "CANONICAL")
    let two_str_fn = format!(
        r#"\.(?:one_statement_parses_to|statements_parse_to)\s*\(\s*"({STR_LIT})"\s*,\s*"({STR_LIT})""#
    );

    // Pattern D: two-arg with a *variable* first arg — extract canonical only.
    // .one_statement_parses_to(sql, "CANONICAL")
    let two_var_fn = format!(
        r#"\.(?:one_statement_parses_to|statements_parse_to)\s*\(\s*[a-zA-Z_][a-zA-Z0-9_]*\s*,\s*"({STR_LIT})""#
    );

    // Pattern E: variable assignments: `let <name> = "SQL";`
    let var_assign = format!(r#"let\s+[a-zA-Z_][a-zA-Z0-9_]*\s*=\s*"({STR_LIT})"\s*;"#);

    let single_re = Regex::new(&single_fn).expect("single fn regex");
    let parse_stmts_re = Regex::new(&parse_stmts_fn).expect("parse_stmts regex");
    let two_str_re = Regex::new(&two_str_fn).expect("two-str fn regex");
    let two_var_re = Regex::new(&two_var_fn).expect("two-var fn regex");
    let var_re = Regex::new(&var_assign).expect("var assign regex");

    let mut raw: Vec<String> = Vec::new();

    for cap in single_re.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            raw.push(m.as_str().to_owned());
        }
    }

    for cap in parse_stmts_re.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            raw.push(m.as_str().to_owned());
        }
    }

    for cap in two_str_re.captures_iter(content) {
        // Group 1 = input SQL, group 2 = canonical
        for idx in [1usize, 2] {
            if let Some(m) = cap.get(idx) {
                raw.push(m.as_str().to_owned());
            }
        }
    }

    for cap in two_var_re.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            raw.push(m.as_str().to_owned());
        }
    }

    for cap in var_re.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            raw.push(m.as_str().to_owned());
        }
    }

    raw
}

/// Returns `true` if the processed SQL string looks like it could be a valid
/// SQL statement (basic heuristic: non-empty, minimum length, starts with a
/// keyword-ish character).
fn looks_like_sql(sql: &str) -> bool {
    let s = sql.trim();
    if s.len() < 6 {
        return false;
    }
    // Must start with a letter (SQL keywords) not a symbol or digit
    s.starts_with(|c: char| c.is_ascii_alphabetic())
}

fn process_file(test_dir: &Path, filename: &str, output: &str) {
    let input_path = test_dir.join(filename);
    if !input_path.exists() {
        eprintln!("  Skipping {filename}: file not found");
        return;
    }

    let content = match fs::read_to_string(&input_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  Failed to read {filename}: {e}");
            return;
        }
    };

    let raw_strings = extract_raw_strings(&content);

    let mut seen: HashSet<String> = HashSet::new();
    let mut sqls: Vec<String> = Vec::new();

    for raw in raw_strings {
        let sql = process_rust_string(&raw);
        if looks_like_sql(&sql) && seen.insert(sql.clone()) {
            sqls.push(sql);
        }
    }

    let out_content = sqls.join("\n") + "\n";
    match fs::write(output, &out_content) {
        Ok(()) => println!("  {output}: {} unique SQL statements", sqls.len()),
        Err(e) => eprintln!("  Failed to write {output}: {e}"),
    }
}

/// Strips SQL line comments (`-- ...`) and normalizes a multi-line SQL string
/// to a single line with collapsed whitespace.
fn normalize_sql_file(content: &str) -> String {
    let no_comments: String = content
        .lines()
        .map(|line| {
            if let Some(pos) = line.find("--") {
                &line[..pos]
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    no_comments.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reads all `.sql` files from a directory and returns their normalized content
/// as individual SQL statements (one per file).
fn read_sql_dir(dir: &Path, seen: &mut HashSet<String>) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut sqls = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sql") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        // Strip trailing semicolon so parsers that don't accept it still pass
        let sql = normalize_sql_file(&content);
        let sql = sql.trim_end_matches(';').trim().to_owned();
        if looks_like_sql(&sql) && seen.insert(sql.clone()) {
            sqls.push(sql);
        }
    }
    sqls
}

/// Like `process_file` but also incorporates `.sql` files from a sibling dir.
fn process_regression(test_dir: &Path, output: &str) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut sqls: Vec<String> = Vec::new();

    // Read the regression Rust file (if present — it mostly uses macros though)
    let rs_path = test_dir.join("sqlparser_regression.rs");
    if rs_path.exists() {
        if let Ok(content) = fs::read_to_string(&rs_path) {
            for raw in extract_raw_strings(&content) {
                let sql = process_rust_string(&raw);
                if looks_like_sql(&sql) && seen.insert(sql.clone()) {
                    sqls.push(sql);
                }
            }
        }
    }

    // Read TPC-H .sql files from tests/queries/tpch/
    let tpch_dir = test_dir.join("queries").join("tpch");
    sqls.extend(read_sql_dir(&tpch_dir, &mut seen));

    // Read any other .sql files directly in tests/queries/
    let queries_dir = test_dir.join("queries");
    sqls.extend(read_sql_dir(&queries_dir, &mut seen));

    let out_content = sqls.join("\n") + "\n";
    match fs::write(output, &out_content) {
        Ok(()) => println!("  {output}: {} unique SQL statements", sqls.len()),
        Err(e) => eprintln!("  Failed to write {output}: {e}"),
    }
}

fn main() {
    let test_dir = match find_sqlparser_test_dir() {
        Some(d) => d,
        None => {
            eprintln!(
                "ERROR: Could not find sqlparser-rs test directory.\n\
                 Make sure sqlparser-rs is listed as a git dependency in Cargo.toml \n\
                 and has been fetched (run `cargo build` first)."
            );
            std::process::exit(1);
        }
    };

    println!("sqlparser-rs tests: {}", test_dir.display());
    println!("Extracting SQL statements...\n");

    let targets: &[(&str, &str)] = &[
        ("sqlparser_postgres.rs", "sqlparser_test_postgres.txt"),
        ("sqlparser_common.rs", "sqlparser_test_common.txt"),
    ];

    for (input, output) in targets {
        process_file(&test_dir, input, output);
    }

    // Regression: .rs macros + TPC-H .sql files
    process_regression(&test_dir, "sqlparser_test_regression.txt");

    println!("\nDone. Run `cargo run --bin correctness` to check parser correctness.");
}

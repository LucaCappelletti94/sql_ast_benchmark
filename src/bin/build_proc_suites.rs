//! Rebuild the Spark SQL and Oracle corpus files from their original sources,
//! keeping compound statements (`BEGIN ... END`, PL/SQL blocks) intact. The
//! original extractor split on every `;`, shredding Spark SQL scripting blocks
//! and Oracle PL/SQL blocks into invalid fragments (issue #22, provenance side).
//!
//! Spark source: apache/spark `sql/core/src/test/resources/sql-tests/inputs`.
//! Spark's own harness wraps any statement that contains inner `;` (the scripting
//! `BEGIN ... END` blocks) in `--QUERY-DELIMITER-START` / `--QUERY-DELIMITER-END`
//! markers, so we honor those: text between a marker pair is one statement,
//! everything else splits on `;`.
//!
//! Oracle source: oracle-samples/db-sample-schemas. These are SQL*Plus scripts:
//! a PL/SQL block (`DECLARE`/`BEGIN`/`CREATE ... PROCEDURE|FUNCTION|PACKAGE|
//! TRIGGER|TYPE`) runs until a line containing only `/`; every other statement
//! ends at `;`.
//!
//!   cargo run --release --bin build_proc_suites -- <spark inputs dir> <oracle schemas dir>
//!
//! Then repack `datasets.tar.zst` (Spark and Oracle are provenance, no oracle).

#![allow(
    clippy::doc_markdown,
    clippy::too_many_lines,
    clippy::items_after_statements
)]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Collapse a raw statement to one trimmed line (drops comments already removed).
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Copy a quoted literal verbatim from `chars[i..]` into `buf`, returning the
/// index just past the closing quote. Handles `'`, `"`, backtick (doubling
/// escape) and `[` (closed by `]`, no escape).
fn copy_quote(chars: &[char], mut i: usize, buf: &mut String) -> usize {
    let open = chars[i];
    let close = if open == '[' { ']' } else { open };
    buf.push(open);
    i += 1;
    while i < chars.len() {
        let d = chars[i];
        if d == close {
            if close != ']' && chars.get(i + 1) == Some(&close) {
                buf.push(d);
                buf.push(d);
                i += 2;
                continue;
            }
            buf.push(d);
            return i + 1;
        }
        buf.push(d);
        i += 1;
    }
    i
}

/// Split Spark golden-test SQL into statements, honoring `--QUERY-DELIMITER`
/// regions (one statement each) and otherwise splitting on top-level `;`. Lines
/// that are pure directive comments (`--CONFIG`, `--SET`, `--IMPORT`, ...) are
/// dropped; trailing `--` and `/* */` comments are stripped.
fn split_spark(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut region = false;

    for raw_line in input.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("--QUERY-DELIMITER-START") {
            region = true;
            continue;
        }
        if trimmed.starts_with("--QUERY-DELIMITER-END") {
            let s = normalize(&buf);
            if !s.is_empty() {
                out.push(s);
            }
            buf.clear();
            region = false;
            continue;
        }
        if region {
            // Whole region is one statement; keep code, drop full-line comments.
            if !trimmed.starts_with("--") {
                strip_line_into(raw_line, &mut buf, &mut Vec::new(), true);
                buf.push(' ');
            }
            continue;
        }
        if trimmed.starts_with("--") {
            continue; // directive / comment line
        }
        // Normal line: split on `;`, stripping inline comments and quotes.
        strip_line_into(raw_line, &mut buf, &mut out, false);
        buf.push(' ');
    }
    let s = normalize(&buf);
    if !s.is_empty() {
        out.push(s);
    }
    out
}

/// Append `line` to `buf`, stripping comments and copying quotes verbatim. When
/// `region_only` is false, a top-level `;` flushes `buf` (normalized) into `out`.
fn strip_line_into(line: &str, buf: &mut String, out: &mut Vec<String>, region_only: bool) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            break; // rest of line is a comment
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            continue;
        }
        if matches!(c, '\'' | '"' | '`' | '[') {
            i = copy_quote(&chars, i, buf);
            continue;
        }
        if c == ';' && !region_only {
            let s = normalize(buf);
            if !s.is_empty() {
                out.push(s);
            }
            buf.clear();
            i += 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
}

/// Split a comment-free string on top-level `;`, respecting quoted literals.
fn split_semicolons(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '\'' | '"' | '`' | '[') {
            i = copy_quote(&chars, i, &mut buf);
            continue;
        }
        if c == ';' {
            out.push(std::mem::take(&mut buf));
            i += 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    out.push(buf);
    out
}

/// Harvest the standalone DML statements from inside a PL/SQL block, so the bulk
/// `INSERT`/`UPDATE`/... that the block wraps remain individual corpus entries. A
/// leading `BEGIN` glued to the first inner statement is stripped. Non-DML pieces
/// (declarations, control flow, BEGIN/END) are dropped.
fn harvest_dml(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    for piece in split_semicolons(block) {
        let mut p = normalize(&piece);
        if let Some(rest) = p
            .strip_prefix("BEGIN ")
            .or_else(|| p.strip_prefix("begin "))
        {
            p = rest.trim().to_string();
        }
        let first = p
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if matches!(
            first.as_str(),
            "INSERT" | "UPDATE" | "DELETE" | "SELECT" | "MERGE" | "WITH"
        ) {
            out.push(p);
        }
    }
    out
}

/// Split Oracle SQL*Plus script text into `(normal, special)`: normal per-statement
/// corpus entries, and special whole PL/SQL anonymous blocks (kept once, isolated
/// from the per-statement metrics). A `/` line ends a block; `;` ends other
/// statements. Anonymous `DECLARE`/`BEGIN` blocks go to `special`, and their inner
/// DML is also harvested into `normal`; `CREATE ... PROCEDURE/...` blocks are kept
/// whole in `normal` (real DDL statements).
fn split_oracle(input: &str) -> (Vec<String>, Vec<String>) {
    let mut normal = Vec::new();
    let mut special = Vec::new();
    let mut buf = String::new();
    let mut in_block = false;
    let mut anon = false;
    let mut started = false;

    for raw_line in input.lines() {
        let trimmed = raw_line.trim();
        // SQL*Plus block terminator: end the current PL/SQL block.
        if trimmed == "/" {
            let s = normalize(&buf);
            if !s.is_empty() {
                if anon {
                    special.push(s);
                    normal.extend(harvest_dml(&buf));
                } else {
                    normal.push(s);
                }
            }
            buf.clear();
            in_block = false;
            anon = false;
            started = false;
            continue;
        }
        // Skip pure comment lines and SQL*Plus client directives (REM, PROMPT,
        // SET, ACCEPT, etc.) when no statement is in progress. These are not SQL
        // and, left in the buffer, would also set `started` and mask a following
        // `BEGIN` block opener (the ACCEPT ... HIDE / BEGIN IF ... pattern).
        if buf.trim().is_empty() {
            let up = trimmed.to_ascii_uppercase();
            // Leading SQL*Plus command words (skip the whole line when one starts it).
            const DIRECTIVES: &[&str] = &[
                "PROMPT",
                "SET ",
                "DEFINE",
                "UNDEFINE",
                "SPOOL",
                "WHENEVER",
                "CONNECT",
                "ALTER SESSION",
                "COLUMN ",
                "ACCEPT ",
                "PAUSE",
                "EXEC ",
                "EXECUTE ",
                "VARIABLE ",
                "VAR ",
                "PRINT ",
                "SHOW ",
                "BREAK",
                "COMPUTE ",
                "TTITLE",
                "BTITLE",
                "STORE ",
                "SAVE ",
                "HOST",
                "CLEAR ",
                "TIMING",
                "START ",
                "ACCEPT",
            ];
            if trimmed.is_empty()
                || trimmed.starts_with("--")
                || trimmed.starts_with('@')
                || up.starts_with("REM ")
                || up == "REM"
                || DIRECTIVES.iter().any(|d| up.starts_with(d))
            {
                continue;
            }
        }

        let chars: Vec<char> = raw_line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '-' && chars.get(i + 1) == Some(&'-') {
                break;
            }
            if c == '/' && chars.get(i + 1) == Some(&'*') {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
                continue;
            }
            if matches!(c, '\'' | '"' | '`' | '[') {
                i = copy_quote(&chars, i, &mut buf);
                started = true;
                continue;
            }
            if (c.is_alphanumeric() || c == '_') && !started && c.is_alphabetic() {
                // Detect the leading keyword to decide block vs simple.
                let mut j = i;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let w: String = chars[i..j].iter().collect::<String>().to_ascii_uppercase();
                if w == "DECLARE" || w == "BEGIN" {
                    in_block = true;
                    anon = true;
                }
                started = true;
                // fall through to copy chars normally below
            }
            // Once inside a CREATE statement, promote to block on a body keyword.
            if c.is_alphabetic() {
                let mut j = i;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let w: String = chars[i..j].iter().collect::<String>().to_ascii_uppercase();
                if matches!(
                    w.as_str(),
                    "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TRIGGER" | "TYPE"
                ) && buf.to_ascii_uppercase().trim_start().starts_with("CREATE")
                {
                    in_block = true;
                }
                buf.push_str(&chars[i..j].iter().collect::<String>());
                i = j;
                continue;
            }
            if c == ';' && !in_block {
                let s = normalize(&buf);
                if !s.is_empty() {
                    normal.push(s);
                }
                buf.clear();
                started = false;
                i += 1;
                continue;
            }
            buf.push(c);
            i += 1;
        }
        buf.push(' ');
    }
    let s = normalize(&buf);
    if !s.is_empty() {
        if anon {
            special.push(s);
            normal.extend(harvest_dml(&buf));
        } else {
            normal.push(s);
        }
    }
    (normal, special)
}

fn sql_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "sql") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Load the lines of `datasets/<dialect>/<file>` into `seen` (for cross-file dedup).
fn seed_seen(seen: &mut HashSet<String>, rel: &str) {
    if let Ok(c) = fs::read_to_string(Path::new("datasets").join(rel)) {
        for l in c.lines() {
            if !l.trim().is_empty() {
                seen.insert(l.trim().to_string());
            }
        }
    }
}

fn build_spark(src: &Path) {
    let mut seen = HashSet::new();
    seed_seen(&mut seen, "spark_sql/clickbench_spark.txt");
    seed_seen(&mut seen, "spark_sql/databricks_perf.txt");
    let mut kept = Vec::new();
    let mut total = 0usize;
    for f in sql_files(src) {
        for s in split_spark(&fs::read_to_string(&f).unwrap_or_default()) {
            total += 1;
            if seen.insert(s.clone()) {
                kept.push(s);
            }
        }
    }
    fs::write(
        "datasets/spark_sql/spark_sql_tst.txt",
        format!("{}\n", kept.join("\n")),
    )
    .expect("write spark corpus");
    println!(
        "spark_sql: {total} parsed, {} kept. wrote datasets/spark_sql/spark_sql_tst.txt",
        kept.len()
    );
}

fn build_oracle(src: &Path) {
    let mut seen = HashSet::new();
    seed_seen(&mut seen, "oracle/oracle_examples.txt");
    let mut normal_kept = Vec::new();
    let mut special_seen = HashSet::new();
    let mut special_kept = Vec::new();
    let (mut n_total, mut s_total) = (0usize, 0usize);
    for f in sql_files(src) {
        let (normal, special) = split_oracle(&fs::read_to_string(&f).unwrap_or_default());
        for s in normal {
            n_total += 1;
            if seen.insert(s.clone()) {
                normal_kept.push(s);
            }
        }
        for s in special {
            s_total += 1;
            if special_seen.insert(s.clone()) {
                special_kept.push(s);
            }
        }
    }
    fs::write(
        "datasets/oracle/oracle_schemas.txt",
        format!("{}\n", normal_kept.join("\n")),
    )
    .expect("write oracle corpus");
    // Special PL/SQL blocks live outside any dialect directory, so the
    // per-statement benchmark never loads them (they would be huge outliers); they
    // are kept once as whole-block test cases.
    fs::create_dir_all("datasets/special").expect("create datasets/special");
    fs::write(
        "datasets/special/oracle_plsql_blocks.txt",
        format!("{}\n", special_kept.join("\n")),
    )
    .expect("write oracle blocks");
    println!(
        "oracle: {n_total} normal parsed, {} kept; {s_total} blocks, {} special kept (datasets/special/oracle_plsql_blocks.txt)",
        normal_kept.len(),
        special_kept.len(),
    );
}

fn main() {
    if let Some(s) = std::env::args().nth(1) {
        build_spark(Path::new(&s));
    }
    if let Some(o) = std::env::args().nth(2) {
        build_oracle(Path::new(&o));
    }
}

#[cfg(test)]
mod tests {
    use super::{split_oracle, split_spark};

    #[test]
    fn spark_region_is_one_statement() {
        let sql = "SELECT 1;\n--QUERY-DELIMITER-START\nBEGIN\n  DECLARE x INT;\n  SET x = 1;\nEND;\n--QUERY-DELIMITER-END\nSELECT 2;";
        assert_eq!(
            split_spark(sql),
            vec![
                "SELECT 1".to_string(),
                "BEGIN DECLARE x INT; SET x = 1; END;".to_string(),
                "SELECT 2".to_string(),
            ]
        );
    }

    #[test]
    fn spark_strips_directives_and_comments() {
        let sql = "--CONFIG dim\n--SET spark.x=1\nSELECT 1 -- trailing\n;\nSET spark.y = 2;";
        assert_eq!(
            split_spark(sql),
            vec!["SELECT 1".to_string(), "SET spark.y = 2".to_string()]
        );
    }

    #[test]
    fn oracle_anon_block_is_special_and_inner_dml_harvested() {
        // The whole anonymous block is kept once as a special entry, and its inner
        // INSERTs also become individual normal corpus statements (in order).
        let sql = "INSERT INTO t VALUES (1);\nBEGIN\n  INSERT INTO t VALUES (2);\n  INSERT INTO t VALUES (3);\nEND;\n/\nINSERT INTO t VALUES (4);";
        let (normal, special) = split_oracle(sql);
        assert_eq!(
            normal,
            vec![
                "INSERT INTO t VALUES (1)".to_string(),
                "INSERT INTO t VALUES (2)".to_string(),
                "INSERT INTO t VALUES (3)".to_string(),
                "INSERT INTO t VALUES (4)".to_string(),
            ]
        );
        assert_eq!(
            special,
            vec!["BEGIN INSERT INTO t VALUES (2); INSERT INTO t VALUES (3); END;".to_string()]
        );
    }

    #[test]
    fn oracle_declare_block_keeps_inner_semicolons() {
        // Declarations and assignments are not DML, so only the INSERT is harvested.
        let sql = "DECLARE v NUMBER;\nBEGIN\n  v := 1;\n  INSERT INTO t VALUES (v);\nEND;\n/";
        let (normal, special) = split_oracle(sql);
        assert_eq!(normal, vec!["INSERT INTO t VALUES (v)".to_string()]);
        assert_eq!(
            special,
            vec!["DECLARE v NUMBER; BEGIN v := 1; INSERT INTO t VALUES (v); END;".to_string()]
        );
    }

    #[test]
    fn oracle_plain_statements_split_on_semicolon() {
        let sql = "CREATE TABLE t (a NUMBER);\nINSERT INTO t VALUES (1);";
        let (normal, special) = split_oracle(sql);
        assert_eq!(
            normal,
            vec![
                "CREATE TABLE t (a NUMBER)".to_string(),
                "INSERT INTO t VALUES (1)".to_string(),
            ]
        );
        assert!(special.is_empty());
    }

    #[test]
    fn oracle_create_procedure_block_stays_whole_in_normal() {
        // A CREATE PROCEDURE block is real DDL: kept whole, in normal, not special.
        let sql = "CREATE PROCEDURE p IS\nBEGIN\n  INSERT INTO t VALUES (1);\nEND;\n/";
        let (normal, special) = split_oracle(sql);
        assert_eq!(
            normal,
            vec!["CREATE PROCEDURE p IS BEGIN INSERT INTO t VALUES (1); END;".to_string()]
        );
        assert!(special.is_empty());
    }
}

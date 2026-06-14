//! Rebuild `datasets/sqlite/sqlite_official_suite.txt` from the original SQLite
//! official test suite, with a SQLite-aware statement splitter that keeps
//! compound `CREATE TRIGGER ... BEGIN ...; ... END` statements intact.
//!
//! The corpus is one statement per line. The original extractor (removed from the
//! repo) split on every `;`, which shredded trigger bodies on their inner
//! semicolons and produced invalid fragments (issue #22). This rebuilds the suite
//! correctly: it splits only on top-level `;` (outside string/identifier quotes,
//! comments, `BEGIN ... END` trigger bodies, and `CASE ... END`), normalizes each
//! statement to one line, strips comments, and dedupes within the suite and
//! against the other committed SQLite corpus files.
//!
//! Source: the SQLite project's own tests, public domain, as bundled in
//! codeschool/sqlite-parser under `test/sql/official-suite/*.sql`. Clone that repo
//! and pass the directory:
//!
//!   git clone --depth 1 https://github.com/codeschool/sqlite-parser /tmp/sp
//!   cargo run --release --bin build_sqlite_suite -- /tmp/sp/test/sql/official-suite
//!
//! Then repack (`tar --zstd -cf datasets.tar.zst datasets`) and re-run the SQLite
//! oracle (`cargo run --release -p oracle -- sqlite`).

#![allow(
    clippy::doc_markdown,
    clippy::too_many_lines,
    clippy::items_after_statements
)]

use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Split raw SQLite script text into normalized one-line statements.
///
/// Splits on top-level `;` only: semicolons inside single/double/backtick/bracket
/// quotes, `--` and block comments, a `CREATE TRIGGER` `BEGIN ... END` body, or a
/// `CASE ... END` are not statement terminators. Each statement is normalized to a
/// single line (whitespace runs collapsed) with comments removed.
#[must_use]
fn split_sql(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut word = String::new();
    let mut case_depth = 0usize;
    let mut block_depth = 0usize;
    let mut is_trigger = false;

    // Apply a completed word's effect on block/case tracking.
    fn classify(
        word: &mut String,
        case_depth: &mut usize,
        block_depth: &mut usize,
        is_trigger: &mut bool,
    ) {
        if word.is_empty() {
            return;
        }
        match word.to_ascii_uppercase().as_str() {
            "TRIGGER" => *is_trigger = true,
            "CASE" => *case_depth += 1,
            "END" => {
                if *case_depth > 0 {
                    *case_depth -= 1;
                } else if *block_depth > 0 {
                    *block_depth -= 1;
                }
            }
            // The only BEGIN inside a CREATE TRIGGER is the body opener. A bare
            // BEGIN (transaction) is not a trigger, so it does not open a block.
            "BEGIN" if *is_trigger => *block_depth += 1,
            _ => {}
        }
        word.clear();
    }

    // Push a single normalizing space (collapse runs, skip leading).
    fn push_space(buf: &mut String) {
        if !buf.is_empty() && !buf.ends_with(' ') {
            buf.push(' ');
        }
    }

    let end_statement = |buf: &mut String,
                         out: &mut Vec<String>,
                         case_depth: &mut usize,
                         block_depth: &mut usize,
                         is_trigger: &mut bool| {
        let s = buf.trim().to_string();
        if !s.is_empty() {
            // Final pass: collapse any whitespace that survived inside quoted
            // literals so the statement is one line (string contents do not
            // affect parse benchmarking).
            let normalized = s.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push(normalized);
        }
        buf.clear();
        *case_depth = 0;
        *block_depth = 0;
        *is_trigger = false;
    };

    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Comments: strip to a single space.
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            classify(
                &mut word,
                &mut case_depth,
                &mut block_depth,
                &mut is_trigger,
            );
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            push_space(&mut buf);
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            classify(
                &mut word,
                &mut case_depth,
                &mut block_depth,
                &mut is_trigger,
            );
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            push_space(&mut buf);
            continue;
        }

        // Quoted string / identifier: copy verbatim, honoring doubling escapes.
        if matches!(c, '\'' | '"' | '`' | '[') {
            classify(
                &mut word,
                &mut case_depth,
                &mut block_depth,
                &mut is_trigger,
            );
            let close = if c == '[' { ']' } else { c };
            buf.push(c);
            i += 1;
            loop {
                if i >= chars.len() {
                    break;
                }
                let d = chars[i];
                if d == close {
                    // Doubling escape ('' "" ``) keeps the quote open. Brackets
                    // have no escape in SQLite.
                    if close != ']' && chars.get(i + 1) == Some(&close) {
                        buf.push(d);
                        buf.push(d);
                        i += 2;
                        continue;
                    }
                    buf.push(d);
                    i += 1;
                    break;
                }
                buf.push(d);
                i += 1;
            }
            continue;
        }

        if c.is_alphanumeric() || c == '_' {
            word.push(c);
            buf.push(c);
            i += 1;
            continue;
        }

        // Non-word character: settle the pending word first.
        classify(
            &mut word,
            &mut case_depth,
            &mut block_depth,
            &mut is_trigger,
        );

        if c == ';' && case_depth == 0 && block_depth == 0 {
            end_statement(
                &mut buf,
                &mut out,
                &mut case_depth,
                &mut block_depth,
                &mut is_trigger,
            );
            i += 1;
            continue;
        }

        if c.is_whitespace() {
            push_space(&mut buf);
        } else {
            buf.push(c);
        }
        i += 1;
    }
    classify(
        &mut word,
        &mut case_depth,
        &mut block_depth,
        &mut is_trigger,
    );
    end_statement(
        &mut buf,
        &mut out,
        &mut case_depth,
        &mut block_depth,
        &mut is_trigger,
    );
    out
}

fn main() {
    let src = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: build_sqlite_suite <official-suite dir>");
        std::process::exit(2);
    });
    let src = Path::new(&src);

    // Statements already in the other committed SQLite corpus files, to dedupe
    // against (keep the suite from duplicating Spider / sql-create-context).
    let mut seen: HashSet<String> = HashSet::new();
    for other in ["spider_sqlite.txt", "sql_create_ctx.txt"] {
        let p = Path::new("datasets/sqlite").join(other);
        if let Ok(content) = fs::read_to_string(&p) {
            for line in content.lines() {
                let l = line.trim();
                if !l.is_empty() {
                    seen.insert(l.to_string());
                }
            }
        }
    }
    let existing = seen.len();

    let mut files: Vec<_> = fs::read_dir(src)
        .expect("read official-suite dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sql"))
        .collect();
    files.sort();

    let mut out_lines: Vec<String> = Vec::new();
    let mut total = 0usize;
    for f in &files {
        let content = fs::read_to_string(f).expect("read sql file");
        for stmt in split_sql(&content) {
            total += 1;
            if seen.insert(stmt.clone()) {
                out_lines.push(stmt);
            }
        }
    }

    let dest = Path::new("datasets/sqlite/sqlite_official_suite.txt");
    fs::write(dest, format!("{}\n", out_lines.join("\n"))).expect("write suite");
    println!(
        "{} source files, {total} statements parsed, {} kept after dedup ({} were dupes of the existing {existing} SQLite statements or each other).",
        files.len(),
        out_lines.len(),
        total - out_lines.len(),
    );
    println!("wrote {}", dest.display());
}

#[cfg(test)]
mod tests {
    use super::split_sql;

    #[test]
    fn keeps_trigger_body_intact() {
        let sql = "CREATE TRIGGER r1 AFTER INSERT ON t2 BEGIN\n  SELECT 'hello';\nEND;\nSELECT 1;";
        assert_eq!(
            split_sql(sql),
            vec![
                "CREATE TRIGGER r1 AFTER INSERT ON t2 BEGIN SELECT 'hello'; END".to_string(),
                "SELECT 1".to_string(),
            ]
        );
    }

    #[test]
    fn multi_statement_trigger_body_stays_one_statement() {
        let sql = "CREATE TRIGGER t AFTER UPDATE ON x BEGIN UPDATE a SET b=1; DELETE FROM c; END; DROP TABLE x;";
        assert_eq!(
            split_sql(sql),
            vec![
                "CREATE TRIGGER t AFTER UPDATE ON x BEGIN UPDATE a SET b=1; DELETE FROM c; END"
                    .to_string(),
                "DROP TABLE x".to_string(),
            ]
        );
    }

    #[test]
    fn leading_semicolons_and_newlines() {
        // The suite often puts the terminator at the start of the next line.
        let sql = "CREATE TABLE abc(a, b, c)\n;ALTER TABLE abc ADD d INTEGER\n;SELECT 1\n";
        assert_eq!(
            split_sql(sql),
            vec![
                "CREATE TABLE abc(a, b, c)".to_string(),
                "ALTER TABLE abc ADD d INTEGER".to_string(),
                "SELECT 1".to_string(),
            ]
        );
    }

    #[test]
    fn semicolons_in_strings_and_comments_do_not_split() {
        let sql = "SELECT ';' AS x -- ; not a split\n; SELECT /* ; */ 2;";
        assert_eq!(
            split_sql(sql),
            vec!["SELECT ';' AS x".to_string(), "SELECT 2".to_string()]
        );
    }

    #[test]
    fn case_end_does_not_close_a_trigger() {
        let sql =
            "CREATE TRIGGER t AFTER INSERT ON x BEGIN SELECT CASE WHEN 1 THEN 2 ELSE 3 END; END; SELECT 9;";
        assert_eq!(
            split_sql(sql),
            vec![
                "CREATE TRIGGER t AFTER INSERT ON x BEGIN SELECT CASE WHEN 1 THEN 2 ELSE 3 END; END"
                    .to_string(),
                "SELECT 9".to_string(),
            ]
        );
    }

    #[test]
    fn bare_begin_transaction_is_its_own_statement() {
        let sql = "BEGIN; INSERT INTO t VALUES(1); COMMIT;";
        assert_eq!(
            split_sql(sql),
            vec![
                "BEGIN".to_string(),
                "INSERT INTO t VALUES(1)".to_string(),
                "COMMIT".to_string(),
            ]
        );
    }
}

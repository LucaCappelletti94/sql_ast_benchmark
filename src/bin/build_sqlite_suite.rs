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
//! Source: the SQLite project's own `*.test` files (TCL, public domain). This
//! reads them directly, with no external parser. An earlier build went through
//! codeschool/sqlite-parser's PEG.js grammar to pre-extract SQL, but that
//! grammar's whitespace rule wrongly classifies backslash as whitespace, so it
//! mistokenized SQLite string literals containing a backslash (the JSON-escape
//! test vectors in `json101.test`), lost string boundaries, and glued several
//! statements into one malformed line. That single bad line then made the SQLite
//! oracle's batch `EXPLAIN` swallow thousands of following statements as one
//! unterminated string, silently grading them valid. Extracting the TCL test
//! bodies ourselves and re-splitting with our own tokenizer (which treats
//! backslash as an ordinary character, per real SQLite) removes that at the root.
//!
//! Clone the SQLite source at a release tag and pass its `test/` directory:
//!
//!   git clone --depth 1 -b version-3.53.0 https://github.com/sqlite/sqlite /tmp/sqlite
//!   cargo run --release --bin build_sqlite_suite -- /tmp/sqlite/test
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

/// TCL test commands whose first brace-delimited argument is a SQL script.
/// `do_*_test` take a test name first, `execsql`/`catchsql` take the script
/// directly; in both cases the first `{...}` group after the keyword is the SQL.
const SQL_CMDS: [&str; 5] = [
    "do_execsql_test",
    "do_catchsql_test",
    "do_eqp_test",
    "execsql",
    "catchsql",
];

/// Negative-test statements the schema-free SQLite oracle cannot grade correctly.
///
/// Each is genuinely invalid (a grammar rule SQLite enforces only after resolving
/// the referenced object: qualified table names / `NOT INDEXED` / `INDEXED BY` /
/// `RETURNING` / `DEFAULT VALUES` inside a trigger body, `ALTER TABLE ADD COLUMN
/// ... UNIQUE`/`PRIMARY KEY`, `NATURAL JOIN` with `ON`/`USING`, a few malformed
/// bodies). The oracle runs `EXPLAIN` against an empty database, so it hits
/// `no such table` first and masks the real error, grading them valid and
/// charging every parser that correctly rejects them. Verified invalid against
/// the real engine by recreating stub tables so it reaches the actual error.
/// Dropped from the corpus rather than mislabeled. Matched against the normalized
/// (single-spaced) statement that [`split_sql`] emits.
const SCHEMA_MASKED_INVALID: [&str; 25] = [
    "ALTER TABLE t1 ADD c PRIMARY KEY",
    "ALTER TABLE t1 ADD c UNIQUE",
    "ALTER TABLE t3651 ADD COLUMN b PRIMARY KEY",
    "ALTER TABLE t3651 ADD COLUMN b UNIQUE",
    "CREATE TABLE aux.g1(a, b, c, PRIMARY KEY(a, b)) %WO%",
    "CREATE TRIGGER AFTER DELETE ON a3 BEGIN INSERT INTO temp.tmptable VALUES(1, 2); END",
    "CREATE TRIGGER AFTER UPDATE ON a1 BEGIN INSERT INTO a4 DEFAULT VALUES; END",
    "CREATE TRIGGER AFTER UPDATE ON a1 BEGIN INSERT INTO main.a4 VALUES(new.a, new.b); END",
    "CREATE TRIGGER IF NOT EXISTS r1 AFTER DELETE ON t1 BEGIN INSERT INTO t1(a) VALUES (1) RETURNING FALSE; INSERT INTO t1(a) VALUES (2) RETURNING TRUE; END",
    "CREATE TRIGGER aux.tr2 AFTER UPDATE ON aux.t1 BEGIN UPDATE main.t2 SET c=new.e, d=new.f; END",
    "CREATE TRIGGER main.t16err1 AFTER INSERT ON tA BEGIN INSERT INTO main.t16 VALUES(1,2,3); END",
    "CREATE TRIGGER main.t16err2 AFTER INSERT ON tA BEGIN UPDATE main.t16 SET rowid=rowid+1; END",
    "CREATE TRIGGER main.t16err3 AFTER INSERT ON tA BEGIN DELETE FROM main.t16; END",
    "CREATE TRIGGER main.t16err4 AFTER INSERT ON tA BEGIN UPDATE t16 NOT INDEXED SET rowid=rowid+1; END",
    "CREATE TRIGGER main.t16err5 AFTER INSERT ON tA BEGIN UPDATE t16 INDEXED BY t16a SET rowid=rowid+1 WHERE a=1; END",
    "CREATE TRIGGER main.t16err6 AFTER INSERT ON tA BEGIN DELETE FROM t16 NOT INDEXED WHERE a=123; END",
    "CREATE TRIGGER main.t16err7 AFTER INSERT ON tA BEGIN DELETE FROM t16 INDEXED BY t16a WHERE a=123; END",
    "CREATE TRIGGER r1 AFTER INSERT ON t1 BEGIN INSERT INTO t1 SELECT e_master LIMIT 1,#1; END",
    "CREATE TRIGGER r1 AFTER INSERT ON t1 BEGIN SELECT * FROM t1; SELECT * FROM; END",
    "CREATE TRIGGER r1 AFTER INSERT ON t1 BEGIN SELECT * FROM; END",
    "CREATE TRIGGER tr1 AFTER DELETE ON t4 BEGIN UPDATE main.t1 SET a=1, b=2; END",
    "CREATE TRIGGER tr1 AFTER INSERT ON t2 BEGIN INSERT INTO aux.t1 VALUES(new.c, new.d); END",
    "CREATE TRIGGER tr3 AFTER DELETE ON t2 BEGIN DELETE FROM aux.t1; END",
    "SELECT * FROM t1 NATURAL JOIN t2 ON t1.a=t2.b",
    "SELECT * FROM t1 NATURAL JOIN t2 USING(b)",
];

const fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Whether a statement closes every string/identifier quote, bracket, and block
/// comment it opens. SQLite quotes: `'..'`/`".."`/`` `..` `` with doubling
/// escapes, `[..]` (no escape), `/* .. */`; backslash is an ordinary character.
///
/// A one-line corpus statement must be balanced: an unbalanced one is not valid
/// SQL, and worse, when the SQLite oracle batches statements as `EXPLAIN <s>;`
/// lines, one unterminated quote swallows every following line as a single
/// string and silently grades them all valid. A handful of TCL test artifacts
/// and deliberate unterminated-literal tests are unbalanced; drop them here so
/// they never enter the corpus.
#[must_use]
fn is_balanced(stmt: &str) -> bool {
    #[derive(PartialEq)]
    enum Quote {
        None,
        Single,
        Double,
        Back,
        Bracket,
        Block,
    }
    let bytes = stmt.as_bytes();
    let mut state = Quote::None;
    let mut idx = 0;
    while idx < bytes.len() {
        let byte = bytes[idx];
        match state {
            Quote::None => match byte {
                b'\'' => state = Quote::Single,
                b'"' => state = Quote::Double,
                b'`' => state = Quote::Back,
                b'[' => state = Quote::Bracket,
                b'-' if bytes.get(idx + 1) == Some(&b'-') => return true, // line comment to EOL
                b'/' if bytes.get(idx + 1) == Some(&b'*') => {
                    state = Quote::Block;
                    idx += 1;
                }
                _ => {}
            },
            Quote::Single | Quote::Double | Quote::Back => {
                let close = match state {
                    Quote::Single => b'\'',
                    Quote::Double => b'"',
                    _ => b'`',
                };
                if byte == close {
                    if bytes.get(idx + 1) == Some(&close) {
                        idx += 1; // doubling escape
                    } else {
                        state = Quote::None;
                    }
                }
            }
            Quote::Bracket => {
                if byte == b']' {
                    state = Quote::None;
                }
            }
            Quote::Block => {
                if byte == b'*' && bytes.get(idx + 1) == Some(&b'/') {
                    state = Quote::None;
                    idx += 1;
                }
            }
        }
        idx += 1;
    }
    state == Quote::None
}

/// Index just past the `}` matching the `{` at `open`, using TCL brace rules:
/// braces nest, but `\{`, `\}`, and `\\` are escapes that do not affect nesting
/// (no other substitution happens inside braces). `None` if unbalanced.
fn matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2; // skip the escaped byte (\{ \} \\ \<newline>)
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Extract the SQL scripts from a TCL `.test` file: for each [`SQL_CMDS`] keyword
/// at a word boundary, take the next brace-delimited group as the script. Bodies
/// are returned verbatim (TCL does no substitution inside braces, so `$x` and
/// `[...]` are literal); the caller re-splits each with [`split_sql`]. Forms that
/// pass the script as a quoted/substituted string (not `{...}`) are skipped.
fn extract_sql_bodies(input: &str) -> Vec<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let kw_len = SQL_CMDS.iter().find_map(|kw| {
            let k = kw.as_bytes();
            let boundary_before = i == 0 || !is_word_byte(bytes[i - 1]);
            let boundary_after = bytes
                .get(i + k.len())
                .is_some_and(|&c| c == b' ' || c == b'\t' || c == b'{');
            (boundary_before && boundary_after && bytes[i..].starts_with(k)).then_some(k.len())
        });
        let Some(kw_len) = kw_len else {
            i += 1;
            continue;
        };
        // Scan past the test name / options to the script's opening brace. Bail
        // on a double quote (quoted script form we do not handle) or an
        // unescaped end of line, so we never grab a later command's brace group.
        let mut j = i + kw_len;
        let body_open = loop {
            match bytes.get(j) {
                Some(b'{') => break Some(j),
                Some(b'"') | None => break None,
                Some(b'\n') => {
                    if j > 0 && bytes[j - 1] == b'\\' {
                        j += 1;
                        continue;
                    }
                    break None;
                }
                Some(_) => j += 1,
            }
        };
        match body_open.and_then(|o| matching_brace(bytes, o).map(|e| (o, e))) {
            Some((o, e)) => {
                out.push(input[o + 1..e - 1].to_string());
                i = e;
            }
            None => i += kw_len,
        }
    }
    out
}

fn main() {
    let src = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: build_sqlite_suite <sqlite test/ dir with *.test files>");
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
        .expect("read sqlite test/ dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "test"))
        .collect();
    files.sort();

    let masked: HashSet<&str> = SCHEMA_MASKED_INVALID.iter().copied().collect();
    let mut out_lines: Vec<String> = Vec::new();
    let mut total = 0usize;
    let mut unbalanced = 0usize;
    let mut masked_dropped = 0usize;
    for f in &files {
        let content = fs::read_to_string(f).expect("read test file");
        for body in extract_sql_bodies(&content) {
            for stmt in split_sql(&body) {
                total += 1;
                if !is_balanced(&stmt) {
                    unbalanced += 1;
                    continue;
                }
                if masked.contains(stmt.as_str()) {
                    masked_dropped += 1;
                    continue;
                }
                if seen.insert(stmt.clone()) {
                    out_lines.push(stmt);
                }
            }
        }
    }

    let dest = Path::new("datasets/sqlite/sqlite_official_suite.txt");
    fs::write(dest, format!("{}\n", out_lines.join("\n"))).expect("write suite");
    println!(
        "{} source files, {total} statements parsed, {} kept after dedup ({unbalanced} dropped as unbalanced, {masked_dropped} dropped as schema-masked invalids, {} were dupes of the existing {existing} SQLite statements or each other).",
        files.len(),
        out_lines.len(),
        total - out_lines.len() - unbalanced - masked_dropped,
    );
    println!("wrote {}", dest.display());
}

#[cfg(test)]
mod tests {
    use super::{
        extract_sql_bodies, is_balanced, matching_brace, split_sql, SCHEMA_MASKED_INVALID,
    };

    #[test]
    fn schema_masked_invalid_list_is_normalized_and_unique() {
        // Entries must equal their own split_sql normalization, else they will
        // never match a corpus statement and silently fail to exclude.
        for &s in &SCHEMA_MASKED_INVALID {
            let norm = split_sql(s);
            assert_eq!(
                norm,
                vec![s.to_string()],
                "entry not in normalized form: {s}"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for &s in &SCHEMA_MASKED_INVALID {
            assert!(seen.insert(s), "duplicate exclusion entry: {s}");
        }
    }

    #[test]
    fn balance_check_accepts_valid_and_rejects_unterminated() {
        assert!(is_balanced("SELECT json_valid('\" \\ \"')"));
        assert!(is_balanced("SELECT \"col\", 'a''b', `t`, [x], /* c */ 1"));
        assert!(is_balanced("SELECT '{\"a\":1}'")); // braces are not quotes
                                                    // The unterminated literals that swallow the oracle batch.
        assert!(!is_balanced("select 'abc"));
        assert!(!is_balanced("select \"abc"));
        assert!(!is_balanced("select [abc"));
        assert!(!is_balanced("SELECT X'01020, 100"));
    }

    #[test]
    fn extracts_do_execsql_test_body() {
        let tcl = "do_execsql_test foo-1.0 {\n  SELECT 1;\n} {1}\n";
        assert_eq!(extract_sql_bodies(tcl), vec!["\n  SELECT 1;\n".to_string()]);
    }

    #[test]
    fn extracts_execsql_and_catchsql_but_not_do_prefixed_substring() {
        // `execsql`/`catchsql` keywords match, but the `execsql` inside
        // `do_execsql_test` must not double-match (word boundary before it).
        let tcl = "execsql {CREATE TABLE t(a)}\ncatchsql {SELECT bad}\n";
        assert_eq!(
            extract_sql_bodies(tcl),
            vec!["CREATE TABLE t(a)".to_string(), "SELECT bad".to_string()]
        );
        let nested = "do_execsql_test x {SELECT 9}";
        assert_eq!(extract_sql_bodies(nested), vec!["SELECT 9".to_string()]);
    }

    #[test]
    fn brace_matching_honors_backslash_escapes_and_nesting() {
        // Balanced inner braces (JSON) are spanned; \{ and \} do not nest.
        let s = b"{ '{\"a\":1}' }";
        assert_eq!(matching_brace(s, 0), Some(s.len()));
        let esc = b"{ a \\{ b }";
        assert_eq!(matching_brace(esc, 0), Some(esc.len()));
        assert_eq!(matching_brace(b"{ unbalanced ", 0), None);
    }

    #[test]
    fn backslash_in_string_literal_does_not_glue_statements() {
        // The json101 regression: three separate, individually valid statements
        // whose string literals contain a backslash. The PEG.js grammar treated
        // `\` as whitespace and glued them; our extractor + splitter must keep
        // them three intact statements with the backslash preserved.
        let tcl = "do_execsql_test json101-10.1 {\n  SELECT json_valid('\" \\  \"');\n} {0}\n\
                   do_execsql_test json101-10.2 {\n  SELECT json_valid('\" \\! \"');\n} {0}\n\
                   do_execsql_test json101-10.3 {\n  SELECT json_valid('\" \\\" \"');\n} {1}\n";
        let stmts: Vec<String> = extract_sql_bodies(tcl)
            .iter()
            .flat_map(|b| split_sql(b))
            .collect();
        assert_eq!(
            stmts,
            vec![
                "SELECT json_valid('\" \\ \"')".to_string(),
                "SELECT json_valid('\" \\! \"')".to_string(),
                "SELECT json_valid('\" \\\" \"')".to_string(),
            ]
        );
    }

    #[test]
    fn skips_quoted_script_forms() {
        // `execsql "..."` (double-quoted, TCL-substituted) is not a `{...}` body.
        assert!(extract_sql_bodies("execsql \"SELECT $x\"\n").is_empty());
    }

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

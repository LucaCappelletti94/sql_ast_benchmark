//! Clean residual corpus artifacts left by the original `;`-only extractor in the
//! corpus files that have no upstream reconstruction tool (issue #22, the long
//! tail). The reconstructed SQLite/Spark/Oracle suites are rebuilt by
//! `build_sqlite_suite` / `build_proc_suites`; this pass repairs the rest in
//! place on the unpacked `datasets/`.
//!
//! Two transforms, both conservative (they never invent SQL and only ever drop a
//! line that cannot be a valid standalone statement):
//!
//!  1. T-SQL `GO` batch separators. `GO` is a sqlcmd/SSMS client directive, not
//!     T-SQL grammar. The extractor split on `;`, so `GO` lines with no semicolon
//!     were glued onto the next statement (`GO SELECT ...`) or sat between two
//!     statements on one line (`... GO ...`). The real SQL Server oracle accepts
//!     `GO <stmt>`, so every parser that correctly rejects `GO` was charged a
//!     false recall failure. We split each line on top-level `GO` tokens,
//!     recovering the real statements. Applied to the `tsql` corpus and the mixed
//!     `multi` corpus (which also carries T-SQL GO batches).
//!
//!  2. Pure procedural fragments (all dialects). Lines that are only a block
//!     keyword (`END IF`, `END LOOP`, `END TRY`, `BEGIN CATCH`, ...) or that start
//!     with a clause keyword that can never begin a statement (`ELSE`, `ELSIF`,
//!     `WHEN`, `THEN`, `AND`, `OR`, `LOOP`) are body pieces of a split
//!     `CREATE FUNCTION`/`PROCEDURE`/batch and are dropped. Bare `END`/`END;` is
//!     kept for SQLite, where `END` is a COMMIT synonym, but dropped elsewhere.
//!     `DELIMITER` client directives (any dialect) and MySQL/`multi` `//`
//!     routine-delimiter fragments are dropped too. The prefix rule also catches
//!     the string-literal fragments that begin mid-prose (`And then my heart ...`,
//!     `loop will exit ...`).
//!
//! A general multi-line string-literal repair was considered and rejected: the
//! corpus mixes `''`-doubling and backslash-escaping dialects (plus PG `E'...'`
//! and dollar-quoting), so a quote scanner mislabels valid statements wholesale.
//! The few genuine string fragments that remain are mostly provenance-only noise.
//!
//! Run `--apply` to write; otherwise it is a dry run reporting counts and samples.
//! After applying, repack `datasets.tar.zst` and re-run the T-SQL oracle (the
//! `GO` split produces new statement strings that need fresh labels).

#![allow(
    clippy::doc_markdown,
    clippy::too_many_lines,
    clippy::items_after_statements
)]

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use sql_ast_benchmark::datasets::{ensure_corpus, Dialect};

/// Split a T-SQL line on top-level `GO` batch separators (a `GO` token bounded by
/// whitespace or line edges, outside any quote). Returns the recovered statement
/// pieces (callers normalize/drop empties). Quote tracking (`'...'` with `''`
/// escaping, `"..."`, `[...]`) keeps a `GO` inside a literal from splitting; T-SQL
/// does not use backslash escapes, so `''`/`""` doubling is the only escape.
fn split_go(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut pieces = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Consume a quoted literal verbatim so a `GO` inside it is not a split.
        if matches!(c, '\'' | '"' | '[') {
            let close = if c == '[' { ']' } else { c };
            buf.push(c);
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
                    i += 1;
                    break;
                }
                buf.push(d);
                i += 1;
            }
            continue;
        }
        // A `GO` token: preceded by start-or-space, followed by space-or-end.
        let at_boundary = i == 0 || chars[i - 1].is_whitespace();
        if at_boundary
            && (c == 'G' || c == 'g')
            && matches!(chars.get(i + 1), Some('O' | 'o'))
            && chars.get(i + 2).is_none_or(|n| n.is_whitespace())
        {
            pieces.push(std::mem::take(&mut buf));
            i += 2;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    pieces.push(buf);
    pieces
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a balanced line is a pure procedural fragment that should be dropped.
fn is_procedural_fragment(line: &str, dialect: Dialect) -> bool {
    let up = normalize(line).to_ascii_uppercase();
    let bare = up.trim_end_matches(';').trim_end();

    // `END` and `END <one token>` (END IF / END LOOP / END CASE / END <label>)
    // are block closers, never a standalone statement, EXCEPT in SQLite where a
    // bare `END` (and `END TRANSACTION`) is a COMMIT synonym.
    let end_word = bare.strip_prefix("END ").map(str::trim);
    if bare == "END" || end_word.is_some_and(|w| !w.is_empty() && !w.contains(' ')) {
        if dialect == Dialect::Sqlite {
            return false;
        }
        return end_word != Some("TRANSACTION");
    }

    // Clause / control-flow keywords that can never begin a statement. `END TRY`
    // and `BEGIN TRY`/`CATCH` are matched as prefixes to also catch the glued
    // `END TRY BEGIN CATCH SELECT ...` chunks of a split TRY/CATCH batch.
    const PREFIX: &[&str] = &[
        "ELSE ",
        "ELSIF ",
        "ELSEIF ",
        "WHEN ",
        "THEN ",
        "AND ",
        "OR ",
        "LOOP ",
        "END TRY",
        "END CATCH",
        "BEGIN TRY",
        "BEGIN CATCH",
        "END IF",
        "END LOOP",
        "END WHILE",
        "END FOR",
        "END CASE",
    ];
    if PREFIX.iter().any(|p| up.starts_with(p)) {
        return true;
    }

    // `DELIMITER` is a client directive, never valid SQL in any dialect.
    if up.starts_with("DELIMITER ") || up == "DELIMITER" {
        return true;
    }
    // MySQL `//` custom-delimiter routine wreckage (also present in the mixed
    // `multi` corpus). `//` is not an operator in these dialects.
    if matches!(dialect, Dialect::Mysql | Dialect::Multi)
        && (up == "//" || up.ends_with(" //") || up.contains(" // "))
    {
        return true;
    }
    // Leaked python `print(...)` lines (from shell/python test fixtures, e.g. the
    // ClickHouse `print(xxhash...)` snippets). Never valid SQL, except T-SQL, where
    // `PRINT(expr)` is a real statement, so that dialect is excluded.
    if dialect != Dialect::Tsql && up.starts_with("PRINT(") {
        return true;
    }
    false
}

/// Result of repairing one corpus file.
struct FileStat {
    name: String,
    kept: usize,
    go_split: usize,
    dropped_proc: usize,
    dropped_dup: usize,
    sample_go: Vec<String>,
    sample_drop: Vec<String>,
}

fn repair_file(path: &Path, dialect: Dialect) -> (Vec<String>, FileStat) {
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stat = FileStat {
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        kept: 0,
        go_split: 0,
        dropped_proc: 0,
        dropped_dup: 0,
        sample_go: Vec::new(),
        sample_drop: Vec::new(),
    };

    for raw in content.lines() {
        if raw.trim().is_empty() {
            continue;
        }
        // 1. T-SQL GO batch-separator split. Also applied to the mixed `multi`
        // corpus, which carries T-SQL GO batches. Other dialects: identity.
        let pieces = if matches!(dialect, Dialect::Tsql | Dialect::Multi) {
            let p = split_go(raw);
            if p.len() > 1 {
                stat.go_split += 1;
                if stat.sample_go.len() < 6 {
                    stat.sample_go
                        .push(raw.chars().take(90).collect::<String>());
                }
            }
            p
        } else {
            vec![raw.to_string()]
        };

        // 2. Procedural-fragment drop on each resulting piece.
        for piece in pieces {
            let n = normalize(&piece);
            if n.is_empty() {
                continue;
            }
            if is_procedural_fragment(&n, dialect) {
                stat.dropped_proc += 1;
                if stat.sample_drop.len() < 12 {
                    stat.sample_drop
                        .push(n.chars().take(90).collect::<String>());
                }
                continue;
            }
            // Preserve the corpus's one-occurrence invariant (GO-splitting can
            // re-introduce duplicate SET/USE statements).
            if !seen.insert(n.clone()) {
                stat.dropped_dup += 1;
                continue;
            }
            out.push(n);
            stat.kept += 1;
        }
    }
    (out, stat)
}

fn main() {
    if let Err(e) = ensure_corpus() {
        eprintln!("ERROR: could not prepare datasets/: {e}");
        std::process::exit(1);
    }
    let apply = std::env::args().any(|a| a == "--apply");
    println!(
        "repair_corpus: {} (pass --apply to write)\n",
        if apply { "APPLYING" } else { "DRY RUN" }
    );

    for dialect in Dialect::ALL {
        let dir = Path::new("datasets").join(dialect.dir_name());
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "txt"))
            .collect();
        files.sort();
        for f in files {
            let (out, stat) = repair_file(&f, dialect);
            let changed = stat.go_split + stat.dropped_proc + stat.dropped_dup;
            if changed == 0 {
                continue;
            }
            println!(
                "{}/{}: kept {}, GO-split {}, dropped {} proc + {} dup",
                dialect.dir_name(),
                stat.name,
                stat.kept,
                stat.go_split,
                stat.dropped_proc,
                stat.dropped_dup
            );
            for s in &stat.sample_go {
                println!("    GO  | {s}");
            }
            for s in &stat.sample_drop {
                println!("    DROP| {s}");
            }
            if apply {
                fs::write(&f, format!("{}\n", out.join("\n"))).expect("write repaired corpus");
            }
        }
    }
    if !apply {
        println!("\nDry run only. Re-run with --apply to write, then repack and re-run the T-SQL oracle.");
    }
}

#[cfg(test)]
mod tests {
    use super::{is_procedural_fragment, split_go};
    use sql_ast_benchmark::datasets::Dialect;

    #[test]
    fn go_splits_leading_and_midline() {
        assert_eq!(
            split_go("SET XACT_ABORT ON GO SELECT 1 GO"),
            vec!["SET XACT_ABORT ON ", " SELECT 1 ", ""]
        );
        assert_eq!(split_go("GO SELECT 1"), vec!["", " SELECT 1"]);
    }

    #[test]
    fn go_inside_a_string_or_identifier_is_not_a_separator() {
        assert_eq!(split_go("SELECT 'A GO B'"), vec!["SELECT 'A GO B'"]);
        assert_eq!(
            split_go("SELECT 'it''s GO time'"),
            vec!["SELECT 'it''s GO time'"]
        );
        assert_eq!(split_go("SELECT [GO]"), vec!["SELECT [GO]"]);
        // A column/alias literally named GO would also be left intact only when
        // quoted; a bare GO token is always treated as the batch separator.
    }

    #[test]
    fn procedural_fragments_are_detected() {
        for s in [
            "END IF",
            "end loop",
            "END mylabel",
            "END if_function",
            "ELSE result := 1",
            "ELSIF x THEN y",
            "AND b = 2",
            "loop will exit after 30 seconds",
            "BEGIN CATCH",
            "END TRY BEGIN CATCH SELECT ERROR_LINE()",
        ] {
            assert!(is_procedural_fragment(s, Dialect::Postgresql), "{s:?}");
        }
    }

    #[test]
    fn real_statements_are_kept() {
        for s in [
            "SELECT 1",
            "CREATE TABLE t (a int)",
            "ALTER TABLE t RENAME CONSTRAINT c TO d",
            "WITH x AS (SELECT 1) SELECT * FROM x",
        ] {
            assert!(!is_procedural_fragment(s, Dialect::Postgresql), "{s:?}");
        }
    }

    #[test]
    fn bare_end_is_a_commit_in_sqlite_only() {
        assert!(!is_procedural_fragment("END", Dialect::Sqlite));
        assert!(!is_procedural_fragment("END;", Dialect::Sqlite));
        assert!(is_procedural_fragment("END", Dialect::Tsql));
    }

    #[test]
    fn leaked_python_print_is_dropped_except_tsql() {
        assert!(is_procedural_fragment(
            "print(xxhash.xxh3_128_hexdigest(b'ClickHouse').upper())",
            Dialect::Clickhouse
        ));
        assert!(is_procedural_fragment("print(t1_row.c2)", Dialect::Multi));
        // T-SQL PRINT(expr) is a real statement -> kept.
        assert!(!is_procedural_fragment("PRINT('hello')", Dialect::Tsql));
        // A normal SELECT is never matched.
        assert!(!is_procedural_fragment(
            "SELECT print_col FROM t",
            Dialect::Clickhouse
        ));
    }

    #[test]
    fn mysql_delimiter_wreckage_is_dropped() {
        assert!(is_procedural_fragment("delimiter //", Dialect::Mysql));
        assert!(is_procedural_fragment(
            "end // create function f() returns int",
            Dialect::Mysql
        ));
        // Same line in another dialect is not delimiter wreckage.
        assert!(!is_procedural_fragment(
            "SELECT 1 // comment",
            Dialect::Postgresql
        ));
    }
}

//! Shared construction of a multi-statement script for the batch benchmarks.
//!
//! Both the batch time bench (`benches/batch_parsing.rs`) and the batch memory
//! bench (`membench -- batch`) must feed parsers byte-identical input, so the
//! join lives here in one place rather than in each binary.

/// Whether a statement is safe to place in a concatenated batch script.
///
/// `COPY ... FROM STDIN` reads the lines that follow it as inline data until a
/// `\.` terminator, so in a single concatenated script it swallows every
/// statement after it as its data payload (the whole tail collapses into one
/// statement). It parses fine on its own, so it stays in the per-statement
/// benchmarks, but it must be excluded from the batch script. Statements are
/// single-line, so a token scan is enough.
#[must_use]
pub fn batch_eligible(stmt: &str) -> bool {
    let toks: Vec<String> = stmt
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let is_copy_from_stdin = toks.iter().any(|t| t == "copy")
        && toks.windows(2).any(|w| w[0] == "from" && w[1] == "stdin");
    !is_copy_from_stdin
}

/// Join accepted statements into a single multi-statement script.
///
/// The separator is a newline, then the `;` terminator, then a newline. The
/// leading newline is essential: a corpus statement is a single line and may end
/// in a `--` (or `#`) line comment, which runs to end of line, so a terminator
/// placed on the same line would be swallowed by that comment and silently merge
/// two statements into one malformed statement. Putting the terminator on its own
/// line closes any trailing line comment first. A trailing `;` on a statement is
/// stripped to avoid an empty statement between terminators, and the last
/// statement gets no terminator (none is required at end of input).
#[must_use]
pub fn join_batch(accepted: &[&str]) -> String {
    let mut out = String::with_capacity(accepted.iter().map(|s| s.len() + 3).sum());
    for (i, s) in accepted.iter().enumerate() {
        if i > 0 {
            out.push_str("\n;\n");
        }
        out.push_str(s.trim().trim_end_matches(';').trim_end());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::join_batch;

    #[test]
    fn joins_with_terminators_and_strips_trailing_semicolons() {
        assert_eq!(
            join_batch(&["SELECT 1;", "SELECT 2"]),
            "SELECT 1\n;\nSELECT 2"
        );
        // Already-terminated and whitespace-padded statements normalize cleanly.
        assert_eq!(
            join_batch(&["  SELECT 1 ;  ", "SELECT 2 ;"]),
            "SELECT 1\n;\nSELECT 2"
        );
    }

    #[test]
    fn terminator_survives_a_trailing_line_comment() {
        // The first statement ends in a -- line comment. The terminator must sit
        // on its own line so the comment does not swallow it and merge the two.
        let joined = join_batch(&["SELECT 1 -- note", "SELECT 2"]);
        assert_eq!(joined, "SELECT 1 -- note\n;\nSELECT 2");
        // The ; is on its own line, after the comment line closes.
        assert!(joined.contains("\n;\n"));
    }

    #[test]
    fn single_statement_has_no_terminator() {
        assert_eq!(join_batch(&["SELECT 1"]), "SELECT 1");
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(join_batch(&[]), "");
    }
}

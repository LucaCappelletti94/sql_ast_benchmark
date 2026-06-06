//! Shared construction of a multi-statement script for the batch benchmarks.
//!
//! Both the batch time bench (`benches/batch_parsing.rs`) and the batch memory
//! bench (`membench -- batch`) must feed parsers byte-identical input, so the
//! join lives here in one place rather than in each binary.

/// Join accepted statements into a single multi-statement script.
///
/// Each corpus statement is one line, so a `;`-and-newline separator yields an
/// unambiguous script. A trailing `;` on a statement is stripped first to avoid
/// an empty statement between terminators. The last statement gets no terminator
/// (none is required at end of input).
#[must_use]
pub fn join_batch(accepted: &[&str]) -> String {
    let mut out = String::with_capacity(accepted.iter().map(|s| s.len() + 2).sum());
    for (i, s) in accepted.iter().enumerate() {
        if i > 0 {
            out.push_str(";\n");
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
            "SELECT 1;\nSELECT 2"
        );
        // Already-terminated and whitespace-padded statements normalize cleanly.
        assert_eq!(
            join_batch(&["  SELECT 1 ;  ", "SELECT 2 ;"]),
            "SELECT 1;\nSELECT 2"
        );
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

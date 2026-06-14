//! Shared construction and sampling for the batch benchmarks.
//!
//! The batch axis measures how a parser handles a multi-statement script. Rather
//! than concatenating a parser's whole accepted set (where one statement that
//! mishandles the terminator makes the all-or-nothing `parse_sql` return zero and
//! voids the entire measurement), we draw `BATCH_K` random batches of `BATCH_M`
//! statements from the set the parser can individually digest, parse each as one
//! script, and report the share that reparse to the exact expected count plus the
//! time and memory over the batches that did. The time bench
//! (`benches/batch_parsing.rs`), the memory bench (`membench -- batch`), and the
//! time machine (`timemachine`) all use the helpers here so they sample and join
//! identically.

/// Statements per sampled batch.
pub const BATCH_M: usize = 128;

/// Number of sampled batches per (parser, dialect).
pub const BATCH_K: usize = 200;

/// Base seed for the deterministic sampler. Mixed per (parser, dialect) so each
/// pair samples reproducibly but distinctly.
pub const BATCH_SEED: u64 = 0x5108_5A17_B47C_0DE5;

/// A three-distinct-statement probe to check a parser reports a true count.
///
/// A parser whose batch entry point returns something other than 3 here (for
/// example `pg_query` summary mode, which returns the number of distinct
/// statement types) cannot be scored on batch accuracy and is left out of the
/// batch axis.
pub const COUNT_PROBE: &str = "SELECT 1\n;\nSELECT 2\n;\nSELECT 3";

/// Whether `count` (a parser's whole-script statement count) reports a true
/// statement count, checked against [`COUNT_PROBE`].
pub fn reports_statement_count(mut count: impl FnMut(&str) -> usize) -> bool {
    count(COUNT_PROBE) == 3
}

/// Join statements into a single multi-statement script.
///
/// The separator is a newline, then the `;` terminator, then a newline. The
/// leading newline is essential: a corpus statement is a single line and may end
/// in a `--` (or `#`) line comment, which runs to end of line, so a terminator
/// placed on the same line would be swallowed by that comment and silently merge
/// two statements into one. Putting the terminator on its own line closes any
/// trailing line comment first. A trailing `;` is stripped to avoid an empty
/// statement between terminators, and the last statement gets no terminator.
#[must_use]
pub fn join_batch(stmts: &[&str]) -> String {
    let mut out = String::with_capacity(stmts.iter().map(|s| s.len() + 3).sum());
    for (i, s) in stmts.iter().enumerate() {
        if i > 0 {
            out.push_str("\n;\n");
        }
        out.push_str(s.trim().trim_end_matches(';').trim_end());
    }
    out
}

/// Whether a statement is safe to place in a concatenated batch script.
///
/// `COPY ... FROM STDIN` reads the lines that follow it as inline data until a
/// `\.` terminator, so in a single script it swallows every statement after it.
/// It parses fine on its own, so it stays in the per-statement benchmarks, but it
/// must be excluded from the batch. Statements are single-line, so a token scan
/// is enough.
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

/// Deterministic `SplitMix64`. Used to sample batches reproducibly without
/// pulling in an RNG dependency (the rest of the benchmark is deterministic).
struct SplitMix64(u64);

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish index in `0..n` (n > 0). Modulo bias is negligible here.
    const fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// A reproducible per-pair seed derived from the base seed and a label.
#[must_use]
pub fn seed_for(label: &str) -> u64 {
    let mut h = BATCH_SEED;
    for &b in label.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Sample `k` batches of distinct indices from `0..n`.
///
/// Each batch holds `min(m, n)` distinct indices (partial Fisher-Yates), batches
/// may overlap, and the result is deterministic for a given `seed`. Returns an
/// empty vec when `n == 0`.
#[must_use]
pub fn sample_batches(n: usize, m: usize, k: usize, seed: u64) -> Vec<Vec<usize>> {
    if n == 0 {
        return Vec::new();
    }
    let take = m.min(n);
    let mut rng = SplitMix64::new(seed);
    let mut pool: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        // Partial Fisher-Yates: swap `take` random picks to the front, then read.
        for i in 0..take {
            let j = i + rng.below(n - i);
            pool.swap(i, j);
        }
        out.push(pool[..take].to_vec());
    }
    out
}

/// Result of measuring a parser on `k` sampled batches.
pub struct BatchEval {
    /// Statements eligible for batching (accepted, single, not input-consuming).
    pub n_eligible: usize,
    /// Distinct statements per batch actually used (`min(BATCH_M, n_eligible)`).
    pub effective_m: usize,
    /// Number of batches attempted.
    pub k: usize,
    /// Batches that reparsed to exactly `effective_m` statements.
    pub n_correct: usize,
    /// The joined scripts of the correct batches, for timing or memory probing.
    pub correct_scripts: Vec<String>,
}

impl BatchEval {
    /// Accuracy as a percentage, or `None` when nothing was eligible.
    #[must_use]
    pub fn accuracy_pct(&self) -> Option<f64> {
        (self.k > 0).then(|| 100.0 * self.n_correct as f64 / self.k as f64)
    }
}

/// Sample batches from `eligible` and find those that reparse to the full count.
///
/// Draws `BATCH_K` batches of `BATCH_M` (seeded reproducibly by `label`), joins
/// each, and uses `count` (the parser's whole-script statement count) to keep the
/// batches that reparse to exactly `effective_m`. `eligible` must already be
/// filtered to statements the parser accepts, that parse to exactly one statement
/// alone, and that satisfy [`batch_eligible`].
pub fn evaluate_batches(
    eligible: &[&str],
    label: &str,
    mut count: impl FnMut(&str) -> usize,
) -> BatchEval {
    let n = eligible.len();
    let batches = sample_batches(n, BATCH_M, BATCH_K, seed_for(label));
    let effective_m = BATCH_M.min(n);
    let mut correct_scripts = Vec::new();
    for idxs in &batches {
        let stmts: Vec<&str> = idxs.iter().map(|&i| eligible[i]).collect();
        let script = join_batch(&stmts);
        if count(&script) == effective_m {
            correct_scripts.push(script);
        }
    }
    BatchEval {
        n_eligible: n,
        effective_m,
        k: batches.len(),
        n_correct: correct_scripts.len(),
        correct_scripts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_with_terminator_on_its_own_line() {
        assert_eq!(
            join_batch(&["SELECT 1;", "SELECT 2"]),
            "SELECT 1\n;\nSELECT 2"
        );
        assert_eq!(
            join_batch(&["  SELECT 1 ;  ", "SELECT 2 ;"]),
            "SELECT 1\n;\nSELECT 2"
        );
    }

    #[test]
    fn terminator_survives_a_trailing_line_comment() {
        let joined = join_batch(&["SELECT 1 -- note", "SELECT 2"]);
        assert_eq!(joined, "SELECT 1 -- note\n;\nSELECT 2");
        assert!(joined.contains("\n;\n"));
    }

    #[test]
    fn single_statement_has_no_terminator() {
        assert_eq!(join_batch(&["SELECT 1"]), "SELECT 1");
        assert_eq!(join_batch(&[]), "");
    }

    #[test]
    fn copy_from_stdin_is_excluded() {
        assert!(!batch_eligible("COPY t FROM STDIN"));
        assert!(!batch_eligible("copy t  from   stdin null 'x'"));
        assert!(batch_eligible("SELECT 1"));
        assert!(batch_eligible("INSERT INTO t SELECT * FROM other"));
    }

    #[test]
    fn sampler_is_deterministic_distinct_and_sized() {
        let a = sample_batches(1000, 128, 200, 42);
        let b = sample_batches(1000, 128, 200, 42);
        assert_eq!(a, b, "same seed gives same batches");
        assert_ne!(
            a,
            sample_batches(1000, 128, 200, 43),
            "seed changes batches"
        );
        assert_eq!(a.len(), 200);
        for batch in &a {
            assert_eq!(batch.len(), 128);
            let mut sorted = batch.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), 128, "indices within a batch are distinct");
            assert!(batch.iter().all(|&i| i < 1000));
        }
    }

    #[test]
    fn sampler_handles_small_and_empty_pools() {
        assert!(sample_batches(0, 128, 200, 1).is_empty());
        let small = sample_batches(10, 128, 5, 1);
        assert_eq!(small.len(), 5);
        for batch in &small {
            assert_eq!(batch.len(), 10, "effective_m caps at the pool size");
        }
    }

    #[test]
    fn accuracy_drops_when_a_swallower_is_present() {
        // A toy "parser": counts ';'-separated parts, but a statement that begins
        // with SWALLOW eats the rest of the script (returns 1). Mirrors how a real
        // terminator bug collapses the count.
        let count = |script: &str| {
            if script.contains("SWALLOW") {
                1
            } else {
                script.split("\n;\n").count()
            }
        };
        let mut clean: Vec<&str> = Vec::new();
        let owned: Vec<String> = (0..500).map(|i| format!("SELECT {i}")).collect();
        for s in &owned {
            clean.push(s);
        }
        let ok = evaluate_batches(&clean, "clean", count);
        assert_eq!(ok.accuracy_pct(), Some(100.0));

        let mut withbug = clean.clone();
        withbug.push("SWALLOW");
        let bug = evaluate_batches(&withbug, "bug", count);
        let acc = bug.accuracy_pct().unwrap();
        assert!(
            acc > 0.0 && acc < 100.0,
            "accuracy {acc} should be between 0 and 100"
        );
    }
}

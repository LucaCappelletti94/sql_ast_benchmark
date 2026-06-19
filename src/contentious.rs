//! Contentious-construct classifier and rule registry.
//!
//! A *contentious* construct is one the reference engine accepts but a parser may
//! reasonably decline to support (a niche engine quirk, a non-standard extension,
//! a lossy or deprecated form). Strict, oracle-graded recall is never affected by
//! this layer: it only tags statements for presentation and the secondary metric.
//!
//! Rules are data, one TOML file per rule under [`RULES_DIR`]. A rule is either a
//! `regex` rule (a [`regex`] pattern matched against a masked form of the
//! statement) or a `structural` rule (a named built-in predicate for properties a
//! regex cannot express, such as a repeated identifier). The `regex` crate matches
//! in guaranteed linear time with no backreferences, so a contributed pattern
//! cannot run arbitrary code or cause catastrophic backtracking.

use crate::datasets::Dialect;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

/// Directory holding the committed rule files, relative to the working directory.
pub const RULES_DIR: &str = "contentious";

/// Why a construct is contentious. A small fixed enum so meaning stays stable and
/// new categories are a deliberate change, not a contributor free-for-all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// A quirk tied to one engine's history or embedding (e.g. TCL variables).
    EngineSpecific,
    /// Accepted by the engine but outside the SQL standard and rejected by peers.
    NonStandard,
    /// Accepted but with surprising or implementation-defined semantics.
    LossyOrAmbiguous,
    /// Accepted for backward compatibility but discouraged by the engine's docs.
    Deprecated,
}

impl Category {
    /// The kebab-case wire form, also used in the rule files and the export.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EngineSpecific => "engine-specific",
            Self::NonStandard => "non-standard",
            Self::LossyOrAmbiguous => "lossy-or-ambiguous",
            Self::Deprecated => "deprecated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Regex,
    Structural,
}

/// One rule file under [`RULES_DIR`], deserialized from TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct RuleFile {
    pub id: String,
    pub title: String,
    pub category: Category,
    /// Dir names of the dialects this rule may fire in, or a single `"all"`.
    pub dialects: Vec<String>,
    kind: Kind,
    /// The regex pattern (regex rules only).
    #[serde(default)]
    pub pattern: Option<String>,
    /// The built-in predicate name (structural rules only).
    #[serde(default)]
    pub predicate: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub references: Vec<String>,
    /// Statements the rule must match (verified in tests).
    #[serde(default)]
    pub matches: Vec<String>,
    /// Statements the rule must not match (verified in tests).
    #[serde(default)]
    pub non_matches: Vec<String>,
}

/// A built-in structural predicate, run against the original statement.
type Predicate = fn(&str) -> bool;

enum Matcher {
    Regex(Regex),
    Structural(Predicate),
}

/// A loaded, compiled rule: its metadata plus its matcher.
pub struct Rule {
    pub meta: RuleFile,
    matcher: Matcher,
}

impl Rule {
    /// Whether this rule is allowed to fire in `dialect`.
    #[must_use]
    pub fn applies_to(&self, dialect: Dialect) -> bool {
        self.meta
            .dialects
            .iter()
            .any(|d| d == "all" || d == dialect.dir_name())
    }

    /// Whether this rule matches. Regex rules see the masked form, structural
    /// rules see the original statement.
    fn is_match(&self, masked: &str, original: &str) -> bool {
        match &self.matcher {
            Matcher::Regex(re) => re.is_match(masked),
            Matcher::Structural(p) => p(original),
        }
    }
}

/// Map a structural predicate name to its built-in implementation. Structural
/// rules are a closed set, extended only by a PR that touches this file.
fn structural_predicate(name: &str) -> Option<Predicate> {
    match name {
        "duplicate_columns" => Some(duplicate_columns),
        _ => None,
    }
}

/// The loaded rule registry.
pub struct Registry {
    pub rules: Vec<Rule>,
}

impl Registry {
    /// Load and compile every `*.toml` rule under `dir`, in id order.
    ///
    /// # Errors
    ///
    /// Returns an error (rather than panicking) on a bad file, a bad regex, an
    /// unknown predicate, a missing `pattern`/`predicate`, or a duplicate id, so
    /// callers can fail the build or a test with a precise message.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("reading {}: {e}", dir.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect();
        paths.sort();

        let mut rules = Vec::new();
        let mut ids = HashSet::new();
        for path in paths {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("reading {}: {e}", path.display()))?;
            let meta: RuleFile =
                toml::from_str(&text).map_err(|e| format!("parsing {}: {e}", path.display()))?;
            if !ids.insert(meta.id.clone()) {
                return Err(format!("duplicate rule id `{}`", meta.id));
            }
            let matcher =
                match meta.kind {
                    Kind::Regex => {
                        let pat = meta.pattern.as_ref().ok_or_else(|| {
                            format!("rule `{}`: regex rule needs `pattern`", meta.id)
                        })?;
                        Matcher::Regex(
                            Regex::new(pat)
                                .map_err(|e| format!("rule `{}`: bad regex: {e}", meta.id))?,
                        )
                    }
                    Kind::Structural => {
                        let name = meta.predicate.as_ref().ok_or_else(|| {
                            format!("rule `{}`: structural rule needs `predicate`", meta.id)
                        })?;
                        Matcher::Structural(structural_predicate(name).ok_or_else(|| {
                            format!("rule `{}`: unknown predicate `{name}`", meta.id)
                        })?)
                    }
                };
            rules.push(Rule { meta, matcher });
        }
        Ok(Self { rules })
    }

    /// The first rule that fires for `sql` in `dialect`, if any. A statement is
    /// contentious when this returns `Some`.
    #[must_use]
    pub fn classify(&self, sql: &str, dialect: Dialect) -> Option<&Rule> {
        let masked = mask(sql);
        self.rules
            .iter()
            .find(|r| r.applies_to(dialect) && r.is_match(&masked, sql))
    }
}

/// The process-wide registry, loaded once from [`RULES_DIR`].
///
/// # Panics
///
/// Panics with the load error if the rule files are malformed, which fails the
/// export build.
#[must_use]
pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        Registry::load(Path::new(RULES_DIR)).unwrap_or_else(|e| panic!("contentious registry: {e}"))
    })
}

/// Produce a masked form of `sql` for matching.
///
/// String and blob literals and comments are replaced by a single space and runs
/// of whitespace are collapsed, so a regex rule matches a predictable token stream
/// and a literal like `'$::x'` or a comment cannot trigger a false match.
/// Double-quoted, backtick, and bracket identifiers are left intact, since an
/// identifier is not a literal.
#[must_use]
pub fn mask(sql: &str) -> String {
    let b = sql.as_bytes();
    // Build raw bytes: only whole literal/comment regions (delimited by ASCII
    // bytes that never occur mid-UTF-8) are dropped, so the result stays valid
    // UTF-8 and multibyte identifiers pass through unchanged.
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        // Line comment: -- ... end of line.
        if c == b'-' && i + 1 < b.len() && b[i + 1] == b'-' {
            out.push(b' ');
            i += 2;
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment: /* ... */.
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            out.push(b' ');
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
            continue;
        }
        // Blob literal: x'...' or X'...', but only when `x` starts a token. After
        // an identifier character it is concatenation like `max` + `'foo'`, not a
        // blob, so the following string is masked by the single-quote branch.
        let x_starts_token = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if (c == b'x' || c == b'X') && x_starts_token && i + 1 < b.len() && b[i + 1] == b'\'' {
            out.push(b' ');
            i += 2;
            while i < b.len() && b[i] != b'\'' {
                i += 1;
            }
            i = (i + 1).min(b.len());
            continue;
        }
        // Single-quoted string with '' escape.
        if c == b'\'' {
            out.push(b' ');
            i += 1;
            while i < b.len() {
                if b[i] == b'\'' {
                    if i + 1 < b.len() && b[i + 1] == b'\'' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    // Collapse whitespace runs to single spaces.
    String::from_utf8_lossy(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Structural predicate: a column named more than once in an `INSERT` target
/// list, an `UPDATE ... SET (cols) = ...` group, or a `USING (cols)` clause.
/// Operates on the masked form so a `)` inside a string literal cannot break the
/// list capture. Identifier comparison is case-insensitive.
fn duplicate_columns(sql: &str) -> bool {
    static LISTS: OnceLock<Vec<Regex>> = OnceLock::new();
    let lists = LISTS.get_or_init(|| {
        vec![
            // INSERT INTO <table> ( <col list> )
            Regex::new(r"(?i)\binsert\s+into\s+[^\s(]+\s*\(([^)]*)\)").unwrap(),
            // SET ( <col list> ) =
            Regex::new(r"(?i)\bset\s*\(([^)]*)\)\s*=").unwrap(),
            // USING ( <col list> )
            Regex::new(r"(?i)\busing\s*\(([^)]*)\)").unwrap(),
        ]
    });
    let masked = mask(sql);
    for re in lists {
        for caps in re.captures_iter(&masked) {
            if let Some(list) = caps.get(1) {
                if has_duplicate_identifier(list.as_str()) {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether a comma-separated identifier list names any identifier twice
/// (case-insensitive, surrounding quotes or backticks stripped). Case-insensitive
/// comparison is correct for `SQLite` (the only dialect declaring this rule
/// today). A case-sensitive dialect would want quoted identifiers compared as-is.
fn has_duplicate_identifier(list: &str) -> bool {
    let mut seen = HashSet::new();
    for part in list.split(',') {
        let id = part
            .trim()
            .trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']')
            .to_ascii_lowercase();
        if id.is_empty() {
            continue;
        }
        if !seen.insert(id) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> Registry {
        Registry::load(Path::new(RULES_DIR)).expect("load registry")
    }

    #[test]
    fn registry_loads_and_ids_are_unique() {
        let r = reg();
        assert!(!r.rules.is_empty(), "expected at least one rule");
        let mut ids: Vec<_> = r.rules.iter().map(|x| x.meta.id.as_str()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len(), "rule ids must be unique");
    }

    #[test]
    fn every_rule_matches_its_examples_and_skips_non_matches() {
        let r = reg();
        for rule in &r.rules {
            // Examples are checked against the dialects the rule declares (the
            // first one is enough, a rule fires identically in any allowed one).
            let dialect = rule
                .meta
                .dialects
                .iter()
                .find_map(|d| Dialect::from_dir_name(d))
                .unwrap_or(Dialect::Sqlite);
            for ex in &rule.meta.matches {
                let masked = mask(ex);
                assert!(
                    rule.is_match(&masked, ex),
                    "rule `{}` should match `{ex}`",
                    rule.meta.id
                );
            }
            for ex in &rule.meta.non_matches {
                let masked = mask(ex);
                assert!(
                    !rule.is_match(&masked, ex),
                    "rule `{}` should not match `{ex}`",
                    rule.meta.id
                );
            }
            // Sanity: the rule applies to at least one real dialect.
            assert!(
                rule.applies_to(dialect) || rule.meta.dialects.iter().any(|d| d == "all"),
                "rule `{}` declares no resolvable dialect",
                rule.meta.id
            );
        }
    }

    #[test]
    fn mask_hides_literals_and_comments() {
        assert_eq!(mask("SELECT '$::x' AS lit"), "SELECT AS lit");
        assert_eq!(mask("SELECT 1 -- $::x\n, 2"), "SELECT 1 , 2");
        assert_eq!(mask("SELECT /* $::y */ 1"), "SELECT 1");
        assert_eq!(mask("SELECT x'4869' "), "SELECT");
        // `x` ending an identifier is concatenation, not a blob: the identifier
        // survives and only the adjacent string is masked.
        assert_eq!(mask("SELECT max'foo'"), "SELECT max");
        // Doubled '' escape stays inside one string.
        assert_eq!(mask("SELECT 'a''b' x"), "SELECT x");
        // Outside a literal, the token survives.
        assert_eq!(mask("select $::xyz"), "select $::xyz");
    }

    #[test]
    fn every_category_string_roundtrips() {
        for (c, s) in [
            (Category::EngineSpecific, "engine-specific"),
            (Category::NonStandard, "non-standard"),
            (Category::LossyOrAmbiguous, "lossy-or-ambiguous"),
            (Category::Deprecated, "deprecated"),
        ] {
            assert_eq!(c.as_str(), s);
        }
    }

    /// Coverage guard against the real corpus: every rule must match at least one
    /// engine-valid statement, so a dead or misscoped rule cannot land silently.
    ///
    /// Scope (a rule never excusing genuinely-invalid SQL) is enforced at the call
    /// sites, not here: the classifier is only ever consulted on engine-valid
    /// statements (`contentious_valid` filters to valid first, and failure tags
    /// come from the expected-valid set), so a rule matching an invalid statement
    /// has no effect. A construct like duplicate columns legitimately appears in
    /// otherwise-invalid statements too, so we count coverage among valid ones.
    #[test]
    fn every_rule_covers_at_least_one_valid_corpus_statement() {
        crate::datasets::ensure_corpus().expect("prepare datasets");
        let registry = reg();
        for rule in &registry.rules {
            let mut hits = 0usize;
            for d in Dialect::ALL
                .iter()
                .copied()
                .filter(|&d| rule.applies_to(d) && crate::has_reference(d))
            {
                for s in crate::report::load_dialect(d) {
                    if crate::reference_accepts(&s, d) != Some(true) {
                        continue;
                    }
                    if rule.is_match(&mask(&s), &s) {
                        hits += 1;
                    }
                }
            }
            assert!(
                hits > 0,
                "rule `{}` matched no engine-valid corpus statement (dead rule)",
                rule.meta.id
            );
        }
    }

    #[test]
    fn duplicate_columns_predicate() {
        assert!(duplicate_columns(
            "INSERT INTO dup1(a,b,c,a,b,c) VALUES(1,2,3,4,5,6)"
        ));
        assert!(duplicate_columns(
            "UPDATE t1 SET (a,a,a,b)=(SELECT 99,100,101,102)"
        ));
        assert!(duplicate_columns("SELECT y FROM t1 JOIN t1 USING (y,y)"));
        assert!(!duplicate_columns("INSERT INTO t(a,b,c) VALUES(1,2,3)"));
        assert!(!duplicate_columns("UPDATE t1 SET (a,b)=(SELECT 1,2)"));
        assert!(!duplicate_columns("SELECT y FROM t1 JOIN t2 USING (y)"));
        // A repeated name inside a string literal must not count.
        assert!(!duplicate_columns("INSERT INTO t(a,b) VALUES('x,x,x', 1)"));
    }
}

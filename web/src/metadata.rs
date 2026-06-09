//! Per-parser repository and crate metadata, shown as a stats block on each
//! parser detail page. Keyed by display name.
//!
//! This is a hand-recorded snapshot (see [`SNAPSHOT`]) from GitHub, crates.io,
//! and Codeberg, not fetched at runtime, so the figures are point-in-time and
//! the wasm build stays free of any network access. Refresh the numbers and
//! the date together when revising.

use crate::cadence::Cadence;

/// The date the figures below were collected (ISO 8601).
pub const SNAPSHOT: &str = "2026-05-31";

/// Whether a parser ships its own fuzzing harness.
#[derive(Clone, Copy, PartialEq)]
pub enum Fuzz {
    /// A fuzz harness lives in the repository.
    Yes,
    /// No harness in the binding, but it wraps libpg_query (PostgreSQL's own C
    /// parser), which is fuzzed by its upstream maintainers.
    Upstream,
    /// No fuzzing.
    No,
}

impl Fuzz {
    /// Short label for display.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Fuzz::Yes => "yes",
            // No harness in the binding itself, but the wrapped C parser
            // (libpg_query) is fuzzed by its own upstream maintainers.
            Fuzz::Upstream => "via libpg_query",
            Fuzz::No => "no",
        }
    }

    /// Whether this counts as fuzzed for the purpose of flagging. `Upstream`
    /// (the wrapped C parser is fuzzed) is treated as acceptable, only `No` is
    /// problematic.
    #[must_use]
    pub const fn is_ok(self) -> bool {
        !matches!(self, Fuzz::No)
    }

    /// A full sentence describing the fuzzing status, for tooltips and screen
    /// readers. Fuzzing feeds random or malformed input to surface crashes.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Fuzz::Yes => "Fuzz tested: the project ships a fuzzing harness that feeds random and malformed input to surface crashes and panics.",
            Fuzz::Upstream => "Fuzz tested upstream: this binding has no harness of its own, but the wrapped C parser (libpg_query) is fuzzed by its own maintainers.",
            Fuzz::No => "Not fuzz tested: the project ships no fuzzing harness, so crash-inducing or malformed input is less likely to have been caught.",
        }
    }
}

/// A full sentence describing whether the repository ships a test suite.
#[must_use]
pub const fn tests_description(has: bool) -> &'static str {
    if has {
        "Has a test suite: the repository ships automated tests that guard against regressions."
    } else {
        "No test suite: the repository ships no automated tests, so correctness is unguarded against regressions."
    }
}

/// A full sentence describing whether the repository ships criterion benchmarks.
#[must_use]
pub const fn benches_description(has: bool) -> &'static str {
    if has {
        "Has a benchmark suite: the repository ships criterion performance benchmarks."
    } else {
        "No benchmark suite: the repository ships no criterion performance benchmarks, so its speed is not tracked over time."
    }
}

/// A full sentence describing whether the crate builds without the standard
/// library, which decides if it can live in constrained runtimes.
#[must_use]
pub const fn no_std_description(no_std: bool) -> &'static str {
    if no_std {
        "Runs without the standard library (no_std plus alloc), so it can be embedded in firmware, kernels, and other bare-metal targets."
    } else {
        "Needs the standard library, so it cannot be dropped into no_std contexts such as embedded firmware or kernel code."
    }
}

/// A full sentence describing whether the crate compiles to WebAssembly, which
/// decides if it can run client-side rather than only on a server.
#[must_use]
pub const fn wasm_description(wasm: bool) -> &'static str {
    if wasm {
        "Compiles to wasm32-unknown-unknown, so it can parse SQL client-side in the browser or any other WebAssembly runtime."
    } else {
        "Will not build for wasm32-unknown-unknown (it pulls in C code or a non-wasm dependency), so it can only run server-side."
    }
}

/// The year the metadata snapshot was taken, parsed from [`SNAPSHOT`].
const SNAPSHOT_YEAR: u16 = 2026;

/// Interpretive sentence for the star count: what popularity it signals.
#[must_use]
pub fn stars_description(stars: u32) -> String {
    let note = if stars >= 1000 {
        "a widely noticed project with broad community adoption"
    } else if stars >= 100 {
        "a modest but real following"
    } else {
        "little community traction so far, so expect a smaller user base to find and report issues"
    };
    format!("Popularity proxy: {} GitHub stars, {note}.", commas(stars))
}

/// Interpretive sentence for the fork count.
#[must_use]
pub fn forks_description(forks: u32) -> String {
    format!(
        "{} forks, a rough measure of how many developers have copied the code to patch or contribute back.",
        commas(forks)
    )
}

/// Interpretive sentence for the commit count: development depth.
#[must_use]
pub fn commits_description(commits: u32) -> String {
    let note = if commits >= 1000 {
        "a long, deep development history"
    } else if commits >= 200 {
        "a moderate development history"
    } else {
        "a short history, so the parser is comparatively young and less battle-tested"
    };
    format!("{} commits, indicating {note}.", commas(commits))
}

/// Interpretive sentence for the contributor count, flagging bus-factor risk.
#[must_use]
pub fn contributors_description(contributors: u32) -> String {
    let note = match contributors {
        0 | 1 => "maintained by a single person, a bus-factor risk if they step away",
        2..=3 => "a small maintainer pool, so maintenance rests on a few people",
        _ => "a broad contributor base, spreading maintenance across many people",
    };
    format!("{} contributors, {note}.", commas(contributors))
}

/// Interpretive sentence for the first-release year: project maturity.
#[must_use]
pub fn since_description(year: u16) -> String {
    let age = SNAPSHOT_YEAR.saturating_sub(year);
    let note = match age {
        0 | 1 => "brand new, so it has had little time to harden against edge cases",
        2..=4 => "still maturing",
        _ => "a mature, long-lived project",
    };
    format!("First appeared in {year}, around {age} years old, {note}.")
}

/// Interpretive sentence for the download count: real-world usage.
#[must_use]
pub fn downloads_description(downloads: &str) -> String {
    format!(
        "{downloads} all-time crates.io downloads, a proxy for how much real-world code already depends on it."
    )
}

/// Interpretive sentence for the license, flagging non-standard or absent terms.
#[must_use]
pub fn license_description(license: &str) -> String {
    match license {
        "MIT" => "MIT license: a simple permissive license, free to use in closed or open source.".to_string(),
        "Apache-2.0" => "Apache-2.0 license: permissive and adds an explicit patent grant, friendly for commercial use.".to_string(),
        "Unlicense" => "Unlicense: a public-domain dedication, usable with no conditions.".to_string(),
        "custom" => "Non-standard license: review its exact terms before depending on it, as it is not a recognised SPDX license.".to_string(),
        "none" => "No license declared: reuse rights are legally unclear, so depending on it is risky until the author adds one.".to_string(),
        other => format!("Distributed under the {other} license."),
    }
}

/// Whether a license is a recognised, low-risk one. A non-standard ("custom")
/// or absent ("none") license is flagged as problematic.
#[must_use]
pub fn license_ok(license: &str) -> bool {
    !matches!(license, "custom" | "none" | "")
}

/// Comma-group an integer (e.g. `12345` to `12,345`). Local copy so this module
/// stays self-contained.
fn commas(n: u32) -> String {
    let s = n.to_string();
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}

/// Repository and crate facts for one parser.
pub struct ParserMeta {
    /// GitHub/Codeberg stars.
    pub stars: u32,
    /// Forks.
    pub forks: u32,
    /// Total commits on the default branch.
    pub commits: u32,
    /// Distinct contributors.
    pub contributors: u32,
    /// Year the project first appeared (repo creation or first crate release).
    pub since: u16,
    /// SPDX license id, or a short note where none is declared.
    pub license: &'static str,
    /// Whether the parser is fuzzed.
    pub fuzz: Fuzz,
    /// Whether the repository ships a test suite.
    pub tests: bool,
    /// Whether the repository ships criterion benchmarks.
    pub benches: bool,
    /// Whether the crate builds without the standard library (`no_std`).
    pub no_std: bool,
    /// Whether the crate compiles to the `wasm32-unknown-unknown` target.
    pub wasm: bool,
    /// Human-readable all-time crates.io downloads (empty when not on crates.io).
    pub downloads: &'static str,
    /// How often the crate publishes releases.
    pub cadence: Cadence,
    /// URL of the source repository.
    pub repo: &'static str,
    /// Whether the crate is published on crates.io (vs git-only / unreleased).
    pub crates_io: bool,
    /// Whether the crate is pure Rust (vs an FFI binding to a C library).
    pub pure_rust: bool,
    /// What the crate uses `unsafe` for, or `""` when it has no unsafe code.
    pub unsafe_note: &'static str,
    /// Month of the most recent release, `"YYYY-MM"`.
    pub last_release: &'static str,
    /// Sanitizer/Miri run in CI (short label, e.g. `"leak"`), or `""` for none.
    pub sanitizers: &'static str,
}

/// Releases at least this recent count as actively maintained. Roughly twelve
/// months before [`SNAPSHOT`]. `"YYYY-MM"` strings order by date.
const MAINTAINED_SINCE: &str = "2025-05";

/// Whether the crate's latest release is recent enough to look maintained.
#[must_use]
pub fn maintained(last_release: &str) -> bool {
    last_release >= MAINTAINED_SINCE
}

/// A full sentence describing whether the crate is pure Rust or a C binding.
#[must_use]
pub const fn pure_rust_description(pure: bool) -> &'static str {
    if pure {
        "Pure Rust: builds with cargo alone, needing no C toolchain or native library."
    } else {
        "Not pure Rust: it binds a C library through FFI, so it needs a C toolchain and cannot target every platform (no wasm, no no_std)."
    }
}

/// A full sentence describing how recently the crate was released.
#[must_use]
pub fn maintenance_description(last_release: &str) -> String {
    if maintained(last_release) {
        format!("Actively maintained: last released {last_release}, within the past year.")
    } else {
        format!("Possibly unmaintained: last released {last_release}, more than a year ago, so it may lag behind SQL changes and fixes.")
    }
}

/// A full sentence describing whether the project runs a sanitizer or Miri.
#[must_use]
pub fn sanitizer_description(sanitizers: &str) -> String {
    if sanitizers.is_empty() {
        "No sanitizer or Miri in CI, so memory errors, leaks, and undefined behavior are not caught automatically.".to_string()
    } else {
        format!("Runs the {sanitizers} sanitizer in CI, catching memory errors automatically on every change.")
    }
}

/// A full sentence describing whether the crate is published on crates.io, which
/// decides if it can be depended on with a plain version requirement.
#[must_use]
pub const fn crates_io_description(published: bool) -> &'static str {
    if published {
        "Published on crates.io, so it can be added with a normal version dependency and is indexed on docs.rs."
    } else {
        "Not on crates.io: it is only available as a git dependency, so it cannot be pinned to a published version."
    }
}

/// The hosting service a repository URL points at, for the source badge.
#[must_use]
pub fn repo_host(url: &str) -> &'static str {
    if url.contains("github.com") {
        "GitHub"
    } else if url.contains("gitlab.com") {
        "GitLab"
    } else if url.contains("bitbucket.org") {
        "Bitbucket"
    } else if url.contains("codeberg.org") {
        "Codeberg"
    } else {
        "git"
    }
}

/// Interpretive sentence for the source-repository badge.
#[must_use]
pub fn repo_description(url: &str) -> String {
    format!(
        "Source repository, hosted on {}. Opens {url} in a new tab.",
        repo_host(url)
    )
}

/// Metadata for a parser by display name, if recorded.
#[must_use]
pub fn parser_meta(name: &str) -> Option<ParserMeta> {
    Some(match name {
        "sqlparser-rs" => ParserMeta {
            stars: 3373,
            forks: 723,
            commits: 1958,
            contributors: 323,
            since: 2018,
            license: "Apache-2.0",
            fuzz: Fuzz::Yes,
            tests: true,
            benches: true,
            no_std: true,
            wasm: true,
            downloads: "63.2M",
            cadence: Cadence::Quarterly,
            repo: "https://github.com/sqlparser-rs/sqlparser-rs",
            crates_io: true,
            pure_rust: true,
            unsafe_note: "",
            last_release: "2026-05",
            sanitizers: "",
        },
        // Both libpg_query bindings: own crate has no harness, but libpg_query
        // (PostgreSQL's own C parser) is fuzzed upstream.
        "pg_query.rs" | "pg_query (summary)" => ParserMeta {
            stars: 237,
            forks: 26,
            commits: 105,
            contributors: 14,
            since: 2022,
            license: "MIT",
            fuzz: Fuzz::Upstream,
            tests: true,
            benches: true,
            no_std: false,
            wasm: false,
            downloads: "1.5M",
            cadence: Cadence::Quarterly,
            repo: "https://github.com/pganalyze/pg_query.rs",
            crates_io: true,
            pure_rust: false,
            unsafe_note: "the C FFI bindings to libpg_query",
            last_release: "2025-08",
            sanitizers: "leak",
        },
        "qusql-parse" => ParserMeta {
            stars: 17,
            forks: 0,
            commits: 808,
            contributors: 8,
            since: 2025,
            license: "Apache-2.0",
            fuzz: Fuzz::No,
            tests: true,
            benches: false,
            no_std: true,
            wasm: true,
            downloads: "2.5k",
            cadence: Cadence::Monthly,
            repo: "https://github.com/antialize/qusql",
            crates_io: true,
            pure_rust: true,
            unsafe_note: "one unchecked UTF-8 conversion in the lexer",
            last_release: "2026-05",
            sanitizers: "",
        },
        "polyglot-sql" => ParserMeta {
            stars: 829,
            forks: 47,
            commits: 133,
            contributors: 8,
            since: 2026,
            license: "MIT",
            fuzz: Fuzz::Yes,
            tests: true,
            benches: true,
            no_std: false,
            wasm: true,
            downloads: "8.3k",
            cadence: Cadence::Monthly,
            repo: "https://github.com/tobilg/polyglot",
            crates_io: true,
            pure_rust: true,
            unsafe_note: "",
            last_release: "2026-06",
            sanitizers: "",
        },
        "databend-common-ast" => ParserMeta {
            stars: 9308,
            forks: 877,
            commits: 34277,
            contributors: 252,
            since: 2020,
            license: "custom",
            fuzz: Fuzz::Yes,
            tests: true,
            benches: true,
            no_std: false,
            wasm: false,
            downloads: "26k",
            cadence: Cadence::Quarterly,
            repo: "https://github.com/datafuselabs/databend/tree/main/src/query/ast",
            crates_io: true,
            pure_rust: true,
            unsafe_note: "",
            last_release: "2026-03",
            sanitizers: "",
        },
        "sqlglot-rust" => ParserMeta {
            stars: 15,
            forks: 2,
            commits: 121,
            contributors: 2,
            since: 2026,
            license: "MIT",
            fuzz: Fuzz::No,
            tests: true,
            benches: true,
            no_std: false,
            wasm: true,
            downloads: "1.3k",
            cadence: Cadence::Monthly,
            repo: "https://github.com/protegrity/sql-glot-rust",
            crates_io: true,
            pure_rust: true,
            unsafe_note: "its C-ABI export layer",
            last_release: "2026-05",
            sanitizers: "",
        },
        "sqlite3-parser" => ParserMeta {
            stars: 62,
            forks: 16,
            commits: 500,
            contributors: 7,
            since: 2017,
            license: "Unlicense",
            fuzz: Fuzz::No,
            tests: true,
            benches: true,
            no_std: false,
            wasm: true,
            downloads: "3.3M",
            cadence: Cadence::Quarterly,
            repo: "https://github.com/gwenn/lemon-rs",
            crates_io: true,
            pure_rust: true,
            unsafe_note: "one unchecked UTF-8 conversion in keyword lookup",
            last_release: "2026-04",
            sanitizers: "",
        },
        "turso_parser" => ParserMeta {
            stars: 19043,
            forks: 996,
            commits: 17718,
            contributors: 269,
            since: 2023,
            license: "MIT",
            fuzz: Fuzz::No,
            tests: true,
            benches: true,
            no_std: false,
            wasm: true,
            downloads: "313k",
            cadence: Cadence::Monthly,
            repo: "https://github.com/tursodatabase/turso",
            crates_io: true,
            pure_rust: true,
            unsafe_note: "unchecked UTF-8 conversions in the lexer and parser",
            last_release: "2026-05",
            sanitizers: "",
        },
        "orql" => ParserMeta {
            stars: 0,
            forks: 1,
            commits: 90,
            contributors: 1,
            since: 2025,
            license: "none",
            fuzz: Fuzz::No,
            tests: true,
            benches: true,
            no_std: false,
            wasm: true,
            downloads: "18",
            cadence: Cadence::Irregular,
            repo: "https://codeberg.org/xitep/orql",
            crates_io: true,
            pure_rust: true,
            unsafe_note: "an unchecked UTF-8 conversion and a transmute",
            last_release: "2026-01",
            sanitizers: "",
        },
        _ => return None,
    })
}

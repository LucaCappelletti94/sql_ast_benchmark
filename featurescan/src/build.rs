//! Build-cost scan of the benchmarked SQL parsers.
//!
//! Compiles a minimal probe (read stdin, parse it, print the statement count)
//! from a clean target directory for every benchmarked parser release, the
//! current root-manifest releases and every time-machine milestone, and records
//! the build CPU time, wall time, compiled unit count, parser-crate-only
//! compile time, and stripped binary size over the no-parser baseline. Writes a
//! committed snapshot at `featurescan/data/build.json` that the web metadata
//! bakes in. Run with `cargo run -p featurescan --bin featurescan-build`.

use std::cmp::Ordering;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use cargo_metadata::semver::Version;
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand};
use viz::{BuildCost, BuildScan};

/// One compiler job, recorded in the snapshot because the wall times assume it.
const JOBS: usize = 8;

/// (display name, package name) of every current parser release; `pg_query`
/// lists twice, the parse and summary probes share the crate.
const CURRENT: &[(&str, &str)] = &[
    ("sqlparser-rs", "sqlparser"),
    ("pg_query.rs", "pg_query"),
    ("pg_query (summary)", "pg_query"),
    ("qusql-parse", "qusql-parse"),
    ("polyglot-sql", "polyglot-sql"),
    ("databend-common-ast", "databend-common-ast"),
    ("sqlglot-rust", "sqlglot-rust"),
    ("sqlite3-parser", "sqlite3-parser"),
    ("turso_parser", "turso_parser"),
];

/// The package names that are benchmarked parsers, so the time-machine
/// dependencies that are not (serde_json, zstd, ...) are skipped.
const PARSER_PACKAGES: &[&str] = &[
    "sqlparser",
    "pg_query",
    "qusql-parse",
    "polyglot-sql",
    "databend-common-ast",
    "sqlglot-rust",
    "sqlite3-parser",
    "turso_parser",
];

const NOTE: &str = "Clean `--release` builds of a minimal probe (read stdin, parse it, \
print the statement count), one per parser release, the current root-manifest releases \
plus every time-machine milestone. Each build starts from an empty target directory with \
the workspace `Cargo.lock` copied in and `cargo fetch`ed first, then runs `--frozen -j 8`, \
so the dependency versions match the benchmarked ones and no network or resolution \
enters the timed region. `cpu_secs` is the cutime plus cstime delta of the whole build \
tree, `wall_secs` the same region, `crate_secs` the parser lib unit duration and `units` \
the compiled unit count from cargo's `--timings` report (build-script execution units \
are excluded, they run a script rather than compile it), and `binary_bytes` the \
stripped probe size over the no-parser baseline. Regenerate with `cargo run -p \
featurescan --bin featurescan-build`.";

/// The probe `main`, shared by every probe source.
const PROBE_MAIN: &str = r##"fn main() {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).expect("read stdin");
    println!("{}", parse(&buf));
}
"##;

/// The probe prolog without extra dependencies.
const PROBE_PROLOG: &str = "use std::io::Read;\n";

/// The sqlite3 probes need the fallible-iterator trait in scope for `next()`.
const PROBE_PROLOG_FALLIBLE: &str =
    "use fallible_iterator::FallibleIterator as _;\nuse std::io::Read;\n";

/// sqlparser probe body, the generic dialect like the time machine's adapter.
const PARSE_SQLPARSER: &str = r##"fn parse(sql: &str) -> usize {
    match p::parser::Parser::parse_sql(&p::dialect::GenericDialect {}, &sql) {
        Ok(v) => v.len(),
        Err(e) => {
            eprintln!("sqlparser: {e}");
            std::process::exit(1);
        }
    }
}
"##;

/// pg_query probe body, the full parse entry point.
const PARSE_PG_QUERY: &str = r##"fn parse(sql: &str) -> usize {
    match p::parse(sql, p::ParserOptions::DEFAULT) {
        Ok(_) => 1,
        Err(e) => {
            eprintln!("pg_query: {e}");
            std::process::exit(1);
        }
    }
}
"##;

/// pg_query probe body, the summary entry point.
const PARSE_PG_QUERY_SUMMARY: &str = r##"fn parse(sql: &str) -> usize {
    match p::summary(sql, p::ParserOptions::DEFAULT, -1) {
        Ok(_) => 1,
        Err(e) => {
            eprintln!("pg_query summary: {e}");
            std::process::exit(1);
        }
    }
}
"##;

/// qusql-parse probe body, the resilient API counts the statements it parsed.
const PARSE_QUSQL: &str = r##"fn parse(sql: &str) -> usize {
    let opts = p::ParseOptions::new()
        .dialect(p::SQLDialect::PostgreSQL)
        .arguments(p::SQLArguments::Dollar);
    let mut issues = p::Issues::new(sql);
    p::parse_statements(sql, &mut issues, &opts).len()
}
"##;

/// polyglot-sql probe body.
const PARSE_POLYGLOT: &str = r##"fn parse(sql: &str) -> usize {
    match p::parse(&sql, p::DialectType::Generic) {
        Ok(v) => v.len(),
        Err(e) => {
            eprintln!("polyglot: {e}");
            std::process::exit(1);
        }
    }
}
"##;

/// databend-common-ast probe body, the single-statement tokenizer plus parser.
const PARSE_DATABEND: &str = r##"fn parse(sql: &str) -> usize {
    let tokens = match p::parser::tokenize_sql(sql) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("databend tokenize: {e}");
            std::process::exit(1);
        }
    };
    match p::parser::parse_sql(&tokens, p::parser::Dialect::PostgreSQL) {
        Ok(_) => 1,
        Err(e) => {
            eprintln!("databend parse: {e}");
            std::process::exit(1);
        }
    }
}
"##;

/// sqlglot-rust probe body.
const PARSE_SQLGLOT: &str = r##"fn parse(sql: &str) -> usize {
    match p::parser::parse_statements(&sql, p::Dialect::Ansi) {
        Ok(v) => v.len(),
        Err(e) => {
            eprintln!("sqlglot: {e}");
            std::process::exit(1);
        }
    }
}
"##;

/// sqlite3-parser probe body for releases before 0.17.0, the plain constructor.
const PARSE_SQLITE3: &str = r##"fn parse(sql: &str) -> usize {
    let input = sql.as_bytes();
    let mut parser = p::lexer::sql::Parser::new(input);
    let mut count = 0;
    loop {
        match parser.next() {
            Ok(Some(_)) => count += 1,
            Ok(None) => break count,
            Err(e) => {
                eprintln!("sqlite3: {e}");
                std::process::exit(1);
            }
        }
    }
}
"##;

/// sqlite3-parser probe body for 0.17.0 and later, which take a bump arena.
const PARSE_SQLITE3_ARENA: &str = r##"fn parse(sql: &str) -> usize {
    let input = sql.as_bytes();
    let arena = p::Bump::new();
    let mut parser = p::lexer::sql::Parser::new(&arena, input);
    let mut count = 0;
    loop {
        match parser.next() {
            Ok(Some(_)) => count += 1,
            Ok(None) => break count,
            Err(e) => {
                eprintln!("sqlite3: {e}");
                std::process::exit(1);
            }
        }
    }
}
"##;

/// turso_parser probe body, the streaming `next_cmd` loop.
const PARSE_TURSO: &str = r##"fn parse(sql: &str) -> usize {
    let mut parser = p::parser::Parser::new(sql.as_bytes());
    let mut count = 0;
    loop {
        match parser.next_cmd() {
            Ok(Some(_)) => count += 1,
            Ok(None) => break count,
            Err(e) => {
                eprintln!("turso: {e}");
                std::process::exit(1);
            }
        }
    }
}
"##;

/// The baseline probe, the same stdin read with no parser to measure against.
const BASELINE_PROBE: &str = r##"use std::io::Read;

fn parse(sql: &str) -> usize {
    sql.trim().lines().count()
}

fn main() {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).expect("read stdin");
    println!("{}", parse(&buf));
}
"##;

/// The baseline scratch manifest, the probe manifest without a parser dep.
const BASELINE_MANIFEST: &str =
    "[package]\nname = \"buildscan-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n\n[dependencies]\n\n[profile.release]\nstrip = true\n";

/// Where a release comes from, enough to rebuild its scratch dependency line.
enum ReleaseSource {
    Registry,
    Git { url: String, rev: String },
}

impl ReleaseSource {
    /// Parse the locked `source` string, `crates.io` or `git+URL#<commit>`.
    fn from_lock_source(source: &str) -> Self {
        match source.strip_prefix("git+") {
            Some(rest) => {
                let (url, rev) = rest
                    .rsplit_once('#')
                    .expect("a git lock source carries a commit");
                ReleaseSource::Git {
                    url: url.to_string(),
                    rev: rev.to_string(),
                }
            }
            None => ReleaseSource::Registry,
        }
    }

    /// `crates.io`, or the git URL and the first 7 hex digits of the commit.
    fn display(&self) -> String {
        match self {
            ReleaseSource::Registry => "crates.io".to_string(),
            ReleaseSource::Git { url, rev } => format!("{url}#{}", &rev[..7]),
        }
    }
}

/// One benchmarked parser release to measure.
struct Release {
    parser: String,
    package: String,
    version: String,
    source: ReleaseSource,
    current: bool,
}

/// The wall and CPU seconds of one clean release build.
struct BuildTimes {
    wall_secs: f64,
    cpu_secs: f64,
}

/// A `--only <package>[@<version>]` filter.
struct Only {
    package: String,
    version: Option<String>,
}

/// Split a package id into (source, name, version); the name is absent when a
/// path package's directory and package names agree.
fn id_parts(repr: &str) -> (&str, Option<&str>, &str) {
    let (source, tail) = repr
        .rsplit_once('#')
        .expect("a package id carries a source");
    match tail.rsplit_once('@') {
        Some((name, version)) => (source, Some(name), version),
        None => (source, None, tail),
    }
}

/// The locked `source` string for one (name, version); a git id matches by
/// prefix, since the lock appends the commit.
fn locked_source(doc: &toml::Value, name: &str, version: &str, id_source: &str) -> String {
    let packages = doc
        .get("package")
        .and_then(toml::Value::as_array)
        .expect("the lockfile has packages");
    for entry in packages {
        let matches_name = entry.get("name").and_then(toml::Value::as_str) == Some(name);
        let matches_version = entry.get("version").and_then(toml::Value::as_str) == Some(version);
        if !matches_name || !matches_version {
            continue;
        }
        let Some(source) = entry.get("source").and_then(toml::Value::as_str) else {
            continue;
        };
        let matches_source = if source == id_source {
            true
        } else if id_source.starts_with("git+") {
            source.starts_with(id_source)
        } else {
            false
        };
        if matches_source {
            return source.to_string();
        }
    }
    panic!("{name} {version} is not in Cargo.lock");
}

/// The display name of a parser release, matching the web metadata names.
fn display_name(package: &str) -> String {
    match package {
        "sqlparser" => "sqlparser-rs".to_string(),
        "pg_query" => "pg_query.rs".to_string(),
        other => other.to_string(),
    }
}

/// The root package's parser dependencies, resolved to the exact locked package.
fn current_releases(metadata: &Metadata, doc: &toml::Value) -> Vec<Release> {
    let resolve = metadata
        .resolve
        .as_ref()
        .expect("cargo metadata resolves the graph");
    let root_package = metadata
        .packages
        .iter()
        .find(|p| p.name == "sql_ast_benchmark")
        .expect("the root package is in the metadata");
    let root_node = resolve
        .nodes
        .iter()
        .find(|n| n.id.repr == root_package.id.repr)
        .expect("the root node is in the graph");
    let mut out = Vec::new();
    for (parser, package) in CURRENT {
        assert!(
            root_package
                .dependencies
                .iter()
                .any(|d| d.name == *package && d.kind == DependencyKind::Normal),
            "the root package does not depend on {package}"
        );
        let node_dep = root_node
            .deps
            .iter()
            .find(|d| id_parts(&d.pkg.repr).1 == Some(*package))
            .unwrap_or_else(|| panic!("{package} is not resolved"));
        let (id_source, _, version) = id_parts(&node_dep.pkg.repr);
        let source =
            ReleaseSource::from_lock_source(&locked_source(doc, package, version, id_source));
        out.push(Release {
            parser: (*parser).to_string(),
            package: (*package).to_string(),
            version: version.to_string(),
            source,
            current: true,
        });
    }
    out
}

/// The timemachine package's renamed dependencies (one pinned release per
/// alias), resolved to the exact locked package.
fn milestone_releases(metadata: &Metadata, doc: &toml::Value) -> Vec<Release> {
    let resolve = metadata
        .resolve
        .as_ref()
        .expect("cargo metadata resolves the graph");
    let tm_package = metadata
        .packages
        .iter()
        .find(|p| p.name == "timemachine")
        .expect("timemachine is in the metadata");
    let tm_node = resolve
        .nodes
        .iter()
        .find(|n| n.id.repr == tm_package.id.repr)
        .expect("the timemachine node is in the graph");
    let mut out = Vec::new();
    for dep in tm_package
        .dependencies
        .iter()
        .filter(|d| d.kind == DependencyKind::Normal && d.rename.is_some())
    {
        let alias = dep.rename.as_deref().expect("the dependency is renamed");
        let node_dep = tm_node
            .deps
            .iter()
            .find(|d| d.name == alias)
            .unwrap_or_else(|| panic!("{alias} is not resolved"));
        let (id_source, name, version) = id_parts(&node_dep.pkg.repr);
        let Some(name) = name else {
            continue;
        };
        if !PARSER_PACKAGES.contains(&name) {
            continue;
        }
        let source = ReleaseSource::from_lock_source(&locked_source(doc, name, version, id_source));
        out.push(Release {
            parser: display_name(name),
            package: name.to_string(),
            version: version.to_string(),
            source,
            current: false,
        });
    }
    out
}

/// Current releases first, then by parser and semver.
fn release_order(a: &Release, b: &Release) -> Ordering {
    (!a.current)
        .cmp(&!b.current)
        .then_with(|| a.parser.cmp(&b.parser))
        .then_with(|| version_key(&a.version).cmp(&version_key(&b.version)))
}

/// A comparable (major, minor, patch) key for a release version.
fn version_key(version: &str) -> (u64, u64, u64) {
    match Version::parse(version) {
        Ok(v) => (v.major, v.minor, v.patch),
        Err(_) => (0, 0, 0),
    }
}

/// 0.17.0 moved the sqlite3 AST into a caller-owned bump arena.
fn uses_arena(version: &str) -> bool {
    match Version::parse(version) {
        Ok(v) => v >= Version::new(0, 17, 0),
        Err(_) => false,
    }
}

/// The probe source for one release, mirroring the time-machine parse calls.
fn probe_source(release: &Release) -> String {
    let (prolog, parse_fn) = match release.package.as_str() {
        "sqlparser" => (PROBE_PROLOG, PARSE_SQLPARSER),
        "pg_query" if release.parser == "pg_query (summary)" => {
            (PROBE_PROLOG, PARSE_PG_QUERY_SUMMARY)
        }
        "pg_query" => (PROBE_PROLOG, PARSE_PG_QUERY),
        "qusql-parse" => (PROBE_PROLOG, PARSE_QUSQL),
        "polyglot-sql" => (PROBE_PROLOG, PARSE_POLYGLOT),
        "databend-common-ast" => (PROBE_PROLOG, PARSE_DATABEND),
        "sqlglot-rust" => (PROBE_PROLOG, PARSE_SQLGLOT),
        "sqlite3-parser" if uses_arena(&release.version) => {
            (PROBE_PROLOG_FALLIBLE, PARSE_SQLITE3_ARENA)
        }
        "sqlite3-parser" => (PROBE_PROLOG_FALLIBLE, PARSE_SQLITE3),
        "turso_parser" => (PROBE_PROLOG, PARSE_TURSO),
        _ => panic!("no probe for the parser package {}", release.package),
    };
    format!("{prolog}\n{parse_fn}\n{PROBE_MAIN}")
}

/// The scratch manifest, the parser pinned to the exact locked version or commit.
fn probe_manifest(release: &Release) -> String {
    let dep = match &release.source {
        ReleaseSource::Registry => {
            format!(
                "p = {{ package = \"{}\", version = \"={}\" }}",
                release.package, release.version
            )
        }
        ReleaseSource::Git { url, rev } => {
            format!(
                "p = {{ package = \"{}\", git = \"{}\", rev = \"{}\" }}",
                release.package, url, rev
            )
        }
    };
    let fallible = if release.package == "sqlite3-parser" {
        "fallible-iterator = \"0.3\"\n"
    } else {
        ""
    };
    format!(
        "[package]\nname = \"buildscan-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n\n[dependencies]\n{dep}\n{fallible}\n[profile.release]\nstrip = true\n"
    )
}

/// The scratch directory of one release, under the workspace `target/`.
fn scratch_dir(scan_root: &Path, release: &Release) -> PathBuf {
    let suffix = if matches!(release.source, ReleaseSource::Git { .. }) {
        "-git"
    } else {
        ""
    };
    scan_root.join(format!("{}-{}{suffix}", release.package, release.version))
}

/// Write the scratch crate (manifest, probe, workspace lock) and clear the target.
fn write_scratch(dir: &Path, manifest: &str, probe: &str, lock: &Path) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create the scratch src dir");
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("write the scratch manifest");
    std::fs::write(src.join("main.rs"), probe).expect("write the scratch probe");
    std::fs::copy(lock, dir.join("Cargo.lock")).expect("copy the workspace Cargo.lock");
    std::fs::remove_dir_all(dir.join("target")).ok();
}

/// Download every locked dependency before the timed region.
fn cargo_fetch(dir: &Path) -> Result<(), String> {
    let output = Command::new("cargo")
        .arg("fetch")
        .current_dir(dir)
        .output()
        .map_err(|e| format!("cargo fetch failed to start: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo fetch exited with {:?}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// The `cutime` and `cstime` fields of `/proc/self/stat`, in clock ticks, read
/// after the last `)` because the command field may hold spaces. They
/// accumulate every waited-on child's CPU time.
fn self_ticks() -> (u64, u64) {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("read /proc/self/stat");
    let rest = &stat[stat.rfind(')').expect("stat has a command field") + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // `rest` starts at field 3 (state), so `cutime` (16) and `cstime` (17)
    // are at indexes 13 and 14.
    (
        fields[13].parse().expect("cutime is numeric"),
        fields[14].parse().expect("cstime is numeric"),
    )
}

/// Run one clean `cargo build --release` in `dir` and time it, the scratch
/// `target/` removed first so `--timings` writes a fresh unit report.
fn clean_build(dir: &Path, clk_tck: f64) -> Result<BuildTimes, String> {
    let target = dir.join("target");
    std::fs::remove_dir_all(&target).ok();
    let before = self_ticks();
    let start = Instant::now();
    let output = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--frozen")
        .arg("-j")
        .arg(JOBS.to_string())
        .arg("--timings")
        // The user-level cargo config enables incremental builds, which a clean release build must not pick up.
        .arg("--config")
        .arg("build.incremental=false")
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("RUSTC_WRAPPER")
        .env_remove("CARGO_BUILD_RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_INCREMENTAL")
        .output()
        .map_err(|e| format!("cargo build failed to start: {e}"))?;
    let wall_secs = start.elapsed().as_secs_f64();
    let after = self_ticks();
    if !output.status.success() {
        return Err(format!(
            "cargo build exited with {:?}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    // Clock ticks to seconds, a deliberately lossy float here.
    let ticks = after
        .0
        .saturating_add(after.1)
        .saturating_sub(before.0.saturating_add(before.1));
    Ok(BuildTimes {
        wall_secs,
        cpu_secs: ticks as f64 / clk_tck,
    })
}

/// Read the units the `--timings` report embeds as `const UNIT_DATA = [...]`.
fn unit_data(html: &str) -> Option<Vec<(String, String, f64)>> {
    const MARKER: &str = "const UNIT_DATA = ";
    let start = html.find(MARKER)?.checked_add(MARKER.len())?;
    let end = html[start..].find(";\n")?.checked_add(start)?;
    let value: serde_json::Value = serde_json::from_str(&html[start..end]).ok()?;
    let mut out = Vec::new();
    for unit in value.as_array()? {
        let name = unit.get("name")?.as_str()?.to_string();
        let target = unit.get("target")?.as_str()?.to_string();
        let duration = unit.get("duration")?.as_f64()?;
        out.push((name, target, duration));
    }
    Some(out)
}

/// The parser lib unit duration and the compiled unit count from the report,
/// excluding build-script execution units, which run a script rather than compile.
fn timing_stats(report: &Path, package: &str) -> (Option<f64>, usize) {
    let Ok(html) = std::fs::read_to_string(report) else {
        return (None, 0);
    };
    let Some(units) = unit_data(&html) else {
        return (None, 0);
    };
    let mut crate_secs = None;
    let mut count = 0;
    for (name, target, duration) in units {
        let label = target.trim();
        if label == "build-script (run)" {
            continue;
        }
        count += 1;
        // The lib unit's target label is empty, the package name identifies it.
        if name == package && label.is_empty() && crate_secs.is_none() {
            crate_secs = Some(duration);
        }
    }
    (crate_secs, count)
}

/// Run the built probe on `SELECT 1` and require a clean exit.
fn smoke(bin: &Path) -> Result<(), String> {
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("the smoke run failed to start: {e}"))?;
    let mut stdin = child.stdin.take().expect("the stdin pipe is open");
    stdin
        .write_all(b"SELECT 1\n")
        .expect("write the smoke input");
    // Closing stdin sends the probe its EOF.
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|e| format!("the smoke run failed to finish: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "the smoke run exited with {:?}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Build one release's scratch crate, measure it, smoke it, and clear the target.
fn measure_release(
    dir: &Path,
    release: &Release,
    baseline_bytes: u64,
    clk_tck: f64,
) -> Result<BuildCost, String> {
    let times = clean_build(dir, clk_tck)?;
    let bin = dir.join("target/release/buildscan-probe");
    let size = std::fs::metadata(&bin)
        .map_err(|e| format!("no release binary: {e}"))?
        .len();
    let (crate_secs, units) = timing_stats(
        &dir.join("target/cargo-timings/cargo-timing.html"),
        &release.package,
    );
    smoke(&bin)?;
    std::fs::remove_dir_all(dir.join("target")).ok();
    Ok(BuildCost {
        parser: release.parser.clone(),
        package: release.package.clone(),
        version: release.version.clone(),
        source: release.source.display(),
        current: release.current,
        cpu_secs: times.cpu_secs,
        wall_secs: times.wall_secs,
        crate_secs,
        units,
        binary_bytes: size.saturating_sub(baseline_bytes),
    })
}

/// A one-line identity for logs, checkpoint reuse, and failure reports.
fn release_key(release: &Release) -> String {
    format!(
        "{} {} {} {} current={}",
        release.parser,
        release.package,
        release.version,
        release.source.display(),
        release.current
    )
}

/// Whether a checkpointed entry is the same release as one being planned.
fn cost_matches(cost: &BuildCost, release: &Release) -> bool {
    cost.parser == release.parser
        && cost.package == release.package
        && cost.version == release.version
        && cost.source == release.source.display()
        && cost.current == release.current
}

/// Load the checkpoint, the `BuildCost` lines written as releases finish.
fn load_checkpoint(path: &Path) -> Vec<BuildCost> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Append one finished `BuildCost` to the checkpoint.
fn append_checkpoint(path: &Path, cost: &BuildCost) {
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        eprintln!("warning: cannot append to the checkpoint {path:?}");
        return;
    };
    let line = serde_json::to_string(cost).expect("serialize the entry");
    file.write_all(format!("{line}\n").as_bytes())
        .expect("append the checkpoint line");
}

/// The clock tick rate of this machine, run once at startup.
fn clk_tck() -> f64 {
    let output = Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .expect("getconf CLK_TCK");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("CLK_TCK is numeric")
}

/// The `rustc -V` string, so the snapshot records the toolchain.
fn rustc_version() -> String {
    let output = Command::new("rustc").arg("-V").output().expect("rustc -V");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<String>>();
    let mut fresh = false;
    let mut only: Option<Only> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--fresh" {
            fresh = true;
        } else if arg == "--only" {
            i += 1;
            let Some(spec) = args.get(i) else {
                eprintln!("--only needs <package>[@<version>]");
                std::process::exit(2);
            };
            let (package, version) = spec
                .split_once('@')
                .map(|(p, v)| (p.to_string(), Some(v.to_string())))
                .unwrap_or_else(|| (spec.clone(), None));
            only = Some(Only { package, version });
        } else {
            eprintln!("unknown flag {arg}");
            std::process::exit(2);
        }
        i += 1;
    }

    let metadata = MetadataCommand::new()
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
        .exec()
        .expect("cargo metadata failed");
    let lock_path = metadata
        .workspace_root
        .join("Cargo.lock")
        .as_std_path()
        .to_path_buf();
    let lock_doc: toml::Value =
        toml::from_str(&std::fs::read_to_string(&lock_path).expect("read Cargo.lock"))
            .expect("parse Cargo.lock");

    let mut planned = current_releases(&metadata, &lock_doc);
    planned.extend(milestone_releases(&metadata, &lock_doc));
    if let Some(only) = &only {
        planned.retain(|r| {
            r.package == only.package && only.version.as_deref().is_none_or(|v| r.version == v)
        });
    }
    if planned.is_empty() {
        eprintln!("no releases match the --only filter");
        std::process::exit(1);
    }
    planned.sort_by(release_order);

    let scan_root = metadata.target_directory.as_std_path().join("buildscan");
    std::fs::create_dir_all(&scan_root).expect("create the buildscan dir");
    let checkpoint = scan_root.join("results.jsonl");
    if fresh {
        std::fs::remove_file(&checkpoint).ok();
    }
    let mut stored = load_checkpoint(&checkpoint);

    let clk_tck = clk_tck();
    let rustc = rustc_version();
    eprintln!(
        "toolchain {rustc}, CLK_TCK {clk_tck}, planning {} releases ({} reused)",
        planned.len(),
        planned
            .iter()
            .filter(|r| stored.iter().any(|c| cost_matches(c, r)))
            .count()
    );

    // The baseline is the no-parser probe built the same way, its stripped
    // size subtracted from every binary.
    let baseline_dir = scan_root.join("baseline");
    write_scratch(&baseline_dir, BASELINE_MANIFEST, BASELINE_PROBE, &lock_path);
    cargo_fetch(&baseline_dir).expect("the baseline cargo fetch failed");
    clean_build(&baseline_dir, clk_tck).expect("the baseline build failed");
    let baseline_bytes = std::fs::metadata(baseline_dir.join("target/release/buildscan-probe"))
        .expect("the baseline binary exists")
        .len();
    std::fs::remove_dir_all(baseline_dir.join("target")).ok();
    eprintln!("baseline probe is {baseline_bytes} bytes stripped");

    let mut entries: Vec<BuildCost> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for release in &planned {
        if let Some(pos) = stored.iter().position(|c| cost_matches(c, release)) {
            eprintln!("reuse {}", release_key(release));
            entries.push(stored.remove(pos));
            continue;
        }
        let dir = scratch_dir(&scan_root, release);
        write_scratch(
            &dir,
            &probe_manifest(release),
            &probe_source(release),
            &lock_path,
        );
        let result =
            cargo_fetch(&dir).and_then(|_| measure_release(&dir, release, baseline_bytes, clk_tck));
        match result {
            Ok(cost) => {
                eprintln!(
                    "built {} (cpu {:.1}s, wall {:.1}s, binary {} bytes)",
                    release_key(release),
                    cost.cpu_secs,
                    cost.wall_secs,
                    cost.binary_bytes
                );
                append_checkpoint(&checkpoint, &cost);
                entries.push(cost);
            }
            Err(e) => failures.push(format!("{}: {e}", release_key(release))),
        }
    }

    if only.is_some() {
        println!(
            "{}",
            serde_json::to_string_pretty(&entries).expect("serialize the entries")
        );
    } else {
        let scan = BuildScan {
            note: NOTE.to_string(),
            rustc,
            jobs: JOBS,
            baseline_bytes,
            entries,
        };
        let out_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/build.json");
        let json = serde_json::to_string_pretty(&scan).expect("serialize the snapshot");
        std::fs::write(&out_path, json).expect("write the snapshot");
        eprintln!("wrote {}", out_path.display());
    }

    if !failures.is_empty() {
        eprintln!("failed releases");
        for failure in &failures {
            eprintln!("  {failure}");
        }
        std::process::exit(1);
    }
}

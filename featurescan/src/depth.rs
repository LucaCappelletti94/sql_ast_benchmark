//! Recursion-depth probe: how deep can each parser nest before it stops, and how
//! does it stop, a clean recursion-limit error or a hard stack overflow.
//!
//! A stack overflow aborts the whole process and is uncatchable (even on a worker
//! thread and even by `catch_unwind`), so the depth where a parser overflows
//! cannot be found safely in-process. Instead each (parser, depth) trial runs in a
//! CHILD PROCESS: if the child exits with a status code it handled that depth
//! (accepted, rejected, or a caught panic). If it is killed by a signal it
//! overflowed. The parent binary-searches two boundaries per parser: the graceful
//! limit (smallest depth the parser rejects instead of accepting) and the crash
//! depth (smallest depth that overflows). A parser that rejects deep input with a
//! clean error and never crashes is "depth-guarded".
//!
//! The probe shape is nested parentheses around a literal (`SELECT (((1)))`),
//! valid SQL at any depth and the classic stack-overflow trigger for
//! recursive-descent parsers. The child worker uses a fixed 8 MiB stack (a typical
//! default), so the crash depths are comparable and reflect a normal environment,
//! not the 512 MiB grading threads.

use std::process::{Command, Stdio};

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{BenchParser, ParseOutcome};
use viz::{DepthReport, DepthScan};

/// Worker stack for the child trial. Fixed so crash depths are comparable.
const PROBE_STACK: usize = 8 * 1024 * 1024;
/// Highest depth tried. Above any recursion limit, below run-away string sizes.
const CEIL: usize = 50_000;
/// Env var carrying the child trial spec: "<parser_index>|<depth>".
const CHILD_ENV: &str = "FEATURESCAN_DEPTH_CHILD";

/// Outcome of one depth trial.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Trial {
    Accepted,
    Rejected,
    Panicked,
    Unsupported,
    /// The child died (stack overflow / abort): this depth is not survivable.
    Crash,
}

/// Representative dialect to probe each parser in (its primary home dialect).
fn rep_dialect(p: BenchParser) -> Dialect {
    match p {
        BenchParser::Sqlite3 | BenchParser::Turso => Dialect::Sqlite,
        BenchParser::Orql => Dialect::Oracle,
        _ => Dialect::Postgresql,
    }
}

/// Nested-parens statement at the given depth: `SELECT ((( 1 )))`.
fn nested_sql(depth: usize) -> String {
    let mut s = String::with_capacity(depth * 2 + 16);
    s.push_str("SELECT ");
    for _ in 0..depth {
        s.push('(');
    }
    s.push('1');
    for _ in 0..depth {
        s.push(')');
    }
    s
}

fn main() {
    if let Ok(spec) = std::env::var(CHILD_ENV) {
        run_child(&spec);
        return;
    }
    run_parent();
}

/// Child: parse one nested statement on a bounded stack and exit with a code that
/// encodes the outcome. A stack overflow aborts the process before we exit, which
/// the parent reads as a crash.
fn run_child(spec: &str) {
    // Caught panics are expected here, so silence the hook so the parent's captured
    // stderr stays clean.
    std::panic::set_hook(Box::new(|_| {}));
    let mut parts = spec.split('|');
    let idx: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let depth: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let parser = BenchParser::all()[idx];
    let dialect = rep_dialect(parser);
    let sql = nested_sql(depth);

    let outcome = std::thread::Builder::new()
        .stack_size(PROBE_STACK)
        .spawn(move || parser.parse_outcome(&sql, dialect))
        .expect("spawn probe thread")
        .join()
        .expect("probe thread");

    let code = match outcome {
        ParseOutcome::Accepted => 0,
        ParseOutcome::Rejected(_) => 1,
        ParseOutcome::Panicked(_) => 2,
        ParseOutcome::Unsupported => 3,
    };
    std::process::exit(code);
}

/// Parent: probe every parser and write the committed depth snapshot.
fn run_parent() {
    let exe = std::env::current_exe().expect("current exe");
    let parsers = BenchParser::all();

    let mut reports = Vec::new();
    for (idx, &p) in parsers.iter().enumerate() {
        // Cache trials so the two binary searches share results.
        let mut cache: std::collections::HashMap<usize, Trial> = std::collections::HashMap::new();
        let mut trial = |depth: usize| -> Trial {
            if let Some(&t) = cache.get(&depth) {
                return t;
            }
            let t = probe(&exe, idx, depth);
            cache.insert(depth, t);
            t
        };

        let report = analyze(p, &mut trial);
        eprintln!("{:22} {}", p.name(), describe(&report));
        reports.push(report);
    }

    let snapshot = DepthScan {
        note: format!(
            "Recursion-depth probe (nested parens, {} MiB worker stack, ceiling {CEIL}). \
             Each (parser, depth) trial runs in a child process. A clean exit means \
             the depth was handled, a signal kill means a stack overflow. \
             `crash_depth` is the smallest overflowing depth (null = never crashed up \
             to the ceiling). `limit_depth` is the smallest depth the parser rejects \
             instead of accepting (its graceful recursion limit, null = accepts up to \
             the boundary). `guarded` = it rejects deep input cleanly and never \
             crashes. Regenerate with `cargo run -p featurescan --bin featurescan-depth`.",
            PROBE_STACK / (1024 * 1024)
        ),
        stack_bytes: PROBE_STACK,
        ceil: CEIL,
        parsers: reports,
    };

    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    std::fs::create_dir_all(&out_dir).expect("create data dir");
    let out_path = out_dir.join("depth.json");
    let json = serde_json::to_string_pretty(&snapshot).expect("serialize");
    std::fs::write(&out_path, json).expect("write snapshot");
    eprintln!("wrote {}", out_path.display());
}

/// Run one child trial and classify its exit status.
fn probe(exe: &std::path::Path, parser_idx: usize, depth: usize) -> Trial {
    let status = Command::new(exe)
        .env(CHILD_ENV, format!("{parser_idx}|{depth}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn child");
    match status.code() {
        Some(0) => Trial::Accepted,
        Some(1) => Trial::Rejected,
        Some(2) => Trial::Panicked,
        Some(3) => Trial::Unsupported,
        // None = killed by signal (overflow/abort). Any other code is treated as a crash.
        _ => Trial::Crash,
    }
}

/// Smallest depth in `(lo, hi]` where `pred` holds, assuming `pred` is monotonic
/// (once true it stays true) with `!pred(lo)` and `pred(hi)`.
fn boundary(mut lo: usize, mut hi: usize, mut pred: impl FnMut(usize) -> bool) -> usize {
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if pred(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// Find the graceful limit and crash depth for one parser.
fn analyze(p: BenchParser, trial: &mut impl FnMut(usize) -> Trial) -> DepthReport {
    let dialect = rep_dialect(p);
    let top = trial(CEIL);

    // Crash depth: smallest depth that overflows, if any up to the ceiling.
    let crash_depth = if top == Trial::Crash {
        Some(boundary(1, CEIL, |d| trial(d) == Trial::Crash))
    } else {
        None
    };

    // If the parser rejects even shallow nesting, it does not accept this probe
    // shape at all, so its graceful recursion limit cannot be read from this shape
    // (the rejection is syntactic, not a depth guard). The crash depth is still
    // meaningful: the parser recurses through the nesting before failing.
    let shape_rejected = trial(1) != Trial::Accepted;

    // Search the survivable range for the graceful limit (first non-accept).
    let safe_top = crash_depth.map_or(CEIL, |c| c - 1);
    let limit_depth = if shape_rejected {
        None
    } else if safe_top >= 2 && trial(safe_top) != Trial::Accepted {
        Some(boundary(1, safe_top, |d| trial(d) != Trial::Accepted))
    } else {
        None
    };

    DepthReport {
        parser: p.name().to_string(),
        dialect: dialect.dir_name().to_string(),
        guarded: crash_depth.is_none(),
        shape_rejected,
        limit_depth,
        crash_depth,
        ceil: CEIL,
    }
}

fn describe(r: &DepthReport) -> String {
    let limit = if r.shape_rejected {
        "shape n/a".to_string()
    } else {
        r.limit_depth.map_or("none".to_string(), |d| d.to_string())
    };
    match r.crash_depth {
        Some(c) => format!("CRASHES at depth {c} (limit: {limit})"),
        None => format!("guarded (limit {limit}, no crash up to {})", r.ceil),
    }
}

#![allow(clippy::doc_markdown, clippy::print_literal, clippy::too_many_lines)]

//! Multi-dialect correctness benchmark over the `datasets/` corpus.
//!
//! Ground truth ("correct") is defined per dialect:
//!
//!   * Oracle-backed dialects — PostgreSQL (pg_query / libpg_query) and SQLite
//!     (sqlite3-parser / lemon-rs, the real SQLite grammar). The oracle splits
//!     the corpus into valid (oracle accepts) and invalid (oracle rejects), and
//!     every parser is graded on four metrics: Recall (of valid stmts, how many
//!     the parser accepts, up), False positives (of invalid stmts, how many it
//!     wrongly accepts, down), Round-trip (of accepted-valid, parse-print-
//!     reparse stable, up) and Fidelity (of accepted-valid, oracle-canonical
//!     preserved, up).
//!
//!   * Other dialects have no oracle. Statements come from that dialect's own
//!     test suites / official samples and are treated as provenance-valid, so
//!     the metric is acceptance rate (+ round-trip of accepted).
//!
//! Each parser runs in its best-matching dialect; parsers that do not model a
//! dialect are shown as N/A.
//!
//!   cargo run --release --bin correctness

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{has_oracle, oracle_accepts, BenchParser};
use std::fs;
use std::path::Path;

/// Large worker stack: deeply nested SQL overflows recursive-descent parsers
/// and stack overflow aborts the process (uncatchable), so give headroom.
const WORKER_STACK: usize = 512 * 1024 * 1024;

/// Per-parser aggregate counts within a dialect.
#[derive(Clone, Default)]
struct Agg {
    /// has a pretty-printer (round-trip / fidelity applicable)?
    can_reprint: bool,
    /// accepted among oracle-valid (recall numerator), or among all (provenance)
    accepted_valid: usize,
    /// accepted among oracle-invalid (false-positive numerator)
    accepted_invalid: usize,
    /// round-trip stable among accepted-valid
    roundtrip_ok: usize,
    /// oracle-fidelity ok among accepted-valid
    fidelity_ok: usize,
}

impl Agg {
    const fn merge(&mut self, o: &Self) {
        self.accepted_valid += o.accepted_valid;
        self.accepted_invalid += o.accepted_invalid;
        self.roundtrip_ok += o.roundtrip_ok;
        self.fidelity_ok += o.fidelity_ok;
    }
}

struct DialectResult {
    dialect: Dialect,
    has_oracle: bool,
    valid_total: usize,
    invalid_total: usize,
    parsers: Vec<BenchParser>,
    aggs: Vec<Agg>,
}

fn pct(n: usize, base: usize) -> f64 {
    if base == 0 {
        0.0
    } else {
        100.0 * n as f64 / base as f64
    }
}

/// Load all non-empty statements for a dialect from its `datasets/` subdir.
fn load_dialect(dialect: Dialect) -> Vec<String> {
    let dir = Path::new("datasets").join(dialect.dir_name());
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    let mut stmts = Vec::new();
    for f in files {
        if let Ok(content) = fs::read_to_string(&f) {
            stmts.extend(
                content
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(String::from),
            );
        }
    }
    stmts
}

fn fresh_aggs(parsers: &[BenchParser], dialect: Dialect) -> Vec<Agg> {
    parsers
        .iter()
        .map(|p| Agg {
            can_reprint: p.can_reprint(dialect),
            ..Agg::default()
        })
        .collect()
}

/// Grade one chunk of statements for a dialect, returning per-parser aggregates
/// plus (valid_total, invalid_total) within the chunk.
fn grade_chunk(
    stmts: &[String],
    dialect: Dialect,
    parsers: &[BenchParser],
) -> (Vec<Agg>, usize, usize) {
    let oracle = has_oracle(dialect);
    let mut aggs = fresh_aggs(parsers, dialect);
    let mut valid_total = 0usize;
    let mut invalid_total = 0usize;

    for sql in stmts {
        // Oracle split (provenance dialects treat every stmt as "valid").
        let is_valid = if oracle {
            oracle_accepts(sql, dialect) == Some(true)
        } else {
            true
        };
        if is_valid {
            valid_total += 1;
        } else {
            invalid_total += 1;
        }

        for (i, &p) in parsers.iter().enumerate() {
            if p.accepts(sql, dialect) != Some(true) {
                continue;
            }
            if is_valid {
                aggs[i].accepted_valid += 1;
                if aggs[i].can_reprint {
                    if p.roundtrips(sql, dialect) == Some(true) {
                        aggs[i].roundtrip_ok += 1;
                    }
                    if p.fidelity(sql, dialect) == Some(true) {
                        aggs[i].fidelity_ok += 1;
                    }
                }
            } else {
                aggs[i].accepted_invalid += 1;
            }
        }
    }
    (aggs, valid_total, invalid_total)
}

fn process_dialect(dialect: Dialect, all_parsers: &[BenchParser]) -> Option<DialectResult> {
    let stmts = load_dialect(dialect);
    if stmts.is_empty() {
        return None;
    }
    let parsers: Vec<BenchParser> = all_parsers
        .iter()
        .copied()
        .filter(|p| p.supports(dialect))
        .collect();

    let n_threads = std::thread::available_parallelism()
        .map_or(8, std::num::NonZeroUsize::get)
        .min(32);
    let chunk = stmts.len().div_ceil(n_threads).max(1);

    let (aggs, valid_total, invalid_total) = std::thread::scope(|scope| {
        let handles: Vec<_> = stmts
            .chunks(chunk)
            .map(|c| {
                let parsers = &parsers;
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, move || grade_chunk(c, dialect, parsers))
                    .expect("spawn worker")
            })
            .collect();

        let mut merged = fresh_aggs(&parsers, dialect);
        let mut vt = 0usize;
        let mut it = 0usize;
        for h in handles {
            let (part, pv, pi) = h.join().expect("grade thread panicked");
            for (m, o) in merged.iter_mut().zip(part.iter()) {
                m.merge(o);
            }
            vt += pv;
            it += pi;
        }
        (merged, vt, it)
    });

    Some(DialectResult {
        dialect,
        has_oracle: has_oracle(dialect),
        valid_total,
        invalid_total,
        parsers,
        aggs,
    })
}

fn cell(v: f64) -> String {
    format!("{v:>6.1}%")
}
const NA: &str = "   N/A";

fn print_result(r: &DialectResult) {
    println!("\n═══ {} ═══", r.dialect.dir_name());
    let nw = r
        .parsers
        .iter()
        .map(|p| p.name().len())
        .max()
        .unwrap_or(22)
        .max(22);

    if r.has_oracle {
        let oracle_name = if r.dialect == Dialect::Sqlite {
            "sqlite3-parser (lemon-rs)"
        } else {
            "pg_query (libpg_query)"
        };
        println!(
            "Oracle: {}   valid: {}   invalid: {}",
            oracle_name, r.valid_total, r.invalid_total
        );
        println!(
            "{:<nw$}  {:>7}  {:>7}  {:>7}  {:>8}",
            "parser",
            "Recall",
            "FalseP",
            "RTrip",
            "Fidelity",
            nw = nw
        );
        println!("{}", "─".repeat(nw + 2 + 7 + 2 + 7 + 2 + 7 + 2 + 8));
        for (p, a) in r.parsers.iter().zip(r.aggs.iter()) {
            let recall = cell(pct(a.accepted_valid, r.valid_total));
            let fp = if r.invalid_total > 0 {
                cell(pct(a.accepted_invalid, r.invalid_total))
            } else {
                NA.to_string()
            };
            let rt = if a.can_reprint {
                cell(pct(a.roundtrip_ok, a.accepted_valid))
            } else {
                NA.to_string()
            };
            let fid = if a.can_reprint {
                cell(pct(a.fidelity_ok, a.accepted_valid))
            } else {
                NA.to_string()
            };
            println!(
                "{:<nw$}  {:>7}  {:>7}  {:>7}  {:>8}",
                p.name(),
                recall,
                fp,
                rt,
                fid,
                nw = nw
            );
        }
    } else {
        println!(
            "No oracle (provenance corpus). Total statements: {}",
            r.valid_total
        );
        println!(
            "{:<nw$}  {:>8}  {:>7}",
            "parser",
            "Accept",
            "RTrip",
            nw = nw
        );
        println!("{}", "─".repeat(nw + 2 + 8 + 2 + 7));
        for (p, a) in r.parsers.iter().zip(r.aggs.iter()) {
            let acc = cell(pct(a.accepted_valid, r.valid_total));
            let rt = if a.can_reprint {
                cell(pct(a.roundtrip_ok, a.accepted_valid))
            } else {
                NA.to_string()
            };
            println!("{:<nw$}  {:>8}  {:>7}", p.name(), acc, rt, nw = nw);
        }
    }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));

    if !Path::new("datasets").exists() {
        eprintln!("ERROR: `datasets/` directory not found. Run download_datasets first.");
        std::process::exit(1);
    }

    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  Multi-dialect SQL parser correctness                                ║");
    println!("║  Oracle-graded (PostgreSQL=pg_query, SQLite=lemon-rs); acceptance-    ║");
    println!("║  rate elsewhere. Each parser run in its best-matching dialect.       ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let all_parsers = BenchParser::all();

    // Oracle-backed dialects first, then the rest.
    let order = [
        Dialect::Postgresql,
        Dialect::Sqlite,
        Dialect::Mysql,
        Dialect::Clickhouse,
        Dialect::Duckdb,
        Dialect::Hive,
        Dialect::SparkSql,
        Dialect::Trino,
        Dialect::Tsql,
        Dialect::Oracle,
        Dialect::Bigquery,
        Dialect::Redshift,
        Dialect::Multi,
    ];

    for dialect in order {
        eprintln!("processing {} …", dialect.dir_name());
        if let Some(r) = process_dialect(dialect, &all_parsers) {
            print_result(&r);
        }
    }
    println!();
}

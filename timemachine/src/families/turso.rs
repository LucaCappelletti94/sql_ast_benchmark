//! turso_parser (the SQLite parser from Turso). SQLite only, a streaming
//! `next_cmd` loop; reprints via each command's `Display`. Only one stable
//! release is published, so the history is a single point.

use sql_ast_benchmark::datasets::Dialect;
use sql_ast_benchmark::{Parser, ParserId};

macro_rules! turso_version {
    ($name:ident, $cr:ident, $ver:literal, $released:literal) => {
        pub struct $name;

        impl Parser for $name {
            fn id(&self) -> ParserId {
                ParserId {
                    family: "turso_parser",
                    version: $ver,
                    released: $released,
                }
            }

            fn supports(&self, dialect: Dialect) -> bool {
                dialect == Dialect::Sqlite
            }

            fn try_parse(&self, sql: &str, dialect: Dialect) -> Option<Result<(), String>> {
                if dialect != Dialect::Sqlite {
                    return None;
                }
                Some(
                    std::panic::catch_unwind(|| {
                        let mut parser = $cr::parser::Parser::new(sql.as_bytes());
                        loop {
                            match parser.next_cmd() {
                                Ok(Some(_)) => {}
                                Ok(None) => return Ok(()),
                                Err(e) => return Err(e.to_string()),
                            }
                        }
                    })
                    .unwrap_or_else(|_| Err("panicked".to_string())),
                )
            }

            fn parse_once(&self, sql: &str, dialect: Dialect) -> bool {
                if dialect != Dialect::Sqlite {
                    return false;
                }
                let mut parser = $cr::parser::Parser::new(sql.as_bytes());
                loop {
                    match parser.next_cmd() {
                        Ok(Some(_)) => {}
                        Ok(None) => break true,
                        Err(_) => break false,
                    }
                }
            }

            fn parse_batch(&self, sql: &str, dialect: Dialect) -> Option<usize> {
                if dialect != Dialect::Sqlite {
                    return None;
                }
                let mut parser = $cr::parser::Parser::new(sql.as_bytes());
                let mut n = 0;
                loop {
                    match parser.next_cmd() {
                        Ok(Some(_)) => n += 1,
                        Ok(None) | Err(_) => break Some(n),
                    }
                }
            }

            fn can_batch(&self) -> bool {
                true
            }

            fn measure_mem(&self, sql: &str, dialect: Dialect) -> Option<(usize, usize)> {
                use sql_ast_benchmark::mem;
                use std::hint::black_box;
                if dialect != Dialect::Sqlite {
                    return None;
                }
                let before = mem::live();
                mem::reset_peak();
                let mut parser = $cr::parser::Parser::new(sql.as_bytes());
                let mut out = Vec::new();
                while let Ok(Some(cmd)) = parser.next_cmd() {
                    out.push(cmd);
                }
                black_box((&parser, &out));
                let r = (
                    mem::peak().saturating_sub(before),
                    mem::live().saturating_sub(before),
                );
                drop(out);
                drop(parser);
                Some(r)
            }

            fn reprint(&self, sql: &str, dialect: Dialect) -> Option<String> {
                if dialect != Dialect::Sqlite {
                    return None;
                }
                std::panic::catch_unwind(|| {
                    let mut parser = $cr::parser::Parser::new(sql.as_bytes());
                    let mut out: Vec<String> = Vec::new();
                    loop {
                        match parser.next_cmd() {
                            Ok(Some(cmd)) => out.push(cmd.to_string()),
                            Ok(None) => break,
                            Err(_) => return None,
                        }
                    }
                    if out.is_empty() {
                        None
                    } else {
                        Some(out.join("; "))
                    }
                })
                .unwrap_or(None)
            }

            fn can_reprint(&self, dialect: Dialect) -> bool {
                dialect == Dialect::Sqlite
            }
        }
    };
}

turso_version!(TursoV0_6, turso_v0_6, "0.6.1", "2026-05-22");

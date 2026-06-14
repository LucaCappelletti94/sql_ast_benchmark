# `CREATE USER` and `ALTER USER ... SET` consume the statement terminator, breaking any following statement

## Summary

In sqlparser, `CREATE USER <name>` (and `ALTER USER <name> SET ...`) parse correctly on their own, but when one is followed by another statement in the same script the parse fails. The shared helper `parse_key_value_options`, used to read the trailing option list, consumes the `;` terminator. The top-level statement loop then no longer sees a separator before the next statement and returns `Expected: end of statement, found: <next token>`, pointing at the first token after the semicolon.

The defect is in `parse_key_value_options` itself, so it affects every statement that ends by calling it in unparenthesized mode. In 0.62.0 there are three such call sites: `CREATE USER`, `ALTER USER ... SET <props>`, and `ALTER USER ... SET TAG ...`. The bug is dialect independent (`GenericDialect`, `MySqlDialect`, `PostgreSqlDialect`, and `SnowflakeDialect` all behave identically). Statements that do not reach the helper (for example `CREATE ROLE`, `DROP USER`, and `ALTER USER ... RENAME`) are unaffected.

## Affected versions

Reproduced on `sqlparser` 0.62.0 (crates.io) and on current `main`.

## Reproduction

`Cargo.toml`:

```toml
[dependencies]
sqlparser = "0.62.0"
```

`src/main.rs`:

```rust
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

fn check(sql: &str) {
    match Parser::parse_sql(&GenericDialect {}, sql) {
        Ok(v) => println!("{sql:<46} -> Ok({} statements)", v.len()),
        Err(e) => println!("{sql:<46} -> {e}"),
    }
}

fn main() {
    // Affected: each ends in an unparenthesized key-value option list.
    check("CREATE USER user1; SELECT 1");
    check("ALTER USER user1 SET x = 'y'; SELECT 1");
    check("ALTER USER user1 SET TAG t = 'v'; SELECT 1");

    // Fine on their own (the terminator is followed by EOF).
    check("CREATE USER user1");
    check("ALTER USER user1 SET x = 'y'");

    // Unaffected: never reach parse_key_value_options.
    check("SELECT 1; CREATE USER user1");
    check("CREATE ROLE role1; SELECT 1");
    check("DROP USER user1; SELECT 1");
    check("ALTER USER user1 RENAME TO user2; SELECT 1");
    check("SELECT 1; SELECT 2");
}
```

## Observed behavior

```text
CREATE USER user1; SELECT 1                    -> Expected: end of statement, found: SELECT at Line: 1, Column: 20
ALTER USER user1 SET x = 'y'; SELECT 1         -> Expected: end of statement, found: SELECT at Line: 1, Column: 31
ALTER USER user1 SET TAG t = 'v'; SELECT 1     -> Expected: end of statement, found: SELECT at Line: 1, Column: 35
CREATE USER user1                              -> Ok(1 statements)
ALTER USER user1 SET x = 'y'                   -> Ok(1 statements)
SELECT 1; CREATE USER user1                    -> Ok(2 statements)
CREATE ROLE role1; SELECT 1                    -> Ok(2 statements)
DROP USER user1; SELECT 1                      -> Ok(2 statements)
ALTER USER user1 RENAME TO user2; SELECT 1     -> Ok(2 statements)
SELECT 1; SELECT 2                             -> Ok(2 statements)
```

The first three inputs fail. Each affected statement parses alone, and the following statement parses alone, yet the two together fail. The affected statement even works when it is the last statement (`SELECT 1; CREATE USER user1` is `Ok(2)`), because then the terminator is followed by EOF and nothing is left to mis-parse. The reported column is always the position of the token immediately after the `;`, which shows the terminator has already been consumed by the time the error is raised.

## Expected behavior

`CREATE USER user1; SELECT 1` (and the two `ALTER USER ... SET` forms) should parse as two statements, the same way `CREATE ROLE role1; SELECT 1` and `SELECT 1; SELECT 2` do.

## Root cause

`parse_key_value_options` (src/parser/mod.rs, around line 20449) drives its loop with `self.next_token()`, which advances past the token it returns. Its terminator arm (around line 20468) breaks on a semicolon that has already been consumed:

```rust
loop {
    match self.next_token().token {
        // ...
        Token::EOF | Token::SemiColon => break, // the ';' is consumed, then we break
        // ...
    }
}
```

So when the option list is unparenthesized and ends at a `;`, the `;` is eaten and discarded. Control returns to the top-level statement loop, which expects a `;` separator (or EOF) before the next statement. Because the separator is gone, it sees the next statement's first token directly and fails with `Expected: end of statement, found: <token>`.

The three unparenthesized call sites in 0.62.0 are:

- `parse_create_user`, src/parser/mod.rs around line 5224: `self.parse_key_value_options(false, &[Keyword::WITH, Keyword::TAG])`.
- `parse_alter_user`, src/parser/alter.rs around line 262 (the `SET TAG` branch): `self.parse_key_value_options(false, &[])`.
- `parse_alter_user`, src/parser/alter.rs around line 280 (the `SET <props>` branch): `self.parse_key_value_options(false, &[])`.

This explains every case above:

- `CREATE USER user1` alone: the loop reads `EOF` and breaks. Fine.
- `CREATE USER user1; SELECT 1`: the loop consumes `;` and breaks, then the top level sees `SELECT` with no preceding separator. Error.
- `SELECT 1; CREATE USER user1`: `SELECT` is parsed normally and does not eat its trailing `;`, then `CREATE USER` is parsed last and ends at `EOF`. Fine.
- `CREATE ROLE`, `DROP USER`, `ALTER USER ... RENAME`: they do not route through `parse_key_value_options`, so they terminate correctly.

The parenthesized callers (`parse_key_value_options(true, ...)`, used by the Snowflake `FILE_FORMAT`, `COPY`, and similar option lists) are unaffected, because they end on `)` rather than on the statement terminator.

## Suggested fix

Do not consume the terminator. Put the semicolon back before breaking so the caller and the top-level statement loop can see it, for example:

```rust
loop {
    match self.next_token().token {
        // ...
        Token::EOF => break,
        Token::SemiColon => {
            self.prev_token();
            break;
        }
        // ...
    }
}
```

(The `EOF` case needs no `prev_token`.) Peeking instead of consuming would work as well. This mirrors how the `end_words` arm already calls `self.prev_token()` before breaking. Fixing the single helper repairs all three statements at once.

## Impact

Any multi-statement script that contains `CREATE USER` or `ALTER USER ... SET` in a non-final position fails to parse in full. This was found while benchmarking sqlparser on whole-script (multi-statement) parsing of real-world SQL corpora, where a single such statement voids the entire script because `parse_sql` is all-or-nothing.

## Suggested regression test

```rust
#[test]
fn key_value_option_statements_do_not_swallow_following_statement() {
    for sql in [
        "CREATE USER user1; SELECT 1",
        "ALTER USER user1 SET x = 'y'; SELECT 1",
        "ALTER USER user1 SET TAG t = 'v'; SELECT 1",
    ] {
        let stmts = Parser::parse_sql(&GenericDialect {}, sql).unwrap();
        assert_eq!(stmts.len(), 2, "{sql}");
    }
}
```

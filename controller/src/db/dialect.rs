// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Rewrites the controller's SQL, which is written for SQLite, so PostgreSQL accepts it.
//!
//! Most differences are covered inside the database: `migrations_pg/000_postgres_schema.sql` defines `datetime()`, `strftime()`,
//! `julianday()`, `hex()`, `printf()`, `json_extract()` and `json_each()` with SQLite's behaviour. What a function cannot do
//! is rewritten here, outside string literals and quoted identifiers:
//!
//! | SQLite | PostgreSQL |
//! |---|---|
//! | `?` and `?3` placeholders | `$1`, `$2`, ... and `$3` |
//! | `CURRENT_TIMESTAMP` | `machina_now()` (TEXT in the stored format) |
//! | `LIKE` (case-insensitive for ASCII) | `ILIKE` |
//! | `CAST(x AS REAL)` / `AS INTEGER` | `AS DOUBLE PRECISION` / `AS BIGINT` |
//! | `INSERT OR IGNORE INTO ...` | `INSERT INTO ... ON CONFLICT DO NOTHING` |
//! | `rowid` (insertion-order tie-break) | `seq`, an identity column that tasks, vm_restore_points and ha_events have in PostgreSQL |
//!
//! `INSERT OR REPLACE`, `typeof()` and the JSON builders are not rewritten: those few statements are written for each
//! backend at the call site, and a statement that slips through fails loudly in PostgreSQL's parser instead of misbehaving.

#[cfg(feature = "postgres")]
use std::collections::HashMap;
#[cfg(feature = "postgres")]
use std::sync::{Mutex, OnceLock};

/// Rewrite one statement for PostgreSQL.
pub fn to_postgres(sql: &str) -> String {
    let (sql, ignore_conflicts) = strip_insert_or_ignore(sql);
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len() + 24);
    let mut i = 0;
    let mut next_placeholder = 0u32;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\'' | b'"' => {
                // copy a quoted literal or identifier verbatim; a doubled quote is an escaped quote
                let q = c;
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == q {
                        if i + 1 < bytes.len() && bytes[i + 1] == q {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                out.push_str(&sql[start..i]);
            }
            b'?' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 1 {
                    out.push('$');
                    out.push_str(&sql[i + 1..j]);
                } else {
                    next_placeholder += 1;
                    out.push('$');
                    out.push_str(&next_placeholder.to_string());
                }
                i = j;
            }
            _ if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &sql[start..i];
                if word.eq_ignore_ascii_case("CURRENT_TIMESTAMP") {
                    out.push_str("machina_now()");
                } else if word.eq_ignore_ascii_case("LIKE") {
                    out.push_str("ILIKE");
                } else if word.eq_ignore_ascii_case("rowid") {
                    // insertion-order tie-break (`ORDER BY created_at, rowid`): tables that need it carry a `seq` identity column
                    out.push_str("seq");
                } else if word.eq_ignore_ascii_case("AS") {
                    out.push_str(word);
                    // `AS REAL` / `AS INTEGER` in a CAST: PostgreSQL's REAL is 4 bytes and INTEGER 4, the controller means 8
                    let mut j = i;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let mut k = j;
                    while k < bytes.len() && bytes[k].is_ascii_alphabetic() {
                        k += 1;
                    }
                    let next = &sql[j..k];
                    if next.eq_ignore_ascii_case("REAL") {
                        out.push_str(&sql[i..j]);
                        out.push_str("DOUBLE PRECISION");
                        i = k;
                    } else if next.eq_ignore_ascii_case("INTEGER") {
                        out.push_str(&sql[i..j]);
                        out.push_str("BIGINT");
                        i = k;
                    }
                } else {
                    out.push_str(word);
                }
            }
            _ => {
                // copy one UTF-8 character
                let ch = sql[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    if ignore_conflicts {
        append_on_conflict_do_nothing(&mut out);
    }
    out
}

/// `INSERT OR IGNORE INTO` -> `INSERT INTO`, reporting whether it was there.
fn strip_insert_or_ignore(sql: &str) -> (String, bool) {
    let trimmed = sql.trim_start();
    let lead = sql.len() - trimmed.len();
    const PREFIX: &str = "INSERT OR IGNORE";
    if trimmed.get(..PREFIX.len()).is_some_and(|head| head.eq_ignore_ascii_case(PREFIX)) {
        let mut s = String::with_capacity(sql.len());
        s.push_str(&sql[..lead]);
        s.push_str("INSERT");
        s.push_str(&trimmed[PREFIX.len()..]);
        return (s, true);
    }
    (sql.to_string(), false)
}

/// Put `ON CONFLICT DO NOTHING` at the end of an INSERT, in front of a trailing `RETURNING` clause.
fn append_on_conflict_do_nothing(out: &mut String) {
    let upper = out.to_ascii_uppercase();
    match upper.rfind(" RETURNING ") {
        Some(at) => out.insert_str(at, " ON CONFLICT DO NOTHING"),
        None => {
            let trimmed = out.trim_end().len();
            out.truncate(trimmed);
            out.push_str(" ON CONFLICT DO NOTHING");
        }
    }
}

/// Statements are rewritten once and kept for the life of the process (the controller's SQL is a bounded set of literals and a
/// few `format!` templates). The cache is capped so a call site that builds unbounded distinct SQL cannot grow it forever; past the
/// cap a statement is still rewritten and leaked, but a warning says so once.
#[cfg(feature = "postgres")]
pub(crate) fn cached_postgres(sql: &str) -> &'static str {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    const CAP: usize = 20_000;
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(hit) = guard.get(sql) {
        return hit;
    }
    let rewritten: &'static str = Box::leak(to_postgres(sql).into_boxed_str());
    if guard.len() < CAP {
        guard.insert(sql.to_string(), rewritten);
    } else if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::warn!("more than {CAP} distinct SQL statements: a call site builds SQL text from data instead of binding it");
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::to_postgres;

    #[test]
    fn question_marks_become_numbered_placeholders() {
        assert_eq!(to_postgres("SELECT * FROM t WHERE a = ? AND b = ?"), "SELECT * FROM t WHERE a = $1 AND b = $2");
        assert_eq!(to_postgres("SELECT ?1, ?2, ?1"), "SELECT $1, $2, $1");
    }

    #[test]
    fn quoted_text_is_never_rewritten() {
        assert_eq!(to_postgres("SELECT '?' , 'LIKE', \"CURRENT_TIMESTAMP\", 'it''s ?' FROM t WHERE x = ?"), "SELECT '?' , 'LIKE', \"CURRENT_TIMESTAMP\", 'it''s ?' FROM t WHERE x = $1");
        assert_eq!(to_postgres("SELECT * FROM t WHERE name LIKE 'a%?'"), "SELECT * FROM t WHERE name ILIKE 'a%?'");
    }

    #[test]
    fn current_timestamp_like_and_casts() {
        assert_eq!(to_postgres("UPDATE t SET at = CURRENT_TIMESTAMP WHERE n NOT LIKE ?"), "UPDATE t SET at = machina_now() WHERE n NOT ILIKE $1");
        assert_eq!(to_postgres("SELECT CAST(? AS REAL), cast(x as integer), CAST(y AS TEXT)"), "SELECT CAST($1 AS DOUBLE PRECISION), cast(x as BIGINT), CAST(y AS TEXT)");
        // identifiers that only contain a keyword are left alone
        assert_eq!(to_postgres("SELECT liked, current_timestamp_at, areal FROM t"), "SELECT liked, current_timestamp_at, areal FROM t");
    }

    #[test]
    fn insert_or_ignore_becomes_on_conflict_do_nothing() {
        assert_eq!(to_postgres("INSERT OR IGNORE INTO t (a, b) VALUES (?, ?)"), "INSERT INTO t (a, b) VALUES ($1, $2) ON CONFLICT DO NOTHING");
        assert_eq!(
            to_postgres("  insert or ignore into t (a) VALUES (?) RETURNING a"),
            "  INSERT into t (a) VALUES ($1) ON CONFLICT DO NOTHING RETURNING a"
        );
        assert_eq!(to_postgres("INSERT INTO t (a) VALUES (?)"), "INSERT INTO t (a) VALUES ($1)");
    }

    #[test]
    fn rowid_tie_breaks_use_the_seq_column() {
        assert_eq!(to_postgres("SELECT x FROM t ORDER BY created_at DESC, rowid DESC"), "SELECT x FROM t ORDER BY created_at DESC, seq DESC");
        assert_eq!(to_postgres("SELECT 'rowid', myrowid FROM t"), "SELECT 'rowid', myrowid FROM t");
    }

    #[test]
    fn functions_defined_in_the_database_pass_through() {
        let s = "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', created_at) FROM t WHERE created_at > datetime('now', '-1 hours')";
        assert_eq!(to_postgres(s), s);
    }

    #[test]
    fn non_ascii_text_survives() {
        assert_eq!(to_postgres("SELECT 'é ü ✓' || ?"), "SELECT 'é ü ✓' || $1");
    }
}

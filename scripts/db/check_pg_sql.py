#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Parse-check every literal SQL statement the controller sends, against the PostgreSQL schema.

Finds each string literal passed straight to crate::db::query / query_as / query_scalar, rewrites it the way the PostgreSQL build does
(controller/src/db/dialect.rs: placeholders, CURRENT_TIMESTAMP, LIKE, CAST types, INSERT OR IGNORE, rowid), and PREPAREs it in a
database that has controller/migrations_pg loaded. PREPARE runs PostgreSQL's full semantic analysis without executing anything, so
it catches what SQLite tolerated and PostgreSQL does not: a bare column next to GROUP BY, an unknown column or function, a
boolean compared with an integer, text compared with a uuid. Statements built with format!() are not literals and are not checked.

  PGURL=postgres://machina@127.0.0.1:5432/machina_check python3 scripts/db/check_pg_sql.py     (the database must already have the schema)
Exit status 1 when any statement is rejected. Errors about undeterminable parameter types are ignored: the check has no values to bind.
"""
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

# Test fixtures that use a table the same test creates with DDL (which cannot be prepared here)
ALLOW = {"INSERT INTO t VALUES ('a')"}
ROOT = Path(__file__).resolve().parent.parent.parent
CALL = re.compile(r"\bdb::query(?:_as|_scalar)?\s*(?:::<[^()]*?>)?\s*\(\s*(r#*\"|\")", re.S)


def rust_string(src, start):
    """Parse a Rust string literal whose opening quote token ends at `start`; returns (value, end) or None."""
    raw = src[start - 1] == "#" or src[start - 2 : start] == 'r"' or src[start - 3 : start - 1] == 'r#'
    # find the opening token
    i = start
    # determine raw-ness and hash count by scanning back to the 'r'
    j = start - 1
    hashes = 0
    while j >= 0 and src[j] == "#":
        hashes += 1
        j -= 1
    is_raw = j >= 0 and src[j] == "r" and src[start - 1] in '"#' and (src[start - 1] == '"' or hashes > 0)
    if src[start - 1] == '"' and not (j >= 0 and src[j] == "r" and hashes == 0 and src[start - 1] == '"' and src[start - 2 : start] == 'r"'):
        is_raw = src[start - 2 : start] == 'r"' or hashes > 0
    if is_raw:
        close = '"' + "#" * hashes
        end = src.find(close, i)
        if end < 0:
            return None
        return src[i:end], end + len(close)
    out = []
    while i < len(src):
        c = src[i]
        if c == "\\":
            n = src[i + 1]
            if n == "\n":  # line continuation: skip the newline and the next line's leading whitespace
                i += 2
                while i < len(src) and src[i] in " \t\r\n":
                    i += 1
                continue
            out.append({"n": "\n", "t": "\t", "r": "\r", '"': '"', "'": "'", "\\": "\\", "0": "\0"}.get(n, n))
            i += 2
            continue
        if c == '"':
            return "".join(out), i + 1
        out.append(c)
        i += 1
    return None


def literals():
    found = {}
    for path in sorted((ROOT / "controller/src").rglob("*.rs")):
        if path.name == "dialect.rs":
            continue
        src = path.read_text(encoding="utf-8")
        for m in CALL.finditer(src):
            start = m.end()
            lit = rust_string(src, start)
            if not lit:
                continue
            text, end = lit
            # a literal followed by `,` or `)` is the whole argument; `+` or `.` means it is part of an expression
            rest = src[end : end + 3].lstrip()
            if rest[:1] in ("+", "."):
                continue
            line = src.count("\n", 0, m.start()) + 1
            found.setdefault(text, []).append(f"{path.relative_to(ROOT)}:{line}")
    return found


def to_postgres(sql):
    """The rules of controller/src/db/dialect.rs."""
    s = sql.lstrip()
    ignore = bool(re.match(r"INSERT\s+OR\s+IGNORE", s, re.I))
    if ignore:
        s = re.sub(r"^INSERT\s+OR\s+IGNORE", "INSERT", s, count=1, flags=re.I)
    out, i, n = [], 0, 0
    while i < len(s):
        c = s[i]
        if c in "'\"":
            j = i + 1
            while j < len(s):
                if s[j] == c:
                    if j + 1 < len(s) and s[j + 1] == c:
                        j += 2
                        continue
                    j += 1
                    break
                j += 1
            out.append(s[i:j])
            i = j
        elif c == "?":
            j = i + 1
            while j < len(s) and s[j].isdigit():
                j += 1
            if j > i + 1:
                out.append("$" + s[i + 1 : j])
            else:
                n += 1
                out.append(f"${n}")
            i = j
        elif c.isalpha() or c == "_":
            j = i
            while j < len(s) and (s[j].isalnum() or s[j] == "_"):
                j += 1
            w = s[i:j]
            u = w.upper()
            if u == "CURRENT_TIMESTAMP":
                out.append("machina_now()")
            elif u == "LIKE":
                out.append("ILIKE")
            elif u == "ROWID":
                out.append("seq")
            elif u == "AS":
                m = re.match(r"(\s+)(REAL|INTEGER)\b", s[j:], re.I)
                out.append(w)
                if m:
                    out.append(m.group(1) + ("DOUBLE PRECISION" if m.group(2).upper() == "REAL" else "BIGINT"))
                    j += m.end()
            else:
                out.append(w)
            i = j
        else:
            out.append(c)
            i += 1
    res = "".join(out)
    if ignore:
        k = res.upper().rfind(" RETURNING ")
        res = res[:k] + " ON CONFLICT DO NOTHING" + res[k:] if k >= 0 else res.rstrip() + " ON CONFLICT DO NOTHING"
    return res


def main():
    url = os.environ.get("PGURL")
    if not url:
        sys.exit("set PGURL to a database that has controller/migrations_pg loaded")
    stmts = literals()
    items = list(stmts.items())
    script = ["\\set ON_ERROR_STOP 0", "\\set VERBOSITY terse"]
    for k, (text, _) in enumerate(items):
        sql = to_postgres(text).strip().rstrip(";")
        if text.strip() in ALLOW or re.match(r"(CREATE|DROP|ALTER|PRAGMA|VACUUM|BEGIN|COMMIT)\b", sql, re.I):
            continue  # DDL and transaction control cannot be prepared
        script.append(f"\\warn @@{k}")
        script.append(f"PREPARE s{k} AS {sql};")
        script.append(f"DEALLOCATE s{k};")
    r = subprocess.run(["psql", url, "-X", "-q", "-f", "-"], input="\n".join(script), capture_output=True, text=True)
    errors = defaultdict(list)
    cur = None
    for line in r.stderr.splitlines():  # markers (\\warn) and errors share stderr, so their order is kept
        m = re.match(r"@@(\d+)", line)
        if m:
            cur = int(m.group(1))
            continue
        m = re.search(r"ERROR:\s+(.*)", line)
        # no values are bound, so parameters have no type: those errors, and the follow-up when a failed PREPARE leaves nothing to
        # DEALLOCATE, say nothing about the statement
        ignore = ("could not determine data type of parameter", "is not unique", "prepared statement", "operator is not unique")
        if m and cur is not None and not any(i in m.group(1) for i in ignore):
            errors[cur].append(m.group(1))
    for k in sorted(errors):
        text, where = items[k]
        print(f"{where[0]}  [{len(where)} site(s)]\n    {' '.join(text.split())[:160]}\n    -> {errors[k][0]}")
    print(f"\n{len(items)} distinct statements checked, {len(errors)} rejected")
    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    main()

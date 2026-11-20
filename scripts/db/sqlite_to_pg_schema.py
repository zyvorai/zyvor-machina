#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Draft the PostgreSQL baseline from the controller's SQLite migrations.

Applies controller/migrations/*.sql to an in-memory SQLite database, reads back every table and index, and rewrites them in
PostgreSQL syntax: INTEGER -> BIGINT, REAL -> DOUBLE PRECISION, BLOB -> UUID (every BLOB column here is a 16-byte id), columns
declared `CHECK(length(col) = 16)` or linked to a UUID column by a foreign key -> UUID, and CURRENT_TIMESTAMP / strftime
defaults -> the same TEXT timestamps the controller already writes. Foreign keys are emitted after all tables so creation order
does not matter. The output is a starting point that is reviewed by hand and committed as
controller/migrations_pg/000_postgres_schema.sql (with scripts/db/pg_compat.sql at the top); it is not regenerated on every change.

  python3 scripts/db/sqlite_to_pg_schema.py > /tmp/draft.sql
"""
import glob
import re
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
TS = "to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')"
TS_ISO = "to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')"
REF = r"REFERENCES\s+\"?(\w+)\"?\s*\(\s*\"?(\w+)\"?\s*\)"
REF_FULL = REF.replace("(\\w+)\\\"?\\s*\\)", "(\\w+)\\\"?\\s*\\)") + r"(?:\s+ON\s+(?:DELETE|UPDATE)\s+(?:CASCADE|SET NULL|SET DEFAULT|RESTRICT|NO ACTION))*"
RESERVED = ("user", "order", "group", "limit")
# Id-looking columns that hold strings, not 16-byte ids (checked against a populated controller database; the rest default to UUID).
TEXT_IDS = {
    "clusters.oidc_client_id", "controller_leadership.holder_id", "ec2_access_keys.access_key_id", "resource_tags.resource_id",
    "ai_models.model_id", "soc_forwarder_cursors.last_event_id", "console_sessions.audit_id",
    "vm_atlas_volumes.volume_id", "vm_atlas_volumes.backend_native_id", "volume_snapshots.atlas_snapshot_id", "volumes.atlas_volume_id",
    "security_group_rules.remote_sg_id",
}
TEXT_ID_PREFIXES = ("ai_",)
# Declared as a reference to an id column, but the controller stores the *hex text* of the id there (never the 16-byte value), so
# in PostgreSQL the column is TEXT with no foreign key (a TEXT column cannot reference a uuid one).
HEX_TEXT_REFS = {("security_group_rules", "remote_sg_id")}
# Tables whose queries break ties with SQLite's `rowid` ("the row inserted later"); PostgreSQL gets an explicit identity column
# and db::dialect rewrites `rowid` to `seq`.
SEQ_TABLES = {"tasks", "vm_restore_points", "ha_events"}
# INTEGER columns the controller reads or binds as a Rust `bool` become BOOLEAN (PostgreSQL will not decode or bind a bool as a
# bigint). Found from the FromRow structs plus the columns the PostgreSQL test run reported; a column that is flag-like but missing
# here fails loudly ("mismatched types ... BOOL") the first time the code touches it, so add it here and regenerate.
BOOL_NAMES = {
    "anti_affinity", "applied", "atlas_backed", "auto_heal", "builtin", "cloud_init", "delete_on_termination", "delivered",
    "dhcp_pinned", "disk_only", "drs_auto_migrate", "enabled", "enforced", "evacuate", "featured", "fence_on_failure", "fenced",
    "firewall_enabled", "ha_allow_unfenced_recovery", "ha_enabled", "installed", "inventory_mark_managed_missing",
    "inventory_prune_unmanaged", "is_public", "live", "maintenance_mode", "managed", "marketplace", "paused", "quiesce",
    "require_vm_delete_approval", "schedulable", "success", "preemptible", "quiesced", "block",
    "is_default", "lockdown_enabled", "ok", "recording_enabled", "zeus_air_gap_llm", "zeus_memory_enabled", "zeus_memory_project_scope", "zeus_memory_team_scope",
}


def load():
    c = sqlite3.connect(":memory:")
    files = sorted(glob.glob(str(ROOT / "controller/migrations/*.sql")))
    for f in files:
        c.executescript(Path(f).read_text())
    last = Path(files[-1]).name
    tables = c.execute("select name, sql from sqlite_master where type='table' and sql is not null and name not like 'sqlite_%' order by name").fetchall()
    indexes = c.execute("select name, sql from sqlite_master where type='index' and sql is not null order by name").fetchall()
    return tables, indexes, last, c


def strip_comments(sql):
    out = []
    for line in sql.split("\n"):
        q = None
        for i, ch in enumerate(line):
            if q:
                if ch == q:
                    q = None
            elif ch in "'\"":
                q = ch
            elif line[i : i + 2] == "--":
                line = line[:i]
                break
        out.append(line.rstrip())
    return "\n".join(out)


def split_defs(body):
    """Split a CREATE TABLE body on top-level commas."""
    out, depth, cur, q = [], 0, "", None
    for ch in body:
        if q:
            cur += ch
            if ch == q:
                q = None
            continue
        if ch in "'\"":
            q = ch
            cur += ch
        elif ch == "(":
            depth += 1
            cur += ch
        elif ch == ")":
            depth -= 1
            cur += ch
        elif ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def parse_table(sql):
    sql = strip_comments(sql)
    m = re.match(r"CREATE TABLE\s+(?:IF NOT EXISTS\s+)?\"?(\w+)\"?\s*\((.*)\)\s*;?\s*$", sql.strip(), re.S)
    return split_defs(m.group(2))


def uuid_columns(parsed):
    """Columns that hold a 16-byte id: BLOBs, CHECK(length(col) = 16), randomblob defaults, and anything a foreign key ties to one."""
    uuid = set()
    parent = {}

    def find(x):
        parent.setdefault(x, x)
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for t, defs in parsed.items():
        for d in defs:
            m = re.match(r"\"?(\w+)\"?\s+(TEXT|BLOB)\b", d, re.I)
            if m:
                col, typ = m.group(1), m.group(2).upper()
                if typ == "BLOB" or "randomblob(16)" in d or re.search(r"CHECK\s*\(\s*length\(\s*\"?%s\"?\s*\)\s*=\s*16\s*\)" % col, d, re.I):
                    uuid.add((t, col))
                r = re.search(REF, d, re.I)
                if r and (t, col) not in HEX_TEXT_REFS:
                    parent[find((t, col))] = find((r.group(1), r.group(2)))
            f = re.match(r"FOREIGN KEY\s*\(\s*\"?(\w+)\"?\s*\)\s*" + REF, d, re.I)
            if f:
                parent[find((t, f.group(1)))] = find((f.group(2), f.group(3)))
    roots = {find(c) for c in uuid}
    for c in list(parent):
        if find(c) in roots:
            uuid.add(c)
    return uuid


def column(name, d, uuid, fks, review):
    m = re.match(r"\"?(\w+)\"?\s+(\w+)(.*)$", d, re.S)
    col, typ, rest = m.group(1), m.group(2).upper(), m.group(3)
    rest = re.sub(r"CHECK\s*\(\s*length\(\s*\"?%s\"?\s*\)\s*=\s*16\s*\)" % col, "", rest, flags=re.I)
    rest = rest.replace("DEFAULT (randomblob(16))", "DEFAULT gen_random_uuid()")
    rest = re.sub(r"DEFAULT\s*\(?\s*CURRENT_TIMESTAMP\s*\)?", "DEFAULT (%s)" % TS, rest, flags=re.I)
    rest = re.sub(r"DEFAULT\s*\(\s*strftime\('%Y-%m-%dT%H:%M:%fZ',\s*'now'\)\s*\)", "DEFAULT (%s)" % TS_ISO, rest, flags=re.I)
    if re.search(r"AUTOINCREMENT", rest, re.I):
        rest = re.sub(r"PRIMARY KEY\s+AUTOINCREMENT", "PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY", rest, flags=re.I)
        typ = "BIGINT"
    ref = re.search(r"\s*" + REF_FULL, rest, re.I)
    if ref:
        if (name, col) not in HEX_TEXT_REFS:
            fks.append((name, col, ref.group(0).strip()))
        rest = rest.replace(ref.group(0), "")
    id_like = col == "id" or col.endswith("_id")
    if typ == "TEXT" and id_like and "%s.%s" % (name, col) not in TEXT_IDS and not name.startswith(TEXT_ID_PREFIXES) and (name, col) not in uuid:
        uuid.add((name, col))  # shows up in the report below so each guess is visible
        review.append("-> uuid: %s.%s" % (name, col))
    if (name, col) in uuid:
        typ = "UUID"
    elif typ == "INTEGER" and col in BOOL_NAMES:
        typ = "BOOLEAN"
        rest = re.sub(r"DEFAULT\s+\(?\s*1\s*\)?", "DEFAULT TRUE", rest)
        rest = re.sub(r"DEFAULT\s+\(?\s*0\s*\)?", "DEFAULT FALSE", rest)
        rest = re.sub(r"CHECK\s*\(\s*%s\s+IN\s*\(\s*0\s*,\s*1\s*\)\s*\)" % col, "", rest, flags=re.I)
    elif typ == "INTEGER":
        typ = "BIGINT"
    elif typ == "REAL":
        typ = "DOUBLE PRECISION"
    elif typ == "JSON":
        typ = "TEXT"
    if typ == "TEXT" and id_like:
        review.append("kept text: %s.%s" % (name, col))
    return "    %s %s%s" % ('"%s"' % col if col.lower() in RESERVED else col, typ, rest.rstrip())


def pg_literal(v, is_uuid, col_type, col=""):
    if v is None:
        return "NULL"
    if col in BOOL_NAMES and col_type.upper() == "INTEGER":
        return "TRUE" if v else "FALSE"
    if is_uuid:
        h = v.hex() if isinstance(v, (bytes, bytearray)) else str(v)
        return "'%s-%s-%s-%s-%s'::uuid" % (h[0:8], h[8:12], h[12:16], h[16:20], h[20:32]) if len(h) == 32 else "'%s'::uuid" % h
    if isinstance(v, (int, float)):
        return repr(v)
    if isinstance(v, (bytes, bytearray)):
        return "'\\x%s'::bytea" % v.hex()
    return "'" + str(v).replace("'", "''") + "'"


def seed_rows(conn, tables, uuid):
    """The rows the SQLite migrations insert (defaults, catalogs): the baseline has to create the same ones."""
    out = []
    for name, _ in tables:
        cols = conn.execute("pragma table_info(%s)" % name).fetchall()
        rows = conn.execute("select * from %s" % name).fetchall()
        for row in rows:
            names = ", ".join('"%s"' % c[1] if c[1].lower() in RESERVED else c[1] for c in cols)
            vals = ", ".join(pg_literal(v, (name, c[1]) in uuid, c[2], c[1]) for v, c in zip(row, cols))
            out.append("INSERT INTO %s (%s) VALUES (%s);" % (name, names, vals))
    return out


def main():
    tables, indexes, last, conn = load()
    parsed = {n: parse_table(s) for n, s in tables}
    uuid = uuid_columns(parsed)
    n = int(last.split("_")[0])
    out = [
        "-- PostgreSQL baseline for machina-controller: the same schema as SQLite migrations 000-%03d." % n,
        "-- Drafted by scripts/db/sqlite_to_pg_schema.py and reviewed by hand. From migration %03d on, every change is" % (n + 1),
        "-- written twice: controller/migrations (SQLite) and controller/migrations_pg (here).",
        "",
    ]
    out.append((ROOT / "scripts/db/pg_compat.sql").read_text().rstrip() + "\n")
    fks, review = [], []
    for name, _ in tables:
        cols = []
        for d in parsed[name]:
            if re.match(r"(PRIMARY|UNIQUE|FOREIGN|CHECK|CONSTRAINT)\b", d, re.I):
                f = re.match(r"FOREIGN KEY\s*\(([^)]*)\)\s*(REFERENCES.*)$", d, re.I | re.S)
                if f:
                    fks.append((name, f.group(1).strip(), f.group(2).strip()))
                else:
                    cols.append("    " + d)
            else:
                cols.append(column(name, d, uuid, fks, review))
        if name in SEQ_TABLES:
            cols.append("    seq BIGINT GENERATED ALWAYS AS IDENTITY")
        out.append("CREATE TABLE %s (\n%s\n);\n" % (name, ",\n".join(cols)))
    seeds = seed_rows(conn, tables, uuid)
    out.append("-- rows the SQLite migrations seed (defaults and catalogs)")
    out.extend(seeds)
    out.append("")
    for i, (t, c, r) in enumerate(fks):
        out.append("ALTER TABLE %s ADD CONSTRAINT %s_%s_fk%d FOREIGN KEY (%s) %s;" % (t, t, re.sub(r"\W+", "_", c), i, c, r))
    out.append("")
    out.append("CREATE TRIGGER preserve_routable_agent_addr BEFORE UPDATE OF agent_grpc_addr ON hosts")
    out.append("  FOR EACH ROW EXECUTE FUNCTION preserve_routable_agent_addr();")
    out.append("")
    for _, sql in indexes:
        stmt = sql.strip().rstrip(";")
        for b in BOOL_NAMES:
            stmt = re.sub(r"\b%s\s*=\s*1\b" % b, "%s = TRUE" % b, stmt)
            stmt = re.sub(r"\b%s\s*=\s*0\b" % b, "%s = FALSE" % b, stmt)
        out.append(stmt + ";")
    out.append("")
    sys.stdout.write("\n".join(out))
    sys.stderr.write("TEXT id-like columns left as TEXT (review): %d\n%s\n" % (len(review), "\n".join(review)))


if __name__ == "__main__":
    main()

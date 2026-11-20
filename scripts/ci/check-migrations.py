#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Guard the controller's SQL migrations.

sqlx applies migrations by their numeric prefix, so two files with one number (it has happened: two branches each took the
next free number) or a renumbered, already-applied file breaks upgrades. Checks that every file is `NNN_name.sql`, that numbers
are unique and strictly increasing, and that no number is skipped except the documented historical gap. With `--base REF` it
also refuses to change or delete a migration that already exists at REF (applied migrations are immutable).

The PostgreSQL schema (controller/migrations_pg) must stay in step: its baseline file 000_postgres_schema.sql says which SQLite
migrations it covers ("000-060"); every SQLite migration after that needs a file with the same number in migrations_pg, and a
migrations_pg file may not exist for a number the baseline already covers. (The baseline itself may still change until the first
release that ships it; numbered files after it are immutable like the SQLite ones.)

Usage: scripts/ci/check-migrations.py [--base origin/main]
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
DIR = ROOT / "controller" / "migrations"
PG_DIR = ROOT / "controller" / "migrations_pg"
NAME_RE = re.compile(r"^(\d{3})_[a-z0-9_]+\.sql$")
KNOWN_GAPS = {30}  # 030 was never used


def main():
    problems = []
    nums = {}
    for p in sorted(DIR.glob("*.sql")):
        m = NAME_RE.match(p.name)
        if not m:
            problems.append(f"{p.name}: name must look like 042_short_name.sql")
            continue
        n = int(m.group(1))
        if n in nums:
            problems.append(f"{p.name}: number {n:03d} is also used by {nums[n]}")
        nums[n] = p.name
    ordered = sorted(nums)
    for a, b in zip(ordered, ordered[1:]):
        missing = [x for x in range(a + 1, b) if x not in KNOWN_GAPS]
        if missing:
            problems.append(f"numbers skipped between {a:03d} and {b:03d}: {', '.join(f'{x:03d}' for x in missing)}")
    # PostgreSQL parity
    baseline = PG_DIR / "000_postgres_schema.sql"
    if baseline.exists():
        m = re.search(r"migrations 000-(\d{3})", baseline.read_text(encoding="utf-8")[:400])
        if not m:
            problems.append("migrations_pg/000_postgres_schema.sql: first lines must say which SQLite migrations it covers (\"migrations 000-NNN\")")
        else:
            covered = int(m.group(1))
            pg = {}
            for p in sorted(PG_DIR.glob("*.sql")):
                pm = NAME_RE.match(p.name)
                if p.name == baseline.name:
                    continue
                if not pm:
                    problems.append(f"migrations_pg/{p.name}: name must look like 061_short_name.sql")
                    continue
                pg[int(pm.group(1))] = p.name
            for n in pg:
                if n <= covered:
                    problems.append(f"migrations_pg/{pg[n]}: the baseline already covers migrations up to {covered:03d}")
            for n in sorted(x for x in nums if x > covered):
                if n not in pg:
                    problems.append(f"migrations/{nums[n]} has no PostgreSQL counterpart: add controller/migrations_pg/{nums[n]} (the same change in PostgreSQL syntax)")
            for n in sorted(pg):
                if n not in nums:
                    problems.append(f"migrations_pg/{pg[n]} has no SQLite counterpart in controller/migrations")
    if "--base" in sys.argv:
        base = sys.argv[sys.argv.index("--base") + 1]
        out = subprocess.run(["git", "diff", "--name-status", f"{base}...HEAD", "--", "controller/migrations", "controller/migrations_pg"], cwd=ROOT, capture_output=True, text=True)
        for line in out.stdout.splitlines():
            status, _, name = line.partition("\t")
            if name.endswith("000_postgres_schema.sql"):
                continue
            if status[:1] in ("M", "D", "R"):
                problems.append(f"{name}: an existing migration was {'modified' if status[:1] == 'M' else 'deleted or renamed'}; add a new migration instead")
    if problems:
        print("\n".join(problems))
        return 1
    print(f"migrations ok: {len(nums)} files, {ordered[0]:03d}..{ordered[-1]:03d}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

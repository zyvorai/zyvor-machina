#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Run the unit tests of every Fleet Cloud feature in docs/guides/features.json, one feature at a time.

A feature fails when any of its tests fails OR when fewer than `min` tests ran, so a renamed, moved or deleted test cannot
silently stop covering it. Prints a table and writes it to $GITHUB_STEP_SUMMARY when set.

Usage: scripts/ci/fleet-cloud-feature-tests.py [--feature ID ...] [--locked] [--dry-run]
  --dry-run  only validate the manifest (crates, filters, minimums); run nothing.
"""
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
MANIFEST = ROOT / "docs" / "guides" / "features.json"
RESULT_RE = re.compile(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed")


def load():
    data = json.loads(MANIFEST.read_text(encoding="utf-8"))
    problems = []
    seen = set()
    for f in data["features"]:
        if f["id"] in seen:
            problems.append(f"duplicate feature id {f['id']}")
        seen.add(f["id"])
        if not f.get("tests"):
            problems.append(f"{f['id']}: no tests listed")
        for t in f.get("tests", []):
            if not t.get("filters") or not isinstance(t.get("min"), int) or t["min"] < 1:
                problems.append(f"{f['id']}: every test entry needs filters and a minimum of at least 1")
            if not (ROOT / t["crate"].replace("machina-", "")).is_dir() and t["crate"] not in ("machina-controller",):
                problems.append(f"{f['id']}: unknown crate {t['crate']}")
    return data["features"], problems


def run_entry(entry, locked):
    cmd = ["cargo", "test", "-p", entry["crate"]] + (["--locked"] if locked else []) + ["--"] + entry["filters"]
    proc = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    out = proc.stdout + proc.stderr
    passed = failed = 0
    for m in RESULT_RE.finditer(out):
        passed += int(m.group(2))
        failed += int(m.group(3))
    return proc.returncode, passed, failed, out


def main():
    args = sys.argv[1:]
    locked = "--locked" in args
    dry = "--dry-run" in args
    only = [args[i + 1] for i, a in enumerate(args) if a == "--feature" and i + 1 < len(args)]
    features, problems = load()
    if problems:
        print("\n".join(f"manifest: {p}" for p in problems))
        return 1
    if dry:
        print(f"manifest ok: {len(features)} features")
        return 0
    rows, bad = [], False
    for f in features:
        if only and f["id"] not in only:
            continue
        passed = failed = 0
        notes = []
        for entry in f["tests"]:
            rc, p, fl, out = run_entry(entry, locked)
            passed += p
            failed += fl
            if rc != 0 or fl:
                notes.append(f"{entry['crate']} {entry['filters']}: cargo exited {rc}")
                print(out[-4000:])
            if p < entry["min"]:
                notes.append(f"{entry['crate']} {entry['filters']}: {p} tests ran, expected at least {entry['min']}")
        ok = not notes
        bad |= not ok
        rows.append((f["id"], f["title"], passed, failed, "ok" if ok else "; ".join(notes)))
    table = ["| Feature | Tests passed | Failed | Status |", "|---|---|---|---|"] + [f"| {i} | {p} | {fl} | {s} |" for i, _, p, fl, s in rows]
    text = "\n".join(table)
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as fh:
            fh.write("### Fleet Cloud feature tests\n\n" + text + "\n")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())

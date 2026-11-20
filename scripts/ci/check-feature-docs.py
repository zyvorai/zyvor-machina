#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Every Fleet Cloud feature in docs/guides/features.json needs a guide that says what it is, how to configure it, how to use
it, how to check it works, and where it stops. Also checks that the guides are linked from docs/guides/README.md.
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
REQUIRED = ["## What it is", "## Configure", "## Use", "## Check it works", "## Limits"]


def main():
    data = json.loads((ROOT / "docs/guides/features.json").read_text(encoding="utf-8"))
    problems = []
    index = (ROOT / "docs/guides/README.md")
    index_text = index.read_text(encoding="utf-8") if index.exists() else ""
    if not index.exists():
        problems.append("docs/guides/README.md is missing")
    for f in data["features"]:
        guide = ROOT / f["guide"]
        if not guide.exists():
            problems.append(f"{f['id']}: guide {f['guide']} is missing")
            continue
        text = guide.read_text(encoding="utf-8")
        for h in REQUIRED:
            if h not in text:
                problems.append(f"{f['guide']}: missing section '{h}'")
        if guide.name not in index_text:
            problems.append(f"{f['guide']}: not linked from docs/guides/README.md")
        if len(text) < 1200:
            problems.append(f"{f['guide']}: too short to be a guide ({len(text)} characters)")
    if problems:
        print("\n".join(problems))
        return 1
    print(f"feature docs ok: {len(data['features'])} guides")
    return 0


if __name__ == "__main__":
    sys.exit(main())

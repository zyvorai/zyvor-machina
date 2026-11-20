#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""CycloneDX 1.5 SBOM for a Machina release, from the lockfiles (Rust: Cargo.lock; web: web/package-lock.json;
Go: go.sum of the SDK and provider). Standard library only. Usage: gen-sbom.py VERSION OUT.json"""
import json
import re
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def cargo_components():
    text = (ROOT / "Cargo.lock").read_text()
    out = []
    for m in re.finditer(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"(?:\nsource = "([^"]+)")?(?:\nchecksum = "([^"]+)")?', text):
        name, ver, source, checksum = m.groups()
        if not source:  # workspace members are described by the root component, not as dependencies
            continue
        c = {"type": "library", "name": name, "version": ver, "purl": f"pkg:cargo/{name}@{ver}", "bom-ref": f"cargo:{name}@{ver}"}
        if checksum:
            c["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        out.append(c)
    return out


def npm_components():
    p = ROOT / "web" / "package-lock.json"
    if not p.exists():
        return []
    out = []
    for path, info in json.loads(p.read_text()).get("packages", {}).items():
        if not path or not info.get("version") or info.get("dev"):
            continue  # skip the root and development-only packages: they are not shipped
        name = path.split("node_modules/")[-1]
        c = {"type": "library", "name": name, "version": info["version"], "purl": f"pkg:npm/{name.replace('@', '%40', 1) if name.startswith('@') else name}@{info['version']}", "bom-ref": f"npm:{name}@{info['version']}"}
        if info.get("license"):
            c["licenses"] = [{"license": {"id": info["license"]}}] if re.fullmatch(r"[A-Za-z0-9.\-+]+", str(info["license"])) else [{"expression": str(info["license"])}]
        if info.get("integrity", "").startswith("sha512-"):
            import base64
            c["hashes"] = [{"alg": "SHA-512", "content": base64.b64decode(info["integrity"][7:]).hex()}]
        out.append(c)
    return out


def go_components():
    out = []
    for mod in ("sdk/go", "terraform/provider"):
        gm = ROOT / mod / "go.mod"
        if not gm.exists():
            continue
        for m in re.finditer(r"^\s*([\w./~-]+)\s+(v[\w.+-]+)(?:\s+//\s*indirect)?\s*$", gm.read_text(), re.M):
            name, ver = m.groups()
            if name.startswith("github.com/zyvorai/"):
                continue
            out.append({"type": "library", "name": name, "version": ver, "purl": f"pkg:golang/{name}@{ver}", "bom-ref": f"go:{name}@{ver}"})
    return out


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    version, outp = sys.argv[1], sys.argv[2]
    seen, comps = set(), []
    for c in cargo_components() + npm_components() + go_components():
        if c["bom-ref"] in seen:
            continue
        seen.add(c["bom-ref"])
        comps.append(c)
    bom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{uuid.uuid4()}",
        "version": 1,
        "metadata": {
            "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "tools": [{"vendor": "Zyvor AI Labs", "name": "machina gen-sbom.py"}],
            "component": {"type": "application", "name": "machina", "version": version, "bom-ref": "machina", "licenses": [{"license": {"name": "LicenseRef-Zyvor-Production-1.0"}}]},
        },
        "components": comps,
    }
    Path(outp).write_text(json.dumps(bom, indent=1))
    kinds = {}
    for c in comps:
        k = c["purl"].split(":")[1].split("/")[0]
        kinds[k] = kinds.get(k, 0) + 1
    print(f"SBOM {outp}: {len(comps)} components {kinds}")


main()

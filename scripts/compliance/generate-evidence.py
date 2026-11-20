#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Generate a machine-readable security evidence manifest from build/test artifacts."""
import argparse, hashlib, json, pathlib, time
p=argparse.ArgumentParser(); p.add_argument('--root', default='.'); p.add_argument('--out', default='machina-evidence.json'); a=p.parse_args()
root=pathlib.Path(a.root)
patterns=['SHA256SUMS','*.sigstore.json','*.sbom.cdx.json','TEST_RESULTS.md','support*.zip']
files=[]; seen=set()
for pat in patterns:
    for f in root.rglob(pat):
        if not f.is_file() or f in seen: continue
        seen.add(f); b=f.read_bytes(); files.append({'path':str(f.relative_to(root)),'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b)})
out={'schema':'zyvor.machina.security-evidence/v1','generated_unix':int(time.time()),'controls':{
    'supply_chain':['SBOM','SHA256','Sigstore'],
    'runtime':['mTLS','RBAC','audit','eBPF enforcement lease'],
    'availability':['HA fencing','backup verification','upgrade rollback'],
},'artifacts':sorted(files,key=lambda x:x['path'])}
pathlib.Path(a.out).write_text(json.dumps(out,indent=2)+'\n')
print(a.out)

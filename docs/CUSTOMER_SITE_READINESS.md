# Customer site readiness

Evidence-based gate for deploying Machina at a customer site. Update this
document when live regression waves complete (see
[`scripts/regression/RESULTS.md`](../scripts/regression/RESULTS.md)).

## Verdict (2026-08-06)

| Scope | Status |
|-------|--------|
| **Single-host Linux KVM/libvirt + GuestKit agent + platform controller** | **Pilot-ready** |
| Multi-host HA failover under real host loss | API dry-run **PASS** (`/api/v1/ha/status`, `ha_enabled=false` on lab); still needs customer host-loss drill |
| Atlas / Ceph storage fabric | Soft-pass when disabled; enable + retest before claiming |
| KubeVirt-primary | Status/negative covered; not primary path on lab host |
| Windows guest RDP-first | Lab golden `win10-msedge`: feature-test **26/26**; offline `enable-rdp` **200** via GuestKit only (`plan apply --skip-backup`, hivex). No Machina `virt-win-reg`/libguestfs-tools path. Dirty NTFS: GuestKit `ntfsfix` before mount |
| Linux offline GuestKit | GuestKit ≥ **0.3.17**: `linux-ssh` plan + rescue `enable-ssh` / `inject-ssh-key` / `reset-password` / `fix-fstab` / `set-hostname`. Machina: `POST /api/v1/vms/{name}/linux/*` (VM shut off) |
| Full UI CDP page-sweep | **Done** — `npm run pages` → **130/130** pass / 0 soft / 0 fail (2026-08-06 full test-all); all `ui`/`ui-*` suites **WAVE_C_FAILS=0** |

**Language for delivery:** Machina is **pilot-ready** for a guided single-site
(or small fleet) Linux KVM deployment after the checklist below. Broader
topologies are **scoped expansions**, not assumed.

## Live evidence (lab: <ephemeral-ip> / chrome-e2e-vm + win10-msedge)

- **Full lab test-all (2026-08-06):** waves A–G green — see [`scripts/regression/RESULTS.md`](../scripts/regression/RESULTS.md) “full lab test-all”
- GuestKit matrix: `./scripts/guestkit-live-matrix.sh --with-offline` → **29/29** (2026-08-06; run from laptop with `MACHINA_SSH`)
- API ops: full ops catalog green + pages **130/130** (2026-08-06; `host-filesystems` soft-timeout in harness if deep walk hangs)
- UI CDP: all `ui` / `ui-*` suites **0 fails** (2026-08-06)
- Media/guest-tools: `feature-test.sh` **26/26** on both goldens (2026-08-06)
- API heartbeat: `npm run api` **13/13** (2026-08-06)
- Offline Windows RDP: `POST …/windows/enable-rdp` → **200** via GuestKit only (no Machina virt-win-reg)
- Demo reels published:
  - [Machina × GuestKit](https://youtu.be/LYoqOye3P3I)
  - [Machina desktop wow reel](https://youtu.be/GYjvbKwUufA) (Cinema + Mission Control, ~30s; lab 2026-08-05)
  - [Linux golden → Create VM](https://youtu.be/8PRWTuKZ3kM) (full process)
  - [Windows golden → Create VM](https://youtu.be/wtrp6pp8I0s) (full process)

Prefer SSH tunnel for API login (avoids PAM rate limits):

```bash
ssh -f -N -L 15092:127.0.0.1:5092 operator@CUSTOMER_HOST
export MACHINA_BASE_URL=https://127.0.0.1:15092
export MACHINA_USER=… MACHINA_PASS=…
cd scripts/regression && npm run once   # or targeted suites
```

## Pre-install checklist

1. **Host:** Ubuntu 22.04/24.04 (or supported), KVM/QEMU/libvirt, nested virt if required, passwordless sudo for deploy user  
2. **Network:** management IP reachable; `virbr0` or customer bridge for guests; firewall plan for `:5092` / `:5093` / agent `:50051`  
3. **Auth:** PAM local admin or OIDC/SAML/LDAP configured; document break-glass account  
4. **Storage:** default libvirt pool sized; Atlas only if `ATLAS_ENABLED=1` + gateway tested  
5. **Backup:** schedule copy of `/var/lib/machina/controller.db` (+ config under `/etc/machina`)  
6. **Guest image:** Linux golden with GuestKit/QGA channel `org.qemu.guest_agent.0` (and `ssh-keygen -A` if SSH NAT is required)

## Deploy

```bash
./scripts/deploy-remote.sh USER@HOST --quick --platform
# or full install / reinstall per CLAUDE.md
curl -sk https://HOST:5092/api/v1/health
curl -sk http://HOST:5093/api/v1/health   # controller
```

## Acceptance tests (must pass on *their* first VM)

| Gate | Command |
|------|---------|
| GuestKit | `./scripts/guestkit-live-matrix.sh` (SSH target = customer host) |
| Core VM ops | `npm run power && npm run guest && npm run net && npm run disk` |
| Platform/host | `npm run platform && npm run storage && npm run host && npm run zeus` |
| Security | `npm run hunt && npm run firewallx` (dry-run lockdown only) |
| Media | `VM=<name> ./scripts/feature-test.sh 127.0.0.1 USER PASS` (on host) |
| UI smoke | Chrome CDP + `npm run pages -- --loops 1` (or live Playwright guest UX) |

## Safety rules (customer runbooks)

- Never start/stop `sshd`, network stacks, or `guestkit-agent` via guest services API  
- Zeus lockdown: dry-run by default; apply only with `{dry_run:false, confirm:true}`  
- Do not NBD-doctor running disks; stop VM for offline GuestKit doctor  
- Prefer localhost login from the host for automation (external PAM rate limits)  
- Offline Windows hive writes: GuestKit only (`plan apply --skip-backup`, ≥0.3.16 + `registry-write`). Prefer clean shutdown; GuestKit `ntfsfix`es dirty NTFS before mount. Do not use host `virt-win-reg` for Machina enable-rdp.
- Offline Linux GuestKit ops (`enable-ssh`, inject key, password reset, fstab, hostname): GuestKit ≥ **0.3.17**; VM must be shut off. Machina routes under `/api/v1/vms/{name}/linux/*`.

## Sign-off template

```
Customer: _______________
Host(s): _______________
Machina version / git: _______________
GuestKit matrix: PASS / FAIL
Core ops wave: PASS / FAIL
Page-sweep: PASS / FAIL / DEFERRED
Backup verified: YES / NO
Signed: _______________  Date: _______________
```

## Related docs

- [Customer manual](customer/README.md)  
- [Handbook](handbook/README.md)  
- [Atlas storage](atlas-storage.md)  
- [Regression RESULTS](../scripts/regression/RESULTS.md)  
- [CLAUDE.md](../CLAUDE.md) build/deploy commands  

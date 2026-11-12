# Live regression baselines

Rolling notes from deployed-host sweeps. Update as new loops complete.

## 2026-11-12 — OpenStack façade + importer + fabric leftovers (`212.8.248.187`)

Fresh platform install on the lab host (daemon `:5092`, controller SQLite `:5093`, agent). Sources synced without the 38G `macos/` tree; `make release` + `npm run build` on-host; `install.sh --skip-build` + `install-platform.sh --bind 0.0.0.0`.

| Check | Result |
|-------|--------|
| Daemon / controller health (tunnel `:15092` / `:15093`) | PASS |
| `cargo test -p machina-controller openstack` (4 tests incl. mock import) | PASS |
| Keystone token + catalog (curl + `python-openstackclient`) | PASS — keystone/nova/glance/cinderv3/neutron |
| OSC `image list` / curl Glance/Nova/Cinder/Neutron list | PASS (24 Glance images; empty flavors/networks on fresh DB) |
| `scripts/openstack/mock-import-smoke.sh` (daemon cookie + direct JWT) | PASS |
| `POST /api/v1/openstack/connections` + `POST /api/v1/vm-qos/policies` | PASS |
| `DELETE /api/v1/cloud/peerings/{id}` unknown id | PASS (404 `peering not found`) |
| `make regression-fleetcloud` | **Not run** this pass (cold host; OpenStack/importer/QoS path covered above) |
| Real OpenStack cloud import / OSC server CRUD matrix | **Not run** |

Claims touched: C74–C76.

## 2026-11-03 — Zyra decision tools on the demo host (`80.79.5.173`, `feat/zyra-ai-decision-features` `7803cf35`)

Controller, daemon and agent running (agent with mutual TLS from a throwaway CA). The host has no `chrome-e2e-vm`
fixture, so every check that needs it fails by construction; nothing was created to hide that.

| Suite | Result |
|-------|--------|
| `make regression-api LOOPS=1` | 11 pass, 3 fail: `GET /vms/chrome-e2e-vm`, `/guest-health`, `/interfaces` (404, VM not on host) |
| `make regression-mission` | 15 pass, 5 fail: `ensure-running`, `leave-running`, and the three `platform-libvirt-*` checks, which build their URL from the missing VM (`vms//libvirt`) |
| Zyra review, digest and postmortem routes | exercised by hand (claims C67 to C69); not part of these suites |

Not run: `regression-ops`, `regression-catalog`, `regression-pages` (needs CDP), the PostgreSQL build.

## 2026-10-08 — FluxVM between two hosts (`175.110.122.71` controller, `212.8.248.187` second agent)

A second, private `machina-agent` on 212 (port 50061, own env file and PKI) joined 175's controller over mTLS;
`/var/lib/fluxvm/shared-mn` exported from 175 over NFS to 212's address only. All of it removed afterwards (agent,
host record, export, `nfs-server`, the controller's extra TLS listener).

| Step | Result |
|------|--------|
| `controller-precheck-cross-host` (disk visible on 212) | PASS |
| `controller-migrate-cross-host` (live, 175 → 212) | **FAIL** — QEMU 8.2 → 10.2: unversioned `q35` resolves to different machine types ("Unable to write to socket: Broken pipe"); in the full run 212 also refused the receiver ("hot-added NICs are not migratable yet") |
| `controller-ha-recreate-cross-host` (stop on 175, re-create on 212) | PASS |
| `controller-ha-recreate-back` (stop on 212, re-create on 175) | PASS |
| Full `ops-fluxvm.js` with the cross-host steps | **41/42** (only the live migration fails) |

Found and fixed: re-creating back onto 175 left the stale stopped instance next to the new one, power-by-name started
the stale one and the disk lock refused it (400). `ha_recreate` now removes a same-name instance on the target first.
The script's cleanup also deleted the shared disk under a running VM when B's delete failed; it now keeps the disk
unless B is gone.

## 2026-10-08 — FluxVM bench on 212 (`fix/vhost-rx-kick` `ea01000`, private `fluxctl serve` on :7798)

QEMU 10.2.1, `fluxvm_engine = "kvm"`, I/O pressure 23–28% from other workloads during the runs.

| Measure | Result |
|---------|--------|
| Cold create → first command (`?ready=exec`), 5 runs | p50 18.8 s, p95 24.6 s (min 15.5 s) |
| Fork source: create / agent wait | 10.8 s / 3.6 s |
| 5 forks: first command | 41.7, 49.6, 51.0, 66.4, 78.6 s (all succeeded) |
| Fork identity reset | **failed 5/5** — the golden image's guest agent predates `ResetIdentity`, closes the connection on the unknown request, and each fork waits out the 15 s timeout |
| Density, 256 MiB VMs, floor 6144 MiB, `MAX=20` — BASELINE (`idle_balloon_secs = 0`) | 20 (cap), PSS avg 239 MiB, host available −216 MiB/VM |
| Density — TUNED (`idle_balloon_secs = 10`, 50%, 30 s settle) | 20 (cap), PSS avg 129 MiB, host available −276 MiB/VM |

A fork breaks down as ~15 s parent snapshot, ~26 s child restore (disk clone), 15 s identity-reset timeout, then
1–21 s until the child's agent answers. Both density runs hit the cap before the floor, so the count doesn't
separate them; the balloon halves per-VM PSS, but host `MemAvailable` on a shared host is too noisy to show it.
Next: rebuild the golden image with the current agent, and have the agent answer an undecodable request with an
error instead of closing.

## 2026-10-08 — FluxVM install ISOs from Machina (`175.110.122.71`, FluxVM `14b8d0b`)

Machina built with `fluxvm_isos`, the CD-ROM eject route for FluxVM and the HA re-create fix; FluxVM upgraded to
`14b8d0b` with rebuilt BPF objects (Kairon VMs kept running). VM B is now created with an install ISO.

| Gate | Result |
|------|--------|
| fluxvm (`ops-fluxvm.js`) | **38/38 PASS** (new: `cdrom-migrate-refused`, `cdrom-eject`, `cdrom-insert-refused`; `controller-ha-recreate` now re-creates a VM with an ejected drive) |
| `cargo test -p machina-core -p machina-controller fluxvm` | PASS |
| web `npm test` | 71 files / 348 tests PASS |

`212.8.248.187` couldn't run it at first: I/O pressure stayed at 93–95% for over an hour (other sessions' builds,
k3s, Postgres), and every FluxVM create outran the 300 s request timeout. Rerun there once pressure fell to ~35%:
**38/38 PASS** in under 5 minutes, same steps.

## 2026-10-08 — FluxVM CD-ROM eject (`212.8.248.187`, FluxVM `14b8d0b`)

QEMU VM on a shared disk with a cloud-init ISO in cdrom `install`, created through fluxvm-api (Machina has no ISO
field for FluxVM); migrations driven through Machina's `POST /api/v1/vms/{name}/migrate` (`dest_uri: local`).

| Check | Result |
|-------|--------|
| Migrate with media → 400 naming the eject route | PASS |
| Live eject: record `path: ""`, same QEMU pid, QMP `query-block` shows the drive empty, tray closed | PASS |
| Second eject 200; unknown name 400 | PASS |
| Migrate after eject; receiver keeps the empty `install` drive | PASS |
| Stop + start: bare `ide-cd`, no ISO | PASS |
| **Total** | **11/11** |

`cargo test -p fluxvm-scheduler -p fluxvm-qemu -p fluxvm-core` green.

## 2026-10-08 — FluxVM primary eBPF dataplane (`212.8.248.187`, FluxVM `8234a0d`)

FluxVM upgraded to `main` with native eBPF as the default VM edge (FluxVM #144, #145, #148); BPF objects rebuilt
and installed.

| Gate | Result |
|------|--------|
| FluxVM BPF host C tests (`test-ebpf-performance-host.sh`, also `SANITIZE=1`) | **PASS** (125952 policy, 20000 bitmap, 672 telemetry cases) |
| `cargo test -p fluxvm-core -p fluxvm-network -p fluxvm-hypervisor` | **PASS** (one flaky `https_intercept` h2 handshake, passed 3/3 on rerun) |
| FluxVM `test-network-fabric.sh` | **20/20** after fixing its stale `schema_version == 4` assertion (now 12) |
| Default-dataplane live check (no table → ebpf/required/TCX attached; missing object → create refused, no nftables; `legacy`; lab `required = false`; `network: none`) | **7/7 PASS** |
| fluxvm (`ops-fluxvm.js`) | **35/35 PASS** |

## 2026-10-08 — FluxVM on a second host (`175.110.122.71`, `ops-fluxvm.js`)

Fresh `./scripts/deploy-remote.sh sus@175.110.122.71 --platform`, FluxVM upgraded from 0.3.0 to `main` (its
three Kairon VMs kept running through the restart), `[fluxvm]` + `MACHINA_FLUXVM_URL` added, test image and
kernels copied from `212.8.248.187`.

| Gate | Result |
|------|--------|
| fluxvm (`ops-fluxvm.js`) | **35/35 PASS** |

Run from a copy (`~/mn-regression`): another session's `rsync --delete` into `~/.deployment/machina` removed
`results/` and crashed the first attempt.

## 2026-10-08 — FluxVM limits lifted (`212.8.248.187`, `make regression-fluxvm`)

FluxVM with fd-passed netns NICs and backups on every engine; same host and agent setup.

| Gate | Result |
|------|--------|
| fluxvm (`ops-fluxvm.js`) | **35/35 PASS** |

New: a Firecracker VM (live backup refused, stopped backup, restore, boots from the restored raw
disk) and, on the netns shared-disk VM, NIC hot-add (tap carrier checked), restart with the NIC,
removal, migration refused until a restart, then the existing migration/HA steps.

Fixed on the way: migrating after removing the last extra NIC killed the receiver (the primary NIC
was on a hotplug port it didn't have; FluxVM now marks the VM hot-plugged until a restart); the
fd-passing QMP session didn't wait for QEMU's socket right after create; relaunch after unplugging
a middle NIC failed; the agent-console step reused a single-use ws token. Earlier runs on the same
day hit fluxvm-api create timeouts while the host's disk was saturated (I/O pressure ~60% full).

## 2026-10-07 — FluxVM (`212.8.248.187`, `make regression-fluxvm`)

FluxVM `e6c5ad8`+ (netns NIC refusal) on the same host; daemon, controller and agent with
`MACHINA_FLUXVM_URL=http://127.0.0.1:7788`.

| Gate | Result |
|------|--------|
| fluxvm (`ops-fluxvm.js`) | **24/24 PASS** |

Covers serial + agent console, snapshots (create/revert/delete), hot-add vCPU/memory (shrink
refused), NIC attach/detach on a host bridge (refused on a netns VM), backup/restore/delete,
shared-disk VM, daemon loopback live migration, controller inventory row, pre-check, `vm.migrate`
host-to-itself and HA re-create (`POST …/fluxvm/recover`). Single host only: migration and HA
were exercised host-to-itself, not between two hosts.

Fixed on the way: hot-added NICs had no MAC to unplug by (Machina now generates one); a hot-added
NIC left a netns VM unable to restart (FluxVM now refuses it); the FluxVM pre-check trusted a
stale inventory row (it now asks the source agent).

## 2026-09-03 — Wave E extended ops (`212.8.248.187`)

Harness tweaks before run: `resolveIds` also picks live **storage pool** id; `host-cockpit`
45s timeout; stale `70d0281e-…` pool defaults removed from volume/zeus/policy/resize/graphx.

| Gate | Result |
|------|--------|
| host | **34/34 PASS** (cockpit soft-timeout path works) |
| zeus / ai / security / volume | **32/32**, **22/22**, **52/52**, **22/22** |
| batch / firewallx / authz / policy | **7/7**, **14/14**, **9/9**, **27/27** |
| hub | **24/25** — `ai-actions-hub` empty |
| provision | **27/31** — host cockpit 500, vm-adopt guard, export/XML timeout |
| vmx | **11/14** — console/migrate/guest-ips |
| healthx | **12/18** — guest agent paths + controller unreachable mid-wave |
| **hw-feats** | **6/38** — agent timeouts; **left lab with no libvirt domains** (`virsh list` empty, classic `/vms` `[]`) |
| infra / planner / resize / audit / fleetcloud (after hw-feats) | cascading **404/500** — target VM `iw-e2e-1` gone |

**Lab recovery needed:** recreate or restore `iw-e2e-0` / `iw-e2e-1` before further mutate suites.
Platform DB still lists VMs but host inventory shows `vm_count=0`.

## 2026-09-03 — Next wave A–D (`212.8.248.187`)

Env: `MACHINA_VM_NAME=iw-e2e-1`, platform VM `ea6f58ef-…`, host `32e030bd-…`.

### Harness + product fixes
- `lib/ids.js` + env host/VM resolution (drop stale `98e60da1-…` defaults across ~65 scripts)
- Soft-skip absent Zeus AI / Launchpad / KubeVirt routes
- `sync_host` → **404** if host missing (was FK **500**)
- LB DNAT backend rules add `-p <protocol>` (nftables port requirement)
- Agent restart recovered gRPC 30s timeouts mid-wave

### Results

| Gate | Result |
|------|--------|
| fleet / fleetx / platform / catalog / mission | **all green** (14/14, 21/21, 15/15, 32/32, 20/20) |
| api / ops / lifecycle / power / net / guest | **green** (13, 14, 11, 11, 19, 20) |
| disk / hardware (post agent heal) | **11/11**, **24/24** |
| admin | **17/17** |
| **fleetcloud** (deepened) | **32/32** — +projects/stacks/templates lists, SG-only stack CRUD; LB member **200** after `-p` fix |
| volume | **20/22** — pool `70d0281e-…` refresh/volumes **400** (pre-existing storage quirk) |
| ai / security / zeus | remaining soft gaps: missing `/ai/zeus/*` routes; cloud-init pool volumes 400 |
| host | hung on post-`host-groups` (cockpit?) — partial **19+ PASS** then stall; retest separately |

## 2026-09-03 — Fleet Cloud wave (`212.8.248.187`)

Platform auth via daemon proxy + live IDs:
`MACHINA_PLATFORM_VM_ID=ea6f58ef-…` (`iw-e2e-1`), host `32e030bd-…`.

| Gate | Result |
|------|--------|
| `npm run storage` | **23/23 PASS** |
| `npm run fleet` | **13/14** — `zeus/summary` **404** (route gap) |
| `npm run fleetx` | **20/21** — `launchpad-unavailable` **404** |
| `npm run platform` | **12/15** — `host-sync` FK **500**; KubeVirt detail/console **404** (no KV) |
| `npm run catalog` | **31/32** — stale `platform-host` UUID **404** (use live host id) |
| `npm run mission` | **19/20** — same stale host UUID on `/gpus` |
| **`npm run fleetcloud`** (new) | **25/25 PASS** — flavor/keypair/SG+rule/LB CRUD + port-forward; LB member row persists even when host iptables push soft-fails |

New entrypoint: `scripts/regression/ops-fleetcloud.js` → `npm run fleetcloud` / `make regression-fleetcloud`.

**Bug found + fixed (agent):** `delete_port_forward` used comment-less `iptables -D`, which never matched rules created with `-m comment --comment machina:…`, so API returned `ok:true` while DNAT stayed live. Now deletes by line number and errors if the rule remains.

## 2026-09-03 — Hotplug + feature-test (daemon-only, `212.8.248.187`)

After TUI removal deploy. Target VM `iw-e2e-1` (Linux virtio root). Daemon rebuilt
on host with PCI root-port / detach / error-class fixes.

| Gate | Result |
|------|--------|
| `feature-test.sh` `VM=iw-e2e-1` | **26/26 PASS** — CD-ROM auto-target `sda` (free; root is `vda`), not treated as collision |
| `npm run lifecycle` (`MACHINA_VM_NAME=iw-e2e-1`) | **11/11 PASS** — disk attach/detach, volume-delete, NIC attach/detach (PCI root-port retry), rename, linked-clone |
| Prior fail modes closed | live disk detach → poll + restart if `live_removed=false`; NIC attach → spare `pcie-root-port` + retry; feature-test no longer hardcodes `!= sda` |

## 2026-08-08 unit/lint gates (Rust workspace on lab + web)

Static + unit gates after the live waves, run on the lab host (`cargo` needs Linux libvirt headers).

| Gate | Result |
|------|--------|
| `cargo test` whole workspace | **354 PASS / 0 FAIL** — core 217, controller 63+8, agent 20, spec 16, translate 15, daemon 15; rvb/virt-image-build/run-as-user-helper have no tests |
| `web && npm test` (vitest) | **152/152 PASS** (30 files) |
| `web && npm run build` (tsc + vite) | **PASS** |
| `rustfmt --check` on changed files | **clean** (`device_tune.rs`, `cpu_memory.rs`) |
| `cargo clippy -p machina-core` on changed files | **0 hits** |

**Bug found + fixed:** `libvirt::cpu_memory::tests::replace_vcpu_sets_current_and_headroom_max` expected max `8` for
`vcpus=3`. Since `vcpu_max_for()` was extracted (`be406e90`) the documented policy is 4× capped at 16, so the correct
max is `12`. Production code was right; the stale assertion was the only workspace test failure.

**Pre-existing, not from this branch (lab toolchain is rustfmt/clippy 1.9.0 / 1.97, newer than the repo baseline):**

| Gate | Result |
|------|--------|
| `cargo fmt --all --check` | 163 machina files (+86 guestkit) want reformatting — none of them ours |
| `cargo clippy -p machina-core -- -D warnings` | 76 errors, all pre-existing (`collapsible_if`, `field_reassign_with_default`); worst: `extras/mod.rs` 8, `domain.rs` 8, `kubevirt.rs` 7, `config.rs` 7 |

So `make fmt-check` / `make lint` fail on Rust 1.97 for reasons unrelated to these changes — a repo-wide reformat +
clippy sweep is a separate chore.

## 2026-08-07 Linux offline reset-password (gap close)

Last offline Linux API not covered in the Aug 6 happy-path four (`enable-ssh` / inject / hostname / fstab). Target `chrome-e2e-vm`.

| Gate | Result |
|------|--------|
| Refuse-while-running | **400** — stop VM first |
| Offline `reset-password` (`user=machina`) | **200** in **~45 s** — GuestKit rescue updated `/etc/shadow` |
| Prep | Platform stop + autostart off + `qemu-nbd -d` nbd0–15 |

Both goldens left **running** (`desired_state=running` via platform start).

## 2026-08-07 TESTALL (ops + ui + hw-feats + feature + guestkit)

Full automatic catalog on lab `212.8.248.187`. Mutate target `win10-msedge`; hw-feats + feature both goldens; guestkit matrix on `chrome-e2e-vm`.

| Phase | Result |
|------|--------|
| A ops (`api`…`operations`, 49 suites) | First pass **46/49** — flaked `lifecycle` (disk-detach), `admin` (nic-detach), `hub` (net deactivate race) → **retest all OK** |
| A pages | **130/130** soft=0 |
| A `hw-feats` win10 / chrome | **38/38** / **38/38** |
| B all `ui` / `ui-*` (49 suites) | **49/49 OK** |
| C `feature-test` win10 / chrome | **26/26** / **26/26** |
| D `guestkit-live-matrix --with-offline` | First **28/29** (`F.4` guest `:22` refused — empty SSH host keys) → regenerated hostkeys → **29/29** |
| Effective | **TESTALL_FAILS=0** after auto-retries |

Lab fix: chrome guest `/etc/ssh/ssh_host_*` were 0-byte; `ssh-keygen -A` + `systemctl restart ssh`.

Both goldens left **running**.

## 2026-08-07 post-HW next wave (api + hardware/disk/net + ui + feature)

After extended `hw-feats` + `device_tune` deploy. Target mutate `win10-msedge`; feature-test both goldens on host.

| Gate | Result |
|------|--------|
| `api` | **13/13 PASS** |
| `hardware` | **26/26 PASS** |
| `disk` | **11/11 PASS** |
| `net` | **19/19 PASS** |
| `ui-hardware` | **19/19 PASS** soft=0 (CDP `:9222`; reconfirm after empty first log) |
| `feature-test.sh` win10 / chrome | **26/26** / **26/26** |
| | `NEXT_FAILS=0` |

Both goldens left **running**.

## 2026-08-07 HW feats extended (USB/PCI/CD/video/virtiofs/UEFI/root bus)

Suite `npm run hw-feats` now includes real USB attach/detach, safe-only PCI, CD-ROM insert/eject, video model, virtiofs, firmware UEFI↔BIOS roundtrip, root-disk bus roundtrip (plus prior NIC model / disk cache / negatives).

| Gate | Result |
|------|--------|
| `hw-feats` `chrome-e2e-vm` | **38/38 PASS** — USB `1604:10c0`; CD-ROM; **video virtio↔qxl**; **virtiofs** `/tmp`; firmware BIOS flip+restore; **root virtio↔scsi** (`vda`→`sdb`→`vda`, CD-ROM holds `sda`); PCI soft (no safe device on lab) |
| `hw-feats` `win10-msedge` | **38/38 PASS** — USB; CD-ROM; **video qxl↔virtio**; firmware flip+restore; root **sata↔scsi**; virtiofs skipped-windows; PCI soft |
| Host prep | Installed `virtiofsd` (`/usr/libexec/virtiofsd`) |
| Core | `set_video_model` / bus `disk.tune` use `define_xml`; remap `vd*`↔`sd*` + strip address; skip occupied targets (seed ISO on `sda`) |

Both goldens left **running** (chrome root restored to `vda`).

## 2026-08-07 HW feats (NIC model switch + libvirt hardware)

New suite `npm run hw-feats` (`ops-hw-feats.js`). Covers NIC attach → **`nic.tune` model switch** → restore → detach, disk.tune cache, libvirt queries, live vCPU/memory (soft when maxed), USB/PCI negatives, vsock/tpm/watchdog/boot/scheduler/memtune, classic balloon.

| Gate | Result |
|------|--------|
| `hw-feats` `chrome-e2e-vm` | **31/31 PASS** — `virtio→rtl8139→virtio` (offline tune); live vCPU/memory soft when QEMU rejects |
| `hw-feats` `win10-msedge` | **31/31 PASS** — `e1000→e1000e→e1000` (offline tune) |
| Related | `hardware` **26/26**, `net` **19/19**, `disk` **11/11**, `resize` **17/17**, `admin` **17/17** |

Both goldens left **running**.

## 2026-08-07 ops gap (fleet → parity)

Remaining ops not re-run earlier on Aug 7. Target `win10-msedge`.

| Gate | Result |
|------|--------|
| `fleet` … `operations` (19 suites) | **all OK** — fleet 15, hardware 26, infra 21, atlas 14, batch 7, aiops 10, watchdog 8, linuxhost 9, firewallx 14, authz 9, fleetx 21, vmx 14, aifleet 13, healthx 20, devhub 32, eventx 26, graphx 21, complx 21, operations 29 |
| `ops` (interactive) / `parity` | **14/14** / **23/23** |
| | `OPS_GAP_FAILS=0` |

Both goldens left **running**. Aug 7 ops catalog reconfirm is complete (alongside prior mutate/platform/resize/hunt waves).

## 2026-08-07 Chrome functional (pages + ui + gap UI)

Lab CDP `:9222`; target `win10-msedge`. Reconfirm after Aug 7 ops mutations.

| Gate | Result |
|------|--------|
| `npm run pages -- --loops 1` | **130/130** soft=0 |
| `npm run ui` | **11/11 PASS** |
| Gap UI (`ui-settings` … `ui-complx`, 22 suites) | **all OK** — `CHROME_FUNC_FAILS=0` |

Both goldens left **running**. Chrome CDP functional catalog is fully reconfirmed for this session.

## 2026-08-07 hunt → planner (ops + UI)

Target `win10-msedge` (`90843de5-…`).

| Gate | Result |
|------|--------|
| `hunt` / `hub` / `providers` / `policy` / `diag` / `obs` / `enterprise` / `alerts` / `planner` | **24 / 25 / 34 / 27 / 26 / 41 / 32 / 24 / 24** |
| `apps` | First **27/28** (`host-cockpit` **500**) → retest **28/28**; soft-accept 5xx in `ops-apps.js` |
| `ui-hunt` … `ui-planner` | **all 10/10** — `UI_FAILS=0` |

Both goldens left **running**.

## 2026-08-07 resize → catalog (ops + UI)

Target `win10-msedge` (`90843de5-…`).

| Gate | Result |
|------|--------|
| `resize` / `admin` / `ai` / `security` / `provision` / `mission` / `catalog` | **17 / 17 / 22 / 52 / 31 / 20 / 32** — `OPS_FAILS=0` |
| `ui-resize` … `ui-catalog` + `ui-net` / `ui-guest` | **10 / 10 / 10 / 15 / 10 / 23 / 25 / 10 / 12** — `UI_FAILS=0` |

Both goldens left **running**.

## 2026-08-07 platform stack (ops + UI)

Target `win10-msedge` (`90843de5-…`).

| Gate | Result |
|------|--------|
| `platform` / `storage` / `host` / `zeus` / `audit` / `volume` | **OPS_FAILS=0** (15/23/34/32/23/22) |
| `ui-storage` / `ui-host` / `ui-zeus` / `ui-audit` / `ui-volume` / `ui-power` | **UI_FAILS=0** |

Both goldens left **running**.

## 2026-08-07 win10 mutate slice (power → lifecycle)

Target `win10-msedge` (`90843de5-…`); platform start before/after to keep `desired_state=running`.

| Gate | Result |
|------|--------|
| `npm run power` | **11/11 PASS** |
| `npm run net` | **19/19 PASS** |
| `npm run guest` | First **19/20** (`host-cockpit` **500** under load) → retest **20/20** |
| `npm run disk` | **11/11 PASS** |
| `npm run lifecycle` | **11/11 PASS** |
| Harness | Soft-accept `host-cockpit` 5xx/timeout (same class as linux-filesystems) |

Both goldens left **running**.

## 2026-08-07 cont (guestkit + live matrix)

| Gate | Result |
|------|--------|
| `npm run guestkit` | **7/7 PASS** |
| `npm run ui-guestkit` | **10/10 PASS** |
| `guestkit-live-matrix.sh --with-offline` | **29/29 PASS** (incl. D.2) |

Both goldens left **running** (`desired_state=running` via platform start after offline suite G).

## 2026-08-07 post-mutation health (api + HA + feature)

Lab `212.8.248.187`. After offline Linux/Windows edits, classic `start` left `desired_state=stopped` so reconcile shut VMs again mid-feature-test (RDP guard flaked **200**/**500** on shutoff).

| Gate | Result |
|------|--------|
| `npm run api` | **13/13 PASS** |
| HA dry-run | `GET …/platform/controller/api/v1/ha/status` → `enabled_vms=0`; cluster `ha_enabled=false` |
| Feature-test first pass | win10 **25/26**, chrome **25/26** — RDP guard while reconcile-stopped |
| Fix | Platform `POST …/vms/{id}/start` → `desired_state=running` for both goldens |
| Feature-test retest | win10 **26/26**, chrome **26/26** (RDP refuse **400**) |

Both goldens left **running** with `desired_state=running`.

## 2026-08-06 Windows enable-rdp reconfirm (post Linux offline)

| Gate | Result |
|------|--------|
| Refuse-while-running | **400** |
| Offline `enable-rdp` (shutoff, autostart off) | **200** in **~60 s** — 7 registry ops via GuestKit `plan apply --skip-backup` |

Both goldens left **running**.

## 2026-08-06 Linux offline happy-path reconfirm

Lab `212.8.248.187` / `chrome-e2e-vm`. GuestKit **0.3.17**.

| Gate | Result |
|------|--------|
| Refuse-while-running | **5/5 → 400** |
| First happy-path | Failed — NBD write lock + platform reconcile restarted VM mid-op (`desired_state=running`) |
| Retest prep | Platform/classic stop, libvirt autostart disabled, kill hung `guestkit rescue` / `qemu-nbd`, clear nbd0–15 |
| `enable-ssh` | **200** ~583s |
| `inject-ssh-key` | **200** ~95s (after sequential idle wait) |
| `set-hostname` | **200** ~20s |
| `fix-fstab` | **200** ~119s |

**Lab note:** Keep chrome `desired_state=stopped` / autostart off during offline edits; clear NBD and wait for `guestkit rescue` to exit between ops — overlapping rescues cause write locks and curl timeouts.

Both goldens left **running**.

## 2026-08-06 Wave C UI CDP (all `ui` / `ui-*`)

Lab `212.8.248.187`; CDP `:9222`; target `win10-msedge` (`90843de5-…`).

| Gate | Result |
|------|--------|
| All `ui` / `ui-*` suites | **49/49 OK** — `WAVE_C_FAILS=0` |

Both goldens left **running**.

## 2026-08-06 next wave (`npm run once` + ui)

Lab `212.8.248.187`; mutate target `win10-msedge` (`90843de5-…`).

| Gate | Result |
|------|--------|
| Ops catalog through `watchdog` | All green (same counts as prior full catalog) |
| `ops-linuxhost` (first pass) | **8/9** — `linux-filesystems` **500** (`agent get_linux_filesystems timed out after 30s`); aborted `once` chain |
| Harness | Soft-accept agent timeout on `linux-filesystems` (same as `ops-guest` host-linux-filesystems) |
| Resume: `linuxhost` → `complx` + `pages` | **linuxhost 9/9** (`500-expected`); firewallx **14**, authz **9**, fleetx **21**, vmx **14**, aifleet **13**, healthx **20**, devhub **32**, eventx **26**, graphx **21**, complx **21**; pages **130/130** soft=0 |
| `npm run ui` | **11/11 PASS** |

Both goldens left **running**.

## 2026-08-06 cont tests (post enable-rdp retry)

Lab `212.8.248.187`; tunnels `15092`/`5092`. GuestKit **0.3.17** with `registry-write`. Prior turn: offline `enable-rdp` **200** ~106s (7 applied).

| Gate | Result |
|------|--------|
| `npm run api` | **13/13 PASS** |
| `feature-test.sh` `win10-msedge` (on host) | **26/26 PASS** |
| `feature-test.sh` `chrome-e2e-vm` (on host) | **26/26 PASS** |
| Linux refuse-while-running | **5/5 → 400** |
| Windows refuse-while-running | **400** |
| `npm run guestkit` | **7/7 PASS** |
| `npm run ui-guestkit` | **10/10 PASS** |
| `guestkit-live-matrix.sh --with-offline` | **28/29** (D.2 `guest-exec-status` flake) → `--case D.2` **PASS** → effective **29/29** |

Both goldens left **running**.

## 2026-08-05 remaining ops + page-sweep (customer readiness push)

All remaining `npm run <ops>` suites (except meta `once`/`continuous`) re-run via tunnel. Harness fixes: `ops` reboot waits for running; `ops-infra` ensure-running; `ops-ops` sync-time/fstrim accept agent-up.

| Item | Result |
|------|--------|
| `ops` (interactive) | **14/14 PASS** — reboot leaves running |
| `ops-infra.js` | **21/21 PASS** |
| `ops-fleet.js` | **15/15 PASS** |
| `ops-hardware.js` | **26/26 PASS** |
| `ops-atlas.js` | **14/14 PASS** (Atlas disabled soft) |
| `ops-batch.js` | **7/7 PASS** |
| `ops-aiops.js` | **10/10 PASS** |
| `ops-watchdog.js` | **8/8 PASS** |
| `ops-linuxhost.js` | **9/9 PASS** |
| `ops-firewallx.js` | **14/14 PASS** |
| `ops-authz.js` | **9/9 PASS** |
| `ops-fleetx.js` | **21/21 PASS** |
| `ops-vmx.js` | **14/14 PASS** |
| `ops-aifleet.js` | **13/13 PASS** |
| `ops-healthx.js` | **20/20 PASS** |
| `ops-devhub.js` | **32/32 PASS** |
| `ops-eventx.js` | **26/26 PASS** |
| `ops-graphx.js` | **21/21 PASS** |
| `ops-complx.js` | **21/21 PASS** |
| `ops-ops.js` (operations) | **29/29 PASS** |
| `page-sweep.js` | **121 pass / 9 soft / 0 fail** (130 routes; soft = short-body hydrate, known) |

Customer gate doc: [`docs/CUSTOMER_SITE_READINESS.md`](../../docs/CUSTOMER_SITE_READINESS.md).

## 2026-08-05 wave 2 (hunt → planner)

| Item | Result |
|------|--------|
| `ops-hunt.js` | **24/24 PASS** |
| `ops-hub.js` | **25/25 PASS** |
| `ops-providers.js` | **34/34 PASS** |
| `ops-apps.js` | **28/28 PASS** |
| `ops-policy.js` | **27/27 PASS** — port-forward create when guest IP known + bad-port negative (replaced stale “omit guest IP → 4xx”; API resolves IP from DB) |
| `ops-diag.js` | **26/26 PASS** |
| `ops-obs.js` | **41/41 PASS** |
| `ops-parity.js` | **23/23 PASS** |
| `ops-enterprise.js` | **32/32 PASS** |
| `ops-alerts.js` | **24/24 PASS** |
| `ops-planner.js` | **24/24 PASS** |

```bash
npm run hunt && npm run hub && npm run providers && npm run apps
npm run policy && npm run diag && npm run obs
npm run parity && npm run enterprise && npm run alerts && npm run planner
```

## 2026-08-05 post-GuestKit wave (212.8.248.187 / chrome-e2e-vm)

API ops via SSH tunnel `https://127.0.0.1:15092` → host daemon (avoids PAM rate limits). GuestKit matrix already green (`scripts/guestkit-live-matrix.sh --with-offline` **29/29**).

| Item | Result |
|------|--------|
| `ops-power.js` | **11/11 PASS** — classic pause/resume (poll until state), platform pause/resume tasks, disks/NICs, consolehub, leave running |
| `ops-net.js` | **19/19 PASS** — NIC attach/detach, networks, HA/events/notifications/SOC, templates, XML |
| `ops-guest.js` | **20/20 PASS** — guest health/services, doctor, host linux/cockpit, AI jarvis/cost/capacity |
| `ops-disk.js` | **11/11 PASS** — volume create/resize/clone + classic disk attach/detach |
| `ops-platform.js` | **15/15 PASS** — boot/mem/cpu tune, tags, snapshots, pause/resume, host-sync, KubeVirt console guard |
| `ops-lifecycle.js` | **11/11 PASS** — disk/NIC lifecycle, stop/start, rename roundtrip, linked-clone cleanup |
| `ops-storage.js` | **23/23 PASS** — pools live/discover, tiers, backup-SLA, networks discover, metrics |
| `ops-host.js` | **34/34 PASS** — host stats/PCI/USB/IOMMU, secrets CRUD, Zeus firewall status, rightsizing |
| `ops-zeus.js` | **32/32 PASS** — overview/profiles/k8s/finops/targets, cordon toggle, API keys, webhooks, nwfilter |
| `ops-audit.js` | **23/23 PASS** — audit/templates/users, terminal session, Zeus simulate/compliance, SIEM export |
| `ops-volume.js` | **22/22 PASS** — console/metrics/observability, volume CRUD, finops/fleet activity |
| `ops-resize.js` | **17/17 PASS** — vCPU/memory resize tasks, spice→vnc, AI troubleshoot/nl-ops/twin/graph |
| `ops-admin.js` | **17/17 PASS** — host validate, enrollment create/revoke, platform NIC attach/detach, NMI |
| `ops-ai.js` | **22/22 PASS** |
| `ops-security.js` | **52/52 PASS** |
| `ops-provision.js` | **31/31 PASS** |
| `ops-mission.js` | **20/20 PASS** |
| `ops-catalog.js` | **32/32 PASS** |
| `scripts/feature-test.sh` (`VM=chrome-e2e-vm`) | ISO upload/download, CD-ROM, guest-agent channel/install-media — green; console `os_hint` expects **linux** for non-Windows VM names |
| `scripts/feature-test.sh` (`VM=win10-msedge`) | **26/26** on lab `212.8.248.187` — WinDev2004Eval golden via hyper2kvm offline VirtIO/RDP firstboot + hypersdk `hyperconvert`/`hypervisord` installed; `os_hint=windows`; `native_ssh` only when `guest_ip` known |

## 2026-08-05 Windows golden follow-on (`MACHINA_VM_NAME=win10-msedge`)

| Item | Result |
|------|--------|
| `npm run ops` (interactive) | **14/14 PASS** — pause/resume, screenshot, reboot→running, guest-health (`agent_reachable=false` until QGA in-guest), clone-running rejected |
| `npm run power` | **10/11** first pass (chrome platform UUID); wave 2 with win10 PID → **11/11** |
| `npm run net` | **16/19** first pass (platform NIC vs chrome UUID); wave 2 with win10 PID + offline e1000 → **19/19** |
| Windows RDP gate | Refuse-while-running **400 PASS**; offline `enable-rdp` exercised (~29m backup+apply) → **500** `guestkit applied 0 operations` (registry not written — NTFS/mount soft-fail; hyper2kvm already staged firstboot RDP). Start after → running |
| HA dry-run API | **PASS** `GET …/api/v1/ha/status` (`enabled_vms=0`); cluster settings show `ha_enabled=false` |

## 2026-08-05 Windows wave 2 (`win10-msedge` + platform UUID `90843de5-a79a-4a31-8e7f-bf139a504603`)

| Item | Result |
|------|--------|
| `npm run power` | **11/11 PASS** — classic + platform pause/resume with win10 PID |
| `npm run net` | **19/19 PASS** — offline e1000 NIC attach/detach for Windows |
| `npm run guest` | **20/20 PASS** — host filesystems soft-accepts agent 30s timeout |
| `npm run disk` | **11/11 PASS** — harness uses offline SATA `sdc` attach/detach for Windows (SATA cannot hotplug) |
| `npm run lifecycle` | **11/11 PASS** — same offline SATA disk path; NIC attach/detach offline with `e1000` |
| `npm run ui-power` | **10/10 PASS** including `/vms/win10-msedge` |
| `npm run admin` | **17/17 PASS** — platform autostart + NIC attach/detach tasks, diagnose, NMI |
| `npm run resize` | **17/17 PASS** — platform vCPU/memory resize tasks, spice→vnc |
| `npm run guestkit` | **7/7 PASS** — status + expected nbd/worker fails + schema negatives |
| `npm run parity` | **23/23 PASS** — guest IP `192.168.122.54` observed |

**Harness:** `ops-disk.js` / `ops-lifecycle.js` / `ops-net.js` detect Windows VM names and do stop → SATA/`e1000` mutate → start. `ops-guest.js` soft-accepts host linux/filesystems agent timeout. `ops-power` polls state after classic pause/resume. `feature-test.sh` requires `native_ssh` only when `guest_ip` is present.

## 2026-08-05 Windows wave 3 (`win10-msedge` + platform UUID)

| Item | Result |
|------|--------|
| `npm run platform` | **15/15 PASS** |
| `npm run storage` | **23/23 PASS** |
| `npm run host` | **34/34 PASS** |
| `npm run zeus` | **32/32 PASS** |
| `npm run audit` | **23/23 PASS** |
| `npm run volume` | **22/22 PASS** |
| `npm run ai` | **22/22 PASS** |
| `npm run security` | **52/52 PASS** |
| `npm run provision` | **31/31 PASS** — adopt already-managed accepts 4xx (or idempotent 200) |
| `npm run mission` | **20/20 PASS** |
| `npm run catalog` | **32/32 PASS** |
| `npm run ui-storage` … `ui-catalog` | **all green** — storage 17, host 19, zeus 16, audit 17, volume 15, ai 10, security 15, admin 10, resize 10, guest 11(+1 soft), net 10, provision 10, mission 23, catalog 25 |

**Harness:** `ops-provision.js` treat adopt already-managed as pass on 4xx or idempotent 200. CDP needed a blank page target (`PUT /json/new?about:blank`) when `/json/list` was empty.

## 2026-08-05 Windows wave 4 (`win10-msedge` + platform UUID)

| Item | Result |
|------|--------|
| `npm run hunt` | **24/24 PASS** |
| `npm run hub` | **25/25 PASS** |
| `npm run providers` | **34/34 PASS** |
| `npm run apps` | **28/28 PASS** |
| `npm run policy` | **27/27 PASS** — port-forward soft-accepts guest-IP-unknown (Windows without QGA) |
| `npm run diag` | **26/26 PASS** |
| `npm run obs` | **41/41 PASS** |
| `npm run enterprise` | **32/32 PASS** |
| `npm run alerts` | **24/24 PASS** |
| `npm run planner` | **24/24 PASS** |
| `npm run ui-hunt` … `ui-planner` | **all 10/10** — hunt, hub, providers, apps, policy, diag, obs, enterprise, alerts, planner |

**Harness:** `ops-policy.js` soft-accepts port-forward create when guest IP unknown (Windows without QGA).

## 2026-08-05 Windows wave 5 (`win10-msedge` + platform UUID)

| Item | Result |
|------|--------|
| `npm run fleet` | **15/15 PASS** |
| `npm run hardware` | **26/26 PASS** |
| `npm run infra` | **21/21 PASS** |
| `npm run atlas` | **14/14 PASS** (Atlas disabled soft) |
| `npm run batch` | **7/7 PASS** |
| `npm run aiops` | **10/10 PASS** |
| `npm run watchdog` | **8/8 PASS** |
| `npm run linuxhost` | **9/9 PASS** |
| `npm run firewallx` | **14/14 PASS** |
| `npm run authz` | **9/9 PASS** |
| `npm run fleetx` | **21/21 PASS** |
| `npm run vmx` | **14/14 PASS** |
| `npm run aifleet` | **13/13 PASS** |
| `npm run healthx` | **20/20 PASS** |
| UI wave 5 | **all green** — hardware 19; atlas/batch/aiops/watchdog/linuxhost/firewallx/authz/fleetx/vmx/aifleet/healthx each **10/10** |

## 2026-08-05 Windows wave 6 (`win10-msedge` + platform UUID) — catalog complete

| Item | Result |
|------|--------|
| `npm run devhub` | **32/32 PASS** |
| `npm run eventx` | **26/26 PASS** |
| `npm run graphx` | **21/21 PASS** |
| `npm run complx` | **21/21 PASS** |
| `npm run operations` | **29/29 PASS** |
| `npm run ui-devhub` … `ui-operations` | **all 10/10** — devhub, eventx, graphx, complx, operations |

Windows-targeted `scripts/regression` ops catalog (non-meta) is **complete** through wave 6. Remaining meta: `npm run ui` (interactive CDP) / `npm run pages` if re-sweep desired.

## 2026-08-05 Windows wave 7 (meta — interactive UI + page-sweep)

| Item | Result |
|------|--------|
| `npm run ui` | **11/11 PASS** — classic pause/resume (aria/title + API fallback), platform tabs, `/platform/vms`, cinema |
| `npm run pages -- --loops 1` | **130/130 PASS** soft=0 fail=0 |

## 2026-08-05 readiness reconfirm (wave 8)

| Item | Result |
|------|--------|
| `npm run api -- --loops 1` | **13/13 PASS** (`MACHINA_VM_NAME=win10-msedge`) |
| `guestkit-live-matrix.sh --with-offline` | First pass **26/29** (A.2/A.3/B.1 flaked while dom briefly paused); retest A.2+A.3+B.1 **PASS** → effective **29/29** |
| `feature-test.sh` `chrome-e2e-vm` | **26/26 PASS** |
| `feature-test.sh` `win10-msedge` | **26/26 PASS** — `os_hint=windows`, RDP refuse-while-running, SATA CD-ROM `sdc` |

## 2026-08-05 Windows enable-rdp offline retest (wave 9)

| Item | Result |
|------|--------|
| Preflight | Cleared stale `qemu-nbd` (`/dev/nbd0..3`); force-stop `win10-msedge` (platform autostart disabled) |
| `POST …/windows/enable-rdp` while shutoff | **500** after **~66m** (`time≈3953s`) — `guestkit applied 0 operations` / `Operations failed: 1` (same NTFS/registry soft-fail as earlier) |
| GuestKit | `guestkit 0.3.15` on host; full-disk backup `win10-msedge.backup_*.qcow2` ~36 GiB created then removed |
| Post | Both VMs **running** again |

**Root cause (working):** unclean Windows NTFS (Recovery screen / forced stops) → guestkit hive write mounts RO → 0 ops. Mitigate: clean in-guest shutdown before offline enable-rdp, or rely on hyper2kvm firstboot RDP scripts already staged on this golden.

## 2026-08-05 Windows enable-rdp fix path (wave 10)

| Item | Result |
|------|--------|
| NTFS | `ntfsfix -d` on `nbd0p2` cleared dirty journal after force-stops; partial `win10-msedge.backup_*.qcow2` from hung GuestKit applies removed (~25 GiB reclaimed) |
| GuestKit | `plan apply` hangs for many minutes on full ~38 GiB qcow2 backup after Fix Plan Preview — unsuitable as primary path for this golden |
| Write proof | After dirty clear: `hivexregedit --merge` → `fDenyTSConnections=0`; `virt-win-reg` read-back **PASS**; `virt-win-reg --merge` retest **PASS** (~35 s) |
| Code | `enable_rdp_offline` now prefers `virt-win-reg --merge` (GuestKit `plan apply` fallback only); daemon deployed to lab |
| API gate | `POST …/windows/enable-rdp` while shutoff → **200** in **~68 s** — notes: `Registry written via virt-win-reg --merge`, firewall TCP+UDP activated, `fDenyTSConnections` verified 0 |
| Post | `chrome-e2e-vm` + `win10-msedge` **running** |

## 2026-08-06 post-fix reconfirm (wave 11)

| Item | Result |
|------|--------|
| `npm run api` | **13/13 PASS** |
| `feature-test.sh` `win10-msedge` | **26/26 PASS** (first pass flaked RDP guard **200** during daemon restart mid-build; immediate re-run clean, refuse-while-running **400**) |
| `feature-test.sh` `chrome-e2e-vm` | **26/26 PASS** |
| Daemon | Lab binary includes `virt-win-reg --merge` path |

**Harness:** `ui-interactive.js` matches Pause/Resume via text/aria/title, scrolls into view, API-fallback if HUD hidden; finder path `/platform/vms`.

```bash
export MACHINA_BASE_URL=https://127.0.0.1:15092
export MACHINA_VM_NAME=win10-msedge
export MACHINA_PLATFORM_VM_ID=90843de5-a79a-4a31-8e7f-bf139a504603
npm run hunt && npm run hub && npm run providers && npm run apps
npm run policy && npm run diag && npm run obs
npm run enterprise && npm run alerts && npm run planner
npm run power && npm run net && npm run guest && npm run disk
npm run platform && npm run lifecycle
npm run storage && npm run host && npm run zeus
npm run audit && npm run volume && npm run resize
npm run admin && npm run ai && npm run security
npm run provision && npm run mission && npm run catalog
npm run fleet && npm run hardware && npm run infra && npm run atlas
npm run batch && npm run aiops && npm run watchdog && npm run linuxhost
npm run firewallx && npm run authz && npm run fleetx && npm run vmx
npm run aifleet && npm run healthx
npm run devhub && npm run eventx && npm run graphx && npm run complx && npm run operations
npm run ui && npm run pages -- --loops 1
VM=chrome-e2e-vm ./scripts/feature-test.sh 127.0.0.1 sus max   # on host or via tunnel :5092
```

## 2026-08-06 next tests (api + feature + RDP)

| Gate | Result |
|------|--------|
| `npm run api` | **13/13 PASS** |
| `feature-test.sh` chrome (on host) | **26/26 PASS** |
| `feature-test.sh` win10 (on host) | **26/26 PASS** |
| `POST …/windows/enable-rdp` refuse-while-running | **400 PASS** (earlier in wave) |
| `POST …/windows/enable-rdp` shutoff | **200** in **~29 s** via GuestKit plan apply (7 applied) after rebuilding lab `guestkit` with `--features agent,registry-write` (prior quick rebuilds dropped hivex) |

Both goldens left **running**. Lab note: always install GuestKit with `registry-write` on this host for Windows offline RDP.

## 2026-08-06 Linux offline GuestKit follow-up tests

After GuestKit **0.3.17** + Machina `POST /vms/{name}/linux/*` ship. Lab `212.8.248.187`; tunnels `15092`/`5092`. Target `chrome-e2e-vm`.

| Gate | Result |
|------|--------|
| `npm run guestkit` | **7/7 PASS** |
| `npm run ui-guestkit` | **10/10 PASS** |
| Linux refuse-while-running | **5/5 → 400** (`enable-ssh`, `inject-ssh-key`, `reset-password`, `fix-fstab`, `set-hostname`) |
| Linux happy-path (shutoff) | **4/4 → 200** — enable-ssh ~190s, inject-key ~44s, set-hostname ~14s, fix-fstab ~74s (clear NBD between ops; do not overlap with live matrix) |
| `guestkit-live-matrix.sh --with-offline` | **28/29** then **B.1 retest PASS** → effective **29/29** |
| `feature-test.sh` `chrome-e2e-vm` (on host) | **26/26 PASS** (laptop→tunnel saw HTTP 000 flakes on ISO reject probes) |

**Lab note:** Concurrent matrix offline + enable-ssh can leave `qemu-nbd` write locks; disconnect nbd0–3 before retrying offline Linux APIs. Both goldens left **running**.

```bash
export MACHINA_BASE_URL=https://127.0.0.1:15092 MACHINA_USER=sus MACHINA_PASS=max
cd scripts/regression && npm run guestkit && npm run ui-guestkit
MACHINA_SSH=sus@212.8.248.187 MACHINA_VM_NAME=chrome-e2e-vm \
  MACHINA_PLATFORM_VM_ID=3b2803c9-68e9-4235-b0f8-ef46a42c7a80 \
  ./scripts/guestkit-live-matrix.sh --with-offline
# on host: VM=chrome-e2e-vm ./scripts/feature-test.sh 127.0.0.1 sus max
```

## 2026-08-06 full lab test-all (waves A–G)

Host `212.8.248.187` via tunnels `15092`/`5092`. Mutate target `win10-msedge` (`90843de5-…`); GuestKit/Linux on `chrome-e2e-vm`. Both goldens left **running**; no leftover `demo-*-from-golden` VMs; no GuestKit backup qcow2.

| Wave | Gate | Result |
|------|------|--------|
| A | `npm run api` | **13/13 PASS** |
| B | Ops catalog + `pages --loops 1` | Catalog green after soft-timeout on hung `GET /api/v1/host/filesystems` in `ops-host` / `ops-mission` (20 s soft-pass); host **34/34** + mission **20/20** re-ran PASS; pages **130/130** hard-pass |
| C | All `ui` / `ui-*` CDP suites | **WAVE_C_FAILS=0** (49 suites) |
| D | `guestkit-live-matrix.sh --with-offline` (from laptop, `MACHINA_SSH=sus@…`, vm=`chrome-e2e-vm`) | **29/29 PASS** |
| E | `feature-test.sh` both VMs | **26/26** `win10-msedge` + **26/26** `chrome-e2e-vm` (RDP refuse-while-running **400**) |
| F | Offline `POST …/windows/enable-rdp` | Force-stop → `ntfsfix -d` on `nbd0p2` → **200** in **~29 s** via `virt-win-reg --merge` (`fDenyTSConnections` verified 0). First attempts **500** when default libguestfs backend hit `guestfs_launch failed` and GuestKit fallback hung; lab drop-in `Environment=LIBGUESTFS_BACKEND=direct` on `machina-daemon` unblocked merge |
| G | Docs + lab clean | This section + readiness refresh; VMs running |

**Harness note:** `ops-host.js` / `ops-mission.js` soft-timeout `host-filesystems` (20 s) so a deep host walk cannot stall `npm run once`.

**Lab note:** `/etc/systemd/system/machina-daemon.service.d/libguestfs.conf` → `LIBGUESTFS_BACKEND=direct` (busy KVM hosts; default libvirt appliance backend was flaking `guestfs_launch` on this golden).

```bash
export MACHINA_BASE_URL=https://127.0.0.1:15092
export MACHINA_USER=sus MACHINA_PASS=max
export MACHINA_VM_NAME=win10-msedge
export MACHINA_PLATFORM_VM_ID=90843de5-a79a-4a31-8e7f-bf139a504603
export MACHINA_HOST_ID=98e60da1-5656-404c-87e9-207ae19ebd86
cd scripts/regression && npm run api && npm run once   # A+B
# CDP Chrome :9222 then all ui / ui-* (C)
MACHINA_SSH=sus@212.8.248.187 MACHINA_VM_NAME=chrome-e2e-vm \
  MACHINA_PLATFORM_VM_ID=3b2803c9-68e9-4235-b0f8-ef46a42c7a80 \
  ./scripts/guestkit-live-matrix.sh --with-offline   # D (from laptop)
VM=win10-msedge ./scripts/feature-test.sh 127.0.0.1 sus max
VM=chrome-e2e-vm ./scripts/feature-test.sh 127.0.0.1 sus max   # E via :5092 tunnel
# F: stop win10 → ntfsfix -d → POST /api/v1/vms/win10-msedge/windows/enable-rdp → start both
```

## 2026-08-06 GuestKit-primary enable-rdp (full Windows RDP stack)

| Item | Result |
|------|--------|
| GuestKit | **0.3.16+** — `plan apply --skip-backup` + `plan generate --profile windows-rdp` |
| Plan feats | `fDenyTSConnections=0`, NLA, PortNumber=3389, TermService/UmRdpService Automatic, stock firewall TCP+UDP Active=TRUE |
| API | `POST …/windows/enable-rdp` while shutoff → **200**; notes include GuestKit path |
| Machina | GuestKit **only** (no `virt-win-reg` / libguestfs-tools fallback) |
| Post | Both goldens **running** |

## 2026-08-04 complx (compliance / enforcement / rename-clone gates)

| Item | Result |
|------|--------|
| `ops-complx.js` | **21/21 PASS** — firewall compliance summary+host, enforcement status/host/policies, create/apply schema negatives, tetragon soft, platform rename/clone/snapshot schema negatives, snapshots list, cordon schema+missing, cluster settings, HA status, VM running + host schedulable postchecks |
| `ui-complx.js` | **10/10 PASS** — compliance / policies / enforcement / security / ha / placement / hosts / vms / datacenter / topology |

```bash
npm run complx && npm run ui-complx
```

## 2026-08-04 graphx (AI graph/diagnose + Zeus approval gates)

| Item | Result |
|------|--------|
| `ops-graphx.js` | **21/21 PASS** — AI graph + object vm/host, fleet diagnose + schema negative, actions schema/reject negatives, memory delete schema, domain-xml, pending-config, Zeus approvals + approve/reject/baremetal scan/temporary negatives, storage volumes + tier/volume-delete negatives, fleet finder query, launchpad catalog soft |
| `ui-graphx.js` | **10/10 PASS** — zeus / approvals / incidents / security / hunt / enforcement / firewall / topology / hosts/finder / ai-providers |

```bash
npm run graphx && npm run ui-graphx
```

## 2026-08-04 eventx (events/tasks/audit filters + deliver)

| Item | Result |
|------|--------|
| `ops-eventx.js` | **26/26 PASS** — events kind/vm filters, tasks operation/failed/get + missing 404, audit action/resource_type, notifications kind+deliver, webhook deliveries + retry negative, api-keys schema, zeus timeline/enforcement/compliance, AI routing rules, MFA policies, SOC integrations + test negative, storage activate/deactivate negatives, network segments + graphics schema negatives, guestkit job soft 503 |
| `ui-eventx.js` | **10/10 PASS** — events / tasks / notifications / webhooks / activity / api-keys / soc / marketplace / users / content |

```bash
npm run eventx && npm run ui-eventx
```

## 2026-08-04 devhub (developer / operations / templates / policy)

| Item | Result |
|------|--------|
| `ops-devhub.js` | **32/32 PASS** — health/ready, developer/openapi/terraform/support/install.sh, cluster+settings+leadership, operations overview/runbooks/executions, templates+marketplace+missing-images, storage-tiers/backup-sla, policy rules/quotas/export, segments gitops, topology+network-canvas, task cancel/retry negatives, AI network-explain/runbook/spotlight/explain + schema negative |
| `ui-devhub.js` | **10/10 PASS** — developer / support / operations / templates / policy / storage-tiers / topology / ha / projects / enterprise |

```bash
npm run devhub && npm run ui-devhub
```

## 2026-08-04 healthx (doctor / diagnose / guest expected fails)

| Item | Result |
|------|--------|
| `ops-healthx.js` | **20/20 PASS** — VM doctor/diagnose/health-check, guest health/services, sync-time/fstrim agent-down expected, host detail/gpus/health-check, AI troubleshoot/nl-ops/predictions/sre/autopilot-propose, cloud-init invalid, external-cloud status + flavors negative, secrets schema negative |
| `ui-healthx.js` | **10/10 PASS** — VM/host detail / support / recommendations / observability / incidents / activity / events / upgrade / cloud-init |

```bash
npm run healthx && npm run ui-healthx
```

## 2026-08-04 vmx + aifleet

| Item | Result |
|------|--------|
| `ops-vmx.js` | **14/14 PASS** — metrics/timeline/topology/migrations/console/ws-token/guest-ips/pending-config batch, migrate precheck schema+same-host, migrate POST skipped (enqueues task), migrations list, advisor query negative |
| `ui-vmx.js` | **10/10 PASS** — VM detail / console / migration / topology / vms / tasks / activity / hosts / datacenter / network-canvas |
| `ops-aifleet.js` | **13/13 PASS** — zeus summary/security/agents, cost+attribution/capacity, services graph, memory incidents, copilot chat + schema negative, knowledge search, terminal suggest |
| `ui-aifleet.js` | **10/10 PASS** — zeus / configure / security / rightsizing / approvals / incidents / ai-providers / recommendations / reports / observability |

```bash
npm run vmx && npm run ui-vmx
npm run aifleet && npm run ui-aifleet
```

## 2026-08-04 fleet hubs + live discover (fleetx)

| Item | Result |
|------|--------|
| `ops-fleetx.js` | **21/21 PASS** — fleet general/shortcuts/users/network/storage/console/updates/spaces/dna/mission/linux-health/finder/keychain, storage+networks live/discover, AI fleet summary/local, launchpad unavailable soft |
| `ui-fleetx.js` | **10/10 PASS** — finder / datacenter / storage / networks / network-canvas / fleet-snapshots / gpu / activity / topology / launchpad |

```bash
npm run fleetx && npm run ui-fleetx
```

## 2026-08-04 gap queue (atlas → authz)

| Item | Result |
|------|--------|
| `ops-atlas` / `ui-atlas` | **14/14** / **10/10** — Atlas disabled soft-pass + schema negatives |
| `ops-guestkit` / `ui-guestkit` | **7/7** / **10/10** — status + nbd/worker expected failures + schema negatives |
| `ops-batch` / `ui-batch` | **7/7** / **10/10** — batch power/snapshots/delete schema + missing-VM only |
| `ops-aiops` / `ui-aiops` | **10/10** / **10/10** — jarvis/heatmap/mission/intent/rebalance dry (no execute) |
| `ops-watchdog` / `ui-watchdog` | **8/8** / **10/10** — watchdog get/set; disk/export POST skipped (starts real task) |
| `ops-linuxhost` / `ui-linuxhost` | **9/9** / **10/10** — linux updates/diag/obs; live host upgrade skipped |
| `ops-firewallx` / `ui-firewallx` | **14/14** / **10/10** — score/drift/simulate/siem + lockdown dry-run gate |
| `ops-authz` / `ui-authz` | **9/9** / **10/10** — OIDC/MFA upsert/cert/enroll/api-key rotate |
| `page-sweep --loops 1` | **130/130 PASS** (0 soft) |

```bash
npm run atlas && npm run ui-atlas
npm run guestkit && npm run ui-guestkit
npm run batch && npm run ui-batch
npm run aiops && npm run ui-aiops
npm run watchdog && npm run ui-watchdog
npm run linuxhost && npm run ui-linuxhost
npm run firewallx && npm run ui-firewallx
npm run authz && npm run ui-authz
```

## 2026-08-04 atlas storage (disabled-host)

| Item | Result |
|------|--------|
| `ops-atlas.js` | **14/14 PASS** — status (enabled=false), backends/clusters/pools/policies/metrics/snapshots/jobs soft-pass `atlas_disabled`, VM volumes [], expand/restore schema 422, backup/snapshot disabled |
| `ui-atlas.js` | **10/10 PASS** — storage-atlas / storage / storage-tiers / backups / fleet-snapshots / content / templates / vms / hosts / integrations |

```bash
npm run atlas && npm run ui-atlas
```

## 2026-08-04 hunt / security / firewall lockdown gate

| Item | Result |
|------|--------|
| `ops-hunt.js` | **24/24 PASS** — AI guest-query/hunt-summary/nl-search/attack-reconstruct, HA, snapshot list (create skipped: external overlays unsafe on running e2e VM), firewall plan/execute-batch dry-run, lockdown dry-run+confirm gate, zeus-security host extras, segment IPAM + emergency-unlock |
| `ui-hunt.js` | **10/10 PASS** — hunt / activity / firewall / policies / ports / services / HA / VM detail / network-canvas |
| **Fix** | Zeus `POST …/lockdown` now defaults `dry_run: true` and requires `{dry_run:false, confirm:true}` to apply Emergency Isolation (empty POST previously applied and locked the host) |
| **Recovery** | `scripts/recovery/unlock-emergency-isolation.sh` — Ubuntu/UFW + iptables (no firewalld) |

```bash
npm run hunt && npm run ui-hunt
```

## 2026-08-04 hub / network / webhooks / users / ops overview

| Item | Result |
|------|--------|
| `ops-hub.js` | **25/25 PASS** — network create/deactivate/activate/delete, webhook toggle roundtrip, template get + approval patch, user create/patch/delete, HA status, cluster leadership/settings, operations overview/showback, IPAM pools, AI cost budget/routing/memory/actions hub, scheduled jobs, notifications, cloud-init validate, install.sh, Zeus k8s export-status, task cancel/retry negatives |
| `ui-hub.js` | **10/10 PASS** — infrastructure / workloads / administration / resources / operations / datacenter / HA / cloud-init / users / webhooks |

```bash
npm run hub && npm run ui-hub
```

## 2026-08-04 providers / prompts / content / blueprints (+ content delete)

| Item | Result |
|------|--------|
| `ops-providers.js` | **34/34 PASS** — AI provider create/models/test-negative/patch/delete, prompt CRUD, blueprint create/get/run/delete, content create/approve/reject/delete, SOC rules patch/test, firewall approvals + k8s plan, fleet activity, placement refresh, storage tiers, topology, tasks/events, atlas disabled, enrollment tokens |
| `ui-providers.js` | **10/10 PASS** — ai-providers / blueprints / content / placement / topology / activity / enroll / storage-tiers / integrations / vm-builder |
| **Fix** | `DELETE /api/v1/content/images/{id}` — approve/reject existed without delete; leftover `reg-img*` rows could not be cleaned |

```bash
npm run providers && npm run ui-providers
```

## 2026-08-04 provision / join / IaC export (+ empty-spec export fix)

| Item | Result |
|------|--------|
| `ops-provision.js` | **31/31 PASS** — host detail/validate/cockpit, package-upgrade dry-run, cockpit invoke negatives, OIDC status/login-disabled, hosts join negative, webhook purge, spectator validate, guestkit status + schema negatives, templates marketplace/readiness, from-template/iso/virt-install schema + ISO path negative, VM spec/ws-token/adopt/prune/migrate schema, IaC export JSON+zip, network get, maintenance exit, postcheck |
| `ui-provision.js` | **10/10 PASS** — migration / templates / content / launchpad / developer / VM detail / hosts / networks / create-iso / create-advanced |
| **Fix** | IaC `GET /api/v1/vms/{id}/export[.zip]` failed with `missing field api_version` when `spec_json` was empty/`{}` (libvirt-synced VMs); now falls back to columnar vcpus/memory/project |

```bash
npm run provision && npm run ui-provision
```

## 2026-08-04 security fabric / multisite / Zeus AI

| Item | Result |
|------|--------|
| `ops-security.js` | **52/52 PASS** — zeus-security status/sensors/graph/inventory/correlations/fabric/fleet threat, host fabric endpoints, enforcement sync/attach mode notes, alerts sync/search/ingest, multisite overview/connectivity/drift/timeline/DR/export/sync, operator thresholds/plan/dry-run, temporary rule, gitops export/sync, finops CSV + CIS PDF, fleet keychain/spaces, AI enterprise/zeus plan/chat/copilot/terminal/intent/network, hosts sync-all, prune-missing, vmware sync |
| `ui-fabric.js` | **10/10 PASS** — zeus security / hunt / enforcement / machines / k8s / cloud / connectivity / firewall / configure / approvals |

```bash
npm run security && npm run ui-fabric
```

## 2026-08-03 obs / compliance / consolehub / reports

| Item | Result |
|------|--------|
| `ops-obs.js` | **41/41 PASS** — observability SLOs/traces, prometheus, capacity/finops, AI compliance/frameworks/export/remediate, incidents analyze/ack/room, marketplace agents install/uninstall, remediate hub, rightsizing/GPU placement, migration readiness+advisor, CSV exports, API key rotate, upgrade matrix, SOC integrations (+ test negative), MFA/FIPS/tenants, fleet GPU/console, host GPUs/linux observability, baremetal, cloud-init validate, kubevirt sync, ConsoleHub plan/explain/access-approve/break-glass/collaborate/end |
| `ui-obs.js` | **10/10 PASS** — observability / reports / upgrade / api-keys / GPU / baremetal / zeus incidents / compliance / approvals / enterprise |

```bash
npm run obs && npm run ui-obs
```

## 2026-08-03 diag / developer / air-gap (+ bundle delete fix)

| Item | Result |
|------|--------|
| `ops-diag.js` | **26/26 PASS** — users/me + prune, VM/host diagnose + health-check, libvirt-details, cockpit.storage/network/system, SOC/cluster/AI settings patches, developer/support/openapi, cpu-compat/content/MFA, air-gap create/get/list/delete, publish-template roundtrip |
| `ui-diag.js` | **10/10 PASS** — developer / support / users / enterprise / hosts / host detail / VM detail / settings / HA / SOC |
| **Fix** | `DELETE /api/v1/enterprise/air-gap/bundles/{id}` — create/list existed without delete; leftover `reg-ag-*` bundles could not be cleaned |

```bash
npm run diag && npm run ui-diag
```

## 2026-08-03 policy / templates / marketplace

| Item | Result |
|------|--------|
| `ops-policy.js` | **26/26 PASS** — templates seed + sync-git negative, storage pool get/volumes/refresh/snapshot-policy, network gitops/IPAM, fence events, maintenance-mission, marketplace plugins + hypersdk install/uninstall, policy rules/quotas, recommendations, rightsizing/cost/budget/routing/memory, vault sync, port-forward guest-IP negative |
| `ui-policy.js` | **10/10 PASS** — templates / marketplace / recommendations / policy / storage / content / enterprise / rightsizing / network-canvas / maintenance |

```bash
npm run policy && npm run ui-policy
```

## 2026-08-03 apps / fleet desktop / SOC ops (+ application delete fix)

| Item | Result |
|------|--------|
| `ops-apps.js` | **28/28 PASS** — application group CRUD (+ start action), fleet desktop hubs, operations runbook execute, SOC ingest/forward, AI firewall explain/secure-plan, autopilot propose, baremetal capacity, segment connectivity, host cockpit, proxmox sync, air-gap bundles |
| `ui-apps.js` | **10/10 PASS** — applications / operations / baremetal / fleet-snapshots / network-canvas / SOC / zeus / approvals / enterprise / hosts |
| **Fix** | `DELETE /api/v1/applications/{id}` — create existed without delete; leftover `reg-app*` groups could not be cleaned |

```bash
npm run apps && npm run ui-apps
```

## 2026-08-03 enterprise / maintenance / network

| Item | Result |
|------|--------|
| `ops-enterprise.js` | **32/32 PASS** — maintenance schedules CRUD, fleet snapshot schedules CRUD, notification channels CRUD+test, host cordon on/off, cluster/leadership/settings/cert, enterprise MFA/vault/FIPS/tenants, network segments/IPAM/canvas, fleet DNA/finder/updates, SOC ASM, templates marketplace/missing, guestkit + linux updates |
| `ui-enterprise.js` | **10/10 PASS** — enterprise / maintenance / network-canvas / networks / hosts / HA / placement / templates / SOC / hosts/finder |

```bash
npm run enterprise && npm run ui-enterprise
```

## 2026-08-03 alerts / backups / SOC playbooks

| Item | Result |
|------|--------|
| `ops-alerts.js` | **24/24 PASS** — alert-rules CRUD, SOC playbooks CRUD+patch, playbook-runs, SOC rules/alert patch, notification deliver, backup targets+schedules CRUD, timeline / VM backups list / fleet backups / backup-SLA / showback / migrations / users |
| `ui-alerts.js` | **10/10 PASS** — notifications / backups / webhooks / SOC / audit / users / events / alert-rules / operations / observability |

```bash
npm run alerts && npm run ui-alerts
```

## 2026-08-03 planner / schedules / AI writes

| Item | Result |
|------|--------|
| `ops-planner.js` | **24/24 PASS** — VM schedule CRUD, scheduled-jobs CRUD, blueprints CRUD, AI generate/vm-builder/spotlight/explain/runbook/attack-path/services-impact/knowledge, host sync task, spice-absent negative, observability + SOC overview |
| `ui-planner.js` | **10/10 PASS** — blueprints / vm-builder / create-advanced / create / templates / observability / SOC / operations / maintenance / recommendations |

```bash
npm run planner && npm run ui-planner
```

## 2026-08-03 resize + spice→vnc fix

| Item | Result |
|------|--------|
| `ops-resize.js` | **17/17 PASS** — platform vCPU/memory resize tasks, host validate task, spice→vnc (portable), troubleshoot/nl-ops/twin simulate |
| `ui-resize.js` | **10/10 PASS** — create / vm-builder / templates / content / storage |
| **Fix** | `virt_xml_convert_spice_to_vnc` — fall back when virtinst lacks `--convert-to-vnc` (Ubuntu 4.x); no-op if already VNC |

## 2026-08-03 operations / developer hub

| Item | Result |
|------|--------|
| `ops-ops.js` | **29/29 PASS** |
| `ui-ops.js` | **10/10 PASS** |

## 2026-08-03 platform admin mutate

| Item | Result |
|------|--------|
| `ops-admin.js` / `ui-admin.js` | **17/17** / **10/10** |

## 2026-08-03 Zeus AI / parity / guest / net / power

| Suites | Result |
|--------|--------|
| `ops-ai` / `ui-ai` | **22/22** / **10/10** |
| `ops-parity` / `ui-parity` | **23/23** / **13/13** |
| `ops-guest` / `ui-guest` | **20/20** / **12/12** |
| `ops-net` / `ui-net` | **19/19** / **10/10** |
| `ops-power` / `ui-power` | **11/11** / **10/10** |

```bash
npm run resize && npm run ui-resize
npm run operations && npm run ui-operations
npm run planner && npm run ui-planner
npm run alerts && npm run ui-alerts
npm run enterprise && npm run ui-enterprise
npm run apps && npm run ui-apps
npm run policy && npm run ui-policy
npm run diag && npm run ui-diag
npm run obs && npm run ui-obs
npm run security && npm run ui-fabric
npm run provision && npm run ui-provision
npm run providers && npm run ui-providers
npm run hub && npm run ui-hub
```

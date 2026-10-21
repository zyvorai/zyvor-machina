# Live qualification scripts

Unit tests prove the logic; these scripts prove the claims that need real infrastructure. They are **not** run in CI. Each one
drives the real API of a real deployment, checks what comes back, cleans up after itself, and saves every answer as evidence.
The older scripts here (`ha-host-loss.sh`, `atlas-live.sh`, `baremetal-vendors.sh`, `transiva-live.sh`, `aws-conformance.py`,
`scale.py`) follow the same idea; the ones below share `lib.sh`.

| Script | Claim | Runs against | Changes real infrastructure? |
|---|---|---|---|
| `k3s-fleet-live.sh` | C61 fleet k3s clusters | controller + 1 to 6 hosts | **Yes**: root k3s installers on the hosts you name, removed at the end |
| `backup-replication-live.sh` | C64 off-site copies | controller + a real S3 bucket | Writes a copy and a manifest to the bucket; fetches onto a host |
| `iscsi-live.sh` | C66 iSCSI pools | a host's daemon + a real array | Logs in to the array, attaches a LUN to a disposable VM |
| `vgpu-live.sh` | C59 vGPU partitions | controller + a vGPU host | Creates and removes a partition; attaches it to a disposable VM |
| `trusted-launch-live.sh` | C60 Secure Boot + vTPM | a host's daemon | Creates, starts and deletes a VM |
| `witness-drill.sh` | C65 HA witness | controller + your network | Runs the commands you give it to cut and restore a path |

Read the header of each script for its environment variables. The scripts that change things refuse to run until you set a
`*_CONFIRM` variable to a sentence naming what they will do (for example `K3S_CONFIRM=install-k3s-on-these-hosts`), so nobody runs
one by accident. Use disposable hosts, VMs and LUNs.

## Exit codes

`0` pass, `1` fail, `2` missing configuration, tool or confirmation, `77` skipped (the environment cannot run the check, for
example no vGPU host). A failure prints `FAIL: ...` and the evidence directory.

## Evidence

Every script prints `evidence: <dir>` first and last. The directory (default a fresh one under `$TMPDIR`, or `QUAL_EVIDENCE_DIR`)
holds each API answer, the domain XML, the kubeconfig and so on. Archive it with the claim.

## Recording a result

When a script passes on real infrastructure, update the claim in `docs/claims.md` in the same change: move the status from
`unit-tested` to `verified on <date>`, and name the hardware, the version under test and where the evidence is kept. Record a
failure there too, with what it showed.

## What these scripts do not cover

- **Windows 11**: `trusted-launch-live.sh` proves libvirt accepts and starts the Secure Boot + vTPM VM. Installing Windows needs a
  person: create the VM the same way with the ISO, finish the installer's hardware check, and keep a screenshot.
- **A real network cut** is only as real as the commands you pass to `witness-drill.sh`.
- **Guest-side checks** (a GPU visible in the guest, a filesystem on the LUN) are optional hooks (`VGPU_GUEST_SSH`,
  `REPL_FETCH_SSH`, `TL_HOST_SSH`) or manual steps the script tells you to do.

## Testing the scripts themselves

`QUAL_POLL_SECS` shortens every poll and `QUAL_TIMEOUT_CAP` caps every wait. They exist so the scripts can be run against a mock
API; do not set them for a real run.

# Machina production-completion mega PR

This change intentionally lands the remaining high-impact **software-side** hardening in one PR instead of feature-by-feature PRs.

## Implemented by code in this PR

- Cross-host VPC peering over the existing authenticated `machina-wg` WireGuard fabric.
- Separation of WireGuard AllowedIPs (source ownership) from installed destination routes.
- Direct routed VPC guard exceptions without weakening the existing fleet-address DNAT guard.
- Host forwarding policy for peering routes, Internet Gateway routes and blackholes.
- EC2-style stateless network ACL enforcement before compatibility iptables/libvirt chains.
- Immediate controller convergence after native peering/route changes; periodic reconciliation remains as drift repair.
- IPv4/IPv6-correct overlay route and unreachable-route programming.
- Production bpfd startup gate: packaged service refuses an empty eBPF build.
- Production security preflight: authentication, encryption master key, mTLS and bpfd requirements.
- Master-key file support for Vault/systemd-credential style delivery.
- JWT signing-key rollover with current + previous verification keys.
- Bounded audit retention with a leader-only daily reaper.
- Native ARM64 CI and dual-architecture release artifacts/packages.
- Architecture-specific checksums, SBOMs and signed release manifests.
- Native Machina → Transiva/h2kvm gateway for provider discovery and migration preview/submit/batch.

## Already present on main; not duplicated

- QGA freeze/thaw + libvirt live backup + integrity verification.
- HA fencing/recovery logic.
- Controller/agent mTLS enrolment path.
- PostgreSQL controller and leader election.
- Native WireGuard VM overlay and VM network policy engine.
- Atlas control-plane adapter.
- Redfish/IPMI bare-metal code and emulator coverage.

## Qualification gates shipped in this PR

These cannot truthfully be marked production-verified by unit tests alone. The PR ships executable evidence collectors instead:

- `scripts/qualification/ha-host-loss.sh` — real host power-loss/fencing/recovery drill.
- `scripts/qualification/baremetal-vendors.sh` — Dell iDRAC, HPE iLO, Supermicro, Lenovo XCC, OpenBMC physical smoke.
- `scripts/qualification/atlas-live.sh` — live Atlas gateway path.
- `scripts/qualification/transiva-live.sh` — Machina → Transiva/h2kvm gateway path.
- `scripts/qualification/aws-conformance.py` — real boto3 calls against EC2/ELBv2 endpoints.
- `scripts/qualification/scale.py` — controller latency/load measurement.
- `scripts/compliance/generate-evidence.py` — evidence manifest from SBOMs/signatures/test artifacts.

## Explicit non-claims

This PR does **not** label the following as certified merely because code exists:

- FIPS-validated cryptography. Current Rust TLS uses ring and is not a FIPS validation claim.
- Physical BMC vendor certification until the vendor script is run on real hardware.
- Multi-host HA host-loss verification until the hard-power-off drill passes.
- Atlas appliance qualification until the live script passes.
- Full AWS service compatibility beyond the actions exercised by tests/qualification.

Advanced ELBv2 L7/ACM semantics and multi-region federation remain separate product subsystems; this PR does not emulate them with misleading no-op API objects.

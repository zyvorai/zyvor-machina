---
sidebar_position: 4
title: Production hardening
description: The production security profile, secret and key lifecycle, audit retention and the mandatory eBPF gate.
---

# Production hardening

Development defaults stay forgiving. A production site opts in to a strict profile that refuses to start on unsafe
settings, supports rotating secrets without downtime, and bounds how long audit evidence is kept.

## The production profile

Set `MACHINA_SECURITY_PROFILE=production` in `/etc/default/machina-platform`. The packaged controller unit runs
`/usr/libexec/machina/production-preflight` before it starts, and the controller does not boot (exit code `78`) unless:

| Check | Requirement |
| --- | --- |
| Authentication | `MACHINA_SKIP_AUTH` is not `1`; `MACHINA_ALLOW_DEV_SECRETS` is not `1` |
| JWT secret | `MACHINA_JWT_SECRET` is set and at least 32 characters |
| Master key | A 64-hex-character AES-256 key, from `MACHINA_API_KEY_MASTER_KEY` or `MACHINA_API_KEY_MASTER_KEY_FILE` |
| Agent transport | `MACHINA_AGENT_CA`, `MACHINA_AGENT_CLIENT_CERT` and `MACHINA_AGENT_CLIENT_KEY` are readable files |
| Datapath | `MACHINA_BPF_REQUIRED` is true |

The profile is opt-in so a developer install is unaffected; a source install without the packaged preflight skips it.

## Master key from a file

`MACHINA_API_KEY_MASTER_KEY_FILE` names a root-readable file holding the 64-hex key. It is read first and falls back to
`MACHINA_API_KEY_MASTER_KEY`. Use it with systemd credentials, a Vault agent or a Kubernetes secret volume so the key
never sits in an environment block. The key encrypts LLM provider keys and BMC credentials at rest.

## JWT signing-key rollover

1. Move the current secret to `MACHINA_JWT_PREVIOUS_SECRETS` (comma separated) and set a new `MACHINA_JWT_SECRET`.
2. Restart the controller. New tokens are signed with the new secret; tokens signed with a previous one still verify.
3. After the longest token lifetime has passed, remove the previous secret.

Previous secrets are used for verification only; nothing is ever issued with them.

## Audit retention

A leader-only job runs daily and deletes `audit_logs` rows older than `MACHINA_AUDIT_RETENTION_DAYS` (default `365`,
clamped to 7-3650; `0` keeps everything). Ship signed audit lines off-host first if you need longer evidence.

## Mandatory eBPF datapath

A `machina-bpfd` built without its embedded eBPF object used to start and report `programs_compiled: false`. With
`MACHINA_BPF_REQUIRED=true` (or `--require-programs`) it refuses to start instead. The packaged service sets it, and
`machina-bpfd --probe` reports both `programs_compiled` and `programs_required`.

## Release packages and ARM64

Release builds produce DEB, RPM and tar bundles for `amd64` and `arm64`, built and tested on native runners, each with
its own SBOM and `SHA256SUMS-<arch>`. `scripts/release/build-release.sh --arch amd64|arm64` builds one.

## Qualification gates

Some claims cannot be proven by unit tests. These scripts collect the evidence on real infrastructure and are **not**
run in CI:

| Script | Proves |
| --- | --- |
| `scripts/qualification/ha-host-loss.sh` | Hard power-off of a host, fencing and recovery |
| `scripts/qualification/baremetal-vendors.sh` | Vendor BMC smoke (iDRAC, iLO, Supermicro, XCC, OpenBMC) |
| `scripts/qualification/atlas-live.sh` | The live Atlas gateway path |
| `scripts/qualification/transiva-live.sh` | Machina to Transiva/h2kvm gateway |
| `scripts/qualification/aws-conformance.py` | boto3 calls against the EC2 and ELBv2 endpoints |
| `scripts/qualification/scale.py` | Controller latency under concurrent load |
| `scripts/compliance/generate-evidence.py` | A manifest of SBOMs, hashes and signatures |

## Not claimed

FIPS-validated cryptography (the TLS stack uses `ring`), full ELBv2 L7/ACM and multi-region federation are not
provided.

Related: [Security](security.md), [Environment variables](../reference/environment.md).

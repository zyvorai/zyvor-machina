---
sidebar_position: 6
title: Transiva migration gateway
description: Provider discovery and VM migration preview, submit and batch through Machina, backed by Transiva/h2kvm.
---

# Transiva migration gateway

Machina exposes the migration plane of [Transiva/h2kvm](https://zyvor.dev) under `/api/v1/transiva/*`. Conversion and
guest repair stay in Transiva; Machina adds one authenticated entry point and the usual RBAC and audit trail.
All routes need the `operator` role.

| Method and path | Forwards to | Purpose |
| --- | --- | --- |
| `GET /api/v1/transiva/status` | `GET /api/v1/health` | Is the Transiva web service reachable |
| `GET /api/v1/transiva/providers` | `GET /api/v1/providers/` | Source platforms Transiva can read |
| `POST /api/v1/transiva/migrations/preview` | `POST /api/v1/migrations/preview` | Dry-run a migration |
| `POST /api/v1/transiva/migrations/submit` | `POST /api/v1/migrations/submit` | Start one migration job |
| `POST /api/v1/transiva/migrations/batch` | `POST /api/v1/migrations/batch` | Start several |

Request bodies are passed through unchanged; use the Transiva reference for their fields.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `MACHINA_TRANSIVA_URL` | `http://127.0.0.1:5070` | Transiva web (h2kweb) base URL; must be `http://` or `https://` |
| `MACHINA_TRANSIVA_API_KEY` | unset | Sent as `X-API-Key` |
| `MACHINA_TRANSIVA_TIMEOUT_SECS` | `60` | Per-call timeout |

A Transiva error is returned with its status and the first 2 KB of its body, plus a remediation hint.

:::note Status
Unit-tested only. `scripts/qualification/transiva-live.sh` runs the full path against a live h2kweb.
:::

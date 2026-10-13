---
sidebar_position: 3
title: REST API
description: Authenticate, call the daemon and controller APIs, and find the OpenAPI specs.
---

# REST API

Everything the UI does goes through the same REST API, so anything you can click you can script.

## Base URLs

| API | Base | Notes |
| --- | --- | --- |
| Daemon | `https://HOST:5092/api/v1` | Single host: VMs, storage, networks, consoles, eBPF (`/bpf/*`) |
| Controller | `https://HOST:5092/api/v1/platform/controller/api/v1` | Fleet, HA, DRS, Fleet Cloud, Zyra AI, fleet eBPF (`/zeus-security/*`); proxied by the daemon |

Call the controller through the daemon proxy rather than `:5093` directly, so one session and one certificate cover
both.

## Authenticate

Interactive session (cookie):

```bash
curl -sk -c cookies -H 'Content-Type: application/json' \
  -d '{"username":"ops","password":"…"}' https://HOST:5092/api/v1/auth/login
curl -sk -b cookies https://HOST:5092/api/v1/vms
```

Automation token (bearer): create one under **Settings** (API tokens) or with
`POST /api/v1/tokens` `{name, username, role}`, then:

```bash
curl -sk -H "Authorization: Bearer mach_…" https://HOST:5092/api/v1/vms
```

Give tokens the least privilege they need; `operator` covers day-to-day automation.

## Errors

Errors are JSON with a human message, a stable `error_code` and, where possible, a `remediation`:

```json
{"error": "vm not found", "error_code": "not_found", "remediation": "check the VM name"}
```

## Long-running operations

Controller operations such as migrations return a task:

```json
{"task_id": "…", "status": "pending", "operation": "vm.migrate"}
```

Poll `GET /api/v1/tasks/{task_id}` until it finishes.

## Live updates

`wss://HOST:5092/ws/v1/watch?token=…` streams VM and host state changes (get the short-lived token from
`POST /api/v1/ws-token`); `GET /api/v1/bpf/stream` streams eBPF events as server-sent events.

## OpenAPI

Generated specs live in the repository:

- [Daemon OpenAPI](https://github.com/zyvorai/zyvor-machina/blob/main/docs/openapi-daemon.json)
- [Controller OpenAPI](https://github.com/zyvorai/zyvor-machina/blob/main/docs/openapi-controller.json)

Clients: the [TypeScript SDK](https://github.com/zyvorai/zyvor-machina/blob/main/sdk/typescript/README.md), the
[Terraform provider](https://github.com/zyvorai/zyvor-machina/blob/main/terraform/machina/README.md) and `machinactl`.

## VPC forwarding

Peering, route and network-ACL endpoints under `/api/v1/cloud/…` report `forwarding_active`; see
[VPC forwarding](../core-concepts/vpc-forwarding.md).

## Migration gateway

`/api/v1/transiva/*` forwards provider discovery and migration preview, submit and batch to Transiva/h2kvm:
[Transiva migration gateway](transiva-api.md).

## Bare metal

Provisioning endpoints under `/api/v1/baremetal` are listed in [Bare-metal API and CLI](bare-metal-api.md).

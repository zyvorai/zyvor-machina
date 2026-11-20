---
sidebar_position: 3
title: Consoles
description: noVNC, SPICE, serial and SSH consoles in the browser, with no gateway to deploy.
---

# Consoles

noVNC, SPICE, serial and SSH are served by `machina-daemon` (single host) or proxied through
`machina-controller` and `machina-agent` (fleet). There is no separate console gateway to install, and every session
goes through the same RBAC and audit log as the rest of the API.

```mermaid
sequenceDiagram
  participant B as Browser
  participant D as machina-daemon
  participant C as machina-controller
  participant A as machina-agent
  B->>D: GET /api/v1/vms/{id}/consolehub/plan
  D->>C: proxy
  C->>A: GetConsoleAccessPlan
  A-->>B: protocols available for this guest
  B->>D: POST /api/v1/vms/{id}/consolehub/sessions
  B->>D: WebSocket /ws/v1/platform/vnc|spice|serial/{id}
  D->>C: proxy
  C->>A: console stream
```

## Protocol selection

| Guest | Default console |
| --- | --- |
| Linux with VNC graphics | noVNC |
| FluxVM QEMU | noVNC (`/ws/v1/vnc/{name}?backend=fluxvm`, FluxVM `ws-ticket`) |
| FluxVM non-QEMU / headless | Serial (read-only log) or agent shell (`/ws/v1/fluxvm-console/{name}`) |
| Headless Linux (libvirt) | SSH (in-browser terminal) |
| SPICE graphics | SPICE |
| Recovery / boot issues | Serial |
| Windows with Remote Desktop enabled | Native RDP through a hypervisor NAT port-forward, with a downloadable `.rdp` file |

RDP only appears once the agent confirms the guest is listening on port 3389. FluxVM guests use VM detail →
**Console (VNC)** (QEMU) plus **Serial** and **Agent console**; they do not use Console Hub.

## VM detail

The VM detail page leads with a live console preview, so the first thing an operator sees is the machine itself.

![VM detail with live console](/machina-vm-detail.png)

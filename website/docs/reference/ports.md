---
sidebar_position: 1
title: Ports and sockets
description: Every port and socket Machina listens on, with defaults and how to change them.
---

# Ports and sockets

| Port / socket | Component | Default bind | Change with |
| --- | --- | --- | --- |
| `5092/tcp` | `machina-daemon` (HTTPS: web UI, REST, WebSocket) | `0.0.0.0` | `[daemon] host` / `port`, `--host` / `--port` |
| `5093/tcp` | `machina-controller` | `0.0.0.0` (`127.0.0.1` behind the daemon proxy with `--platform-bind`) | `machina-controller --host --port` |
| `50051/tcp` | `machina-agent` gRPC (TLS) | `127.0.0.1` | `machina-agent --listen` |
| `50052/tcp` | `machina-agent` console proxy | `127.0.0.1` | `machina-agent --console-listen` |
| `/run/machina-bpf/bpfd.sock` | `machina-bpfd` (root only, mode 0600) | local | `MACHINA_BPFD_SOCK` |
| `4222/tcp` | NATS (optional, external) | — | `NATS_URL` |
| `3000/tcp` | Vite dev server (development only) | `localhost` | `web/vite.config.ts` |

Only `5092` needs to be reachable by operators. Open `5093` and `50051`/`50052` between controller and hypervisors
for a multi-host fleet (deploy with `--platform-bind 0.0.0.0`), and protect them with mTLS.

VM consoles do not need extra ports: noVNC, SPICE, serial and SSH are tunnelled over WebSockets on `5092`.

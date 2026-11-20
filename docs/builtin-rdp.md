# Built-in RDP

Machina resolves the guest IPv4 (QEMU guest agent) and exposes:

- `GET /api/v1/vms/{name}/rdp-info` — host, port, WebSocket path
- `wss://…/ws/v1/rdp/{name}?token=…` — binary TCP proxy to guest port 3389
- Web UI — **RDP** button and `.rdp` download on VM details; `/vms/{name}/rdp` holds the tunnel

Use the downloaded `.rdp` file with Microsoft Remote Desktop, or keep the tunnel page open for clients that speak RDP over the WebSocket bridge.

Requires Windows guest RDP enabled and `qemu-guest-agent` for address discovery.

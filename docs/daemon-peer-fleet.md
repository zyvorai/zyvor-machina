# Daemon peer fleet

`machina-daemon` can aggregate other daemons into one inventory without running the controller. Use it for a merged
VM list, cross-host power actions and an active/standby pair. Automatic VM failover, fencing and DRS come from the
controller instead: see [controller-ha.md](controller-ha.md).

## Configuration

```toml
[fleet]
enabled = true
primary_peer = "hv-east"
standby_peer = "hv-west"

[[fleet.peers]]
name = "hv-east"
url = "https://hv-east.example.com:5092"
api_token = "mach_…"        # automation token created on that peer
insecure_tls = false

[[fleet.peers]]
name = "hv-west"
url = "https://hv-west.example.com:5092"
api_token = "mach_…"
```

- **Local host** is always included in the merged inventory.
- **primary_peer** is highlighted in the Fleet UI as the preferred peer for operator workflows.
- **standby_peer** records the DR / secondary host; the lifecycle proxy can target any peer by name.
- Use least-privilege tokens on peers (`operator` role).

## APIs

| Endpoint | Purpose |
|----------|---------|
| `GET /api/v1/fleet/status` | Peer health and versions |
| `GET /api/v1/fleet/vms` | Merged VM list (local and peers) |
| `POST /api/v1/fleet/peers/{name}/proxy` | Forward `GET` / `POST` / `DELETE` to a peer API path (e.g. `/vms/{name}/start`) |
| `GET /api/v1/fleet/prometheus` | One Prometheus scrape for the local host and all peers (`machina_peer` label) |

## Web UI

**Fleet** (`/fleet`) shows peer status, cross-host start / stop / shutdown, and the aggregated Prometheus scrape URL.

## Active / standby pattern

1. Run `machina-daemon` on each hypervisor (`install.sh --bind 0.0.0.0 --open-firewall`).
2. Point DNS or a reverse-proxy VIP at the **primary** host for operators.
3. Keep the **standby** in `[fleet]` for inventory; fail over by moving DNS / the VIP to the standby.
4. Optional: `contrib/systemd/machina-daemon-standby.service.example` for a passive unit that stays stopped until
   failover.

The daemon has no leader election of its own; use your load balancer or DNS for the VIP. For automatic failover run
the controller.

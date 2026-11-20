# Admin Basics (Machina)

## Ports

| Port | Service |
|------|---------|
| **5092** | `machina-daemon` — HTTPS UI + `/api/v1` + WebSocket |
| **5093** | Optional controller (multi-host) |
| **3000** | Vite dev (`web/`) proxying to `:5092` |

## Auth

- Default: PAM against host accounts.
- Optional: LDAP, OIDC SSO.
- Set strong `MACHINA_JWT_SECRET`; replace self-signed TLS for production.
- Never enable `MACHINA_DAEMON_SKIP_AUTH` / `MACHINA_SKIP_AUTH` outside labs.
- Configure `roles.json` so not everyone is Admin.

## Install sketch

Use `./machinactl deploy` / `install.sh` from the customer bundle or repo docs. Verify health on `:5092`.

## Vessel (containers)

Podman or Docker on the host is optional but enabled by default:

```toml
[vessel]
enabled = true
# socket = "/run/podman/podman.sock"
auto_discover = true
```

If the engine is down, the daemon still starts; use **Infrastructure → Containers** or `POST /api/v1/vessel/reconnect` after fixing the socket.

## Related

- [Getting Started](getting-started.md)
- Handbook admin guide in `docs/handbook/` when present

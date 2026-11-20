# OIDC run-as-user

Machina maps OIDC identities to an **effective Linux user** for authorization and, when enabled, runs allow-listed host commands as that user.

## Config

```toml
[auth.run_as_user]
enabled = false
mode = "disabled"   # disabled | sudo | polkit | setuid_helper
setuid_helper_path = "/usr/local/libexec/machina-run-as-user"
prefer_session_libvirt_on_impersonation = false
```

| Field | Meaning |
|-------|---------|
| `enabled` | Turn on impersonation for supported routes |
| `mode` | `sudo`, `polkit`, or `setuid_helper` |
| `setuid_helper_path` | Setuid helper binary (see `contrib/run-as-user/README.md`) |
| `prefer_session_libvirt_on_impersonation` | When `[libvirt] dual_connection = true`, default empty `?connection=` to `session` for mapped OIDC users on VM routes |

Status: `GET /api/v1/auth/run-as-user`

### `sudo` mode

`sudo -n -u <effective_linux_user> -- <allow-listed program>`

Host requirements: passwordless sudo for allow-listed programs (`useradd`, `userdel`, `usermod`, `homectl`, `chpasswd`, `id`, `getent`).

### `polkit` mode

`pkexec --user <effective_linux_user> -- <allow-listed program>`

```bash
sudo cp contrib/polkit/machina-run-as-user.rules /etc/polkit-1/rules.d/50-machina-run-as-user.rules
```

### `setuid_helper` mode

`<setuid_helper_path> <effective_linux_user> <program> [args…]`

Install: `contrib/run-as-user/README.md`

## Supported routes today

- `POST/DELETE /api/v1/system/os-users` — OS account lifecycle as the mapped user
- **Libvirt session default** — when `prefer_session_libvirt_on_impersonation` is set and the OIDC user maps to a Linux account, empty `?connection=` selects `qemu:///session` on routes wired through `spawn_libvirt_actor` (see matrix below)
- Explicit `?connection=system` or `?connection=session` always wins

### Route coverage (`spawn_libvirt_actor` + session default)

| Route group | Session default when policy active |
|-------------|-------------------------------------|
| `/api/v1/vms/*` | Yes — list, lifecycle, disks, resize, clone, … |
| `/api/v1/vms/{name}/guest/*`, firmware, devices, tune | Yes — `vm_guest` routes |
| `/api/v1/advanced/*` (CD-ROM, migrate, boot, PCI, pools, …) | Yes |
| `/api/v1/networks/*`, `/api/v1/storage/*` | Yes |
| `/api/v1/snapshots/*` | Yes |
| `/api/v1/console/*`, `/ws/v1/console|vnc|spice|rdp/*` | Yes — WS tokens carry full `RequestActor` |
| `/api/v1/vms/{name}/virt-viewer.vv` | Yes |
| Extras: USB, cloud-init, import, live resize, DHCP, templates, virt-image-build | Yes (VM/storage paths) |
| K8s, fleet proxy, host-only extras (package updates, inventory) | No — not libvirt session policy |
| `[libvirt] extra_uris` federated list | Read-only; no session write path |

## Related docs

- [`oidc-effective-linux-user.md`](oidc-effective-linux-user.md) — mapping claims to local users
- [`macos-build.md`](macos-build.md) — building the daemon without Linux PAM

## Out of scope

- Arbitrary libvirt XML execution as the mapped user without session URI policy
- Credential caching across unrelated OIDC sessions
- Multi-tenant isolation beyond single-host RBAC

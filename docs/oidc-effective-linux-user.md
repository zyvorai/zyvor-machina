# OIDC, PAM, and local Linux user alignment

Machina distinguishes three strings:

- **Session username** — what the UI and RBAC use (`RequestActor.username`).
- **Effective Linux user** — optional mapping used for sudo-gated host actions and optional `qemu:///session` policy (`RequestActor.effective_linux_user`).
- **Daemon OS identity** — the UNIX user running `machina-daemon`. Libvirt RPC still uses this process unless `[auth.run_as_user]` session URI policy selects `qemu:///session` (see [`oidc-run-as-user.md`](oidc-run-as-user.md)).

## Defaults (out of the box)

| Auth path | Session username | Effective Linux user |
|-----------|------------------|----------------------|
| **PAM (password)** | The login name you typed | Always `Some(<login name>)` — same string as the session username |
| **OIDC** | From IdP claims: `username_claim` (default `preferred_username`), else `email`, else `sub` | Candidate from `linux_username_claim` (default `preferred_username`), then `username_claim`, then the resolved session username — **kept only if** that string exists on the host (`getent passwd` after username validation) |

Config defaults (`OidcConfig::default()`):

- `username_claim` = `preferred_username`
- `linux_username_claim` = `preferred_username` (same default key; override either when your IdP uses a dedicated Unix login claim)
- `require_local_user_for_session_libvirt` = `false` (set `true` to reject `qemu:///session` VM creation when no mapped local user exists)

There is **no fixed default OIDC “Unix username”** across deployments: it is whatever your IdP puts in those claims. If the candidate does not exist locally, `effective_linux_user` is `None` even though the browser session is valid.

## Policy alignment vs impersonation

Behavior is **policy alignment** plus optional **run-as-user** for allow-listed host commands. Full detail: [`oidc-run-as-user.md`](oidc-run-as-user.md).

When `[auth.run_as_user]` is enabled (`sudo`, `polkit`, or `setuid_helper`):

- `POST/DELETE /api/v1/system/os-users` runs `useradd` / `userdel` / `homectl` as `effective_linux_user`
- With `prefer_session_libvirt_on_impersonation` and `[libvirt] dual_connection = true`, empty `?connection=` defaults to **session** on libvirt routes that use `spawn_libvirt_actor` (VMs, snapshots, advanced, guest devices, networks, storage, consoles, extras VM paths)

The daemon process still opens libvirt as its own UID; session policy chooses **which URI** (`qemu:///system` vs `qemu:///session`), not a full libvirt re-exec as the mapped user.

Status: `GET /api/v1/auth/run-as-user`

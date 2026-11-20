# Machina run-as-user setuid helper

Build and install the minimal setuid-root helper used when `[auth.run_as_user] mode = "setuid_helper"`.

## Build

```bash
cargo build --release -p machina-run-as-user-helper
```

## Install (Linux)

```bash
sudo install -m 4755 -o root -g root \
  target/release/machina-run-as-user \
  /usr/local/libexec/machina-run-as-user
```

Point Machina config at the same path (default):

```toml
[auth.run_as_user]
enabled = true
mode = "setuid_helper"
setuid_helper_path = "/usr/local/libexec/machina-run-as-user"
```

## Config

```toml
[auth.run_as_user]
enabled = true
mode = "setuid_helper"
# Optional: default VM create to qemu:///session when dual libvirt is enabled
prefer_session_libvirt_on_impersonation = true
```

## Security

The helper only execs allow-listed basenames: `useradd`, `userdel`, `usermod`, `homectl`, `chpasswd`, `id`, `getent`. Review `run-as-user-helper/src/main.rs` before deploying.

Alternative backends without setuid: `sudo` or `polkit` (see `docs/oidc-run-as-user.md`).

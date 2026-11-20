# Host Enrollment

## Purpose

Join new KVM hypervisors to the control plane. You generate a one-time join token here and run the install command on the new host; its `machina-agent` then registers with the controller.

## When to use it

- Adding a new hypervisor to the fleet
- Re-enrolling a host after a reinstall
- Auditing which join tokens were issued, used or revoked

## How to get there

- Route: `/platform/enroll`
- Nav: **Platform → Add Host** (or spotlight / Finder search)

## What you can do

1. Under **Join token**, generate a token. The **Active token** panel shows the token, its expiry and a ready-made **Install command**; copy either.
2. On the new KVM host (with libvirt installed), run the install command, or `machina-agent join --controller URL --token TOKEN`.
3. The host appears on [Hosts](platform-hosts.md) once its first heartbeat arrives. Run **Validate** there.
4. **Recent tokens** lists issued tokens with their state. Used and revoked tokens stay for audit.

## If something is wrong

- **Token expired:** generate a new one; tokens are single-use and time-limited.
- **Host never appears:** check that the host resolves and reaches the controller URL in the command, and that `machina-agent` is running (`journalctl -u machina-agent`).
- Deploying the controller and agent together on a test host: `./scripts/deploy-remote.sh USER@HOST --platform`.

## Related pages

- [Hosts](platform-hosts.md)
- [Upgrade Matrix](platform-upgrade.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)

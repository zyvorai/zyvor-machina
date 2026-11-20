# Common workflows

End-to-end jobs across Machina Core, Mission Control, and Fleet Cloud. Prefer console click-paths.

## Create and console into a VM (this host)

1. **Core → Create VM** (`/create`) — install media or golden clone
2. **Core → Virtual Machines** (`/vms`) → open guest → **Console** / SSH
3. Optional: **Infrastructure → Containers** if you need Vessel containers on the same host

## Fleet day-2 (Mission Control)

1. **Platform → Mission Control** (`/platform`) — Launchpad tiles live here (no separate `/platform/launchpad`)
2. **Hosts** (`/platform/hosts`) / **Machine Finder** (`/platform/vms`)
3. **Time Machine** backups (`/platform/backups`) as needed
4. **Zyra AI** (`/platform/zyra`) for fleet intelligence; **Zeus Security** (`/platform/zeus/security`) for Security Center

## Fleet Cloud

1. **Fleet Cloud → Overview** (`/fleet-cloud`)
2. **Create Instance** → pick image / flavor / network / keypair / security groups
3. Manage **Volumes**, **Networking**, **Load Balancers** as required — native Machina APIs (no external cloud wiring)

## Migrate a VM in

1. **Migration Assistant** (`/platform/migration`) — scan vCenter/ESXi/OVF/VMDK
2. Import wizard → confirm in **Machine Finder** (`/platform/vms?lens=migration`)
3. Placement / HA (`/platform/placement`) if you need DRS-style move

## First-hour health check

1. Open `https://<host>:5092` — PAM/OIDC sign-in
2. **System Check** (`/system-check`)
3. **Dashboard** or **Mission Control** if enrolled
4. Create a test VM and open console

## Related

- [Getting Started](getting-started.md)
- [Using the Dashboard](using-the-dashboard.md)
- [Page-by-page guides](pages/README.md)
- [Admin basics](admin-basics.md)

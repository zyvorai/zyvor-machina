# VM daily access (SSH, VNC, export, ports)

Operators use the **Connect** hub on VM detail (platform and classic) for everyday tasks: graphical console, SSH, copy commands, guest ports, NAT exposure, and exports.

See also: [Platform VM Detail UX](platform-vm-detail-ux.md) (hero, action bar, Access tab layout).

## Connect hub (platform)

On **VM detail → Overview**, a single **Connect** card consolidates what used to be separate Daily access, laptop checklist, and inline NAT panels:

| Section | Actions |
|---------|---------|
| **Connect** | **Open Cinema** — ConsoleHub / noVNC; **SSH** — in-browser terminal (hypervisor keys); copy guest IP and NAT-aware laptop `ssh` command |
| **Laptop path** | Inline checklist when the guest is on libvirt NAT (e.g. `192.168.122.x`): VM running → guest IP → expose SSH → copy laptop command |
| **Expose service** | Presets (SSH, HTTP, HTTPS, …) and custom port form; active NAT rules in a compact table (expand on Overview, full panel on **Access** tab) |
| **Export** | Download spec JSON, domain XML, both, or copy spec (shown on **Access** tab) |

The card keeps `data-testid="vm-daily-access"` and `vm-laptop-access-checklist` for automation.

### Access tab

**VM detail → Access** is the deep-work surface for connectivity:

- Full Connect hub with NAT panel expanded and export actions
- Links to **Guest security & ports** and **Guest health** tabs
- Same NAT presets as **Network → Hypervisor NAT** and ConsoleHub **Network** lens

Use `?tab=access` to deep-link directly.

### Header action bar

The page header shows **Open Cinema**, **SSH**, one contextual power action (Start / Shutdown / Resume), and a **Power & more** overflow menu (pause, reboot, NMI, force stop, Studio, Virt-Viewer, pop out, delete). **Ask Zyra** opens Spotlight with VM blockers and suggested intents.

### Attention stack

Above the tabs, **at most one** banner is expanded (pending config shutdown, guest agent offline, or incomplete laptop NAT path). Lower-priority items appear as compact chips; dismiss persists per VM.

## Classic VM detail

On classic **VM detail**, the Daily access strip below the power toolbar mirrors the Connect hub layout.

## SSH from your laptop

- Machina does not store private keys.
- Cloud-init VMs are often **SSH-key only** — use the private key that matches the injected public key: `ssh -i ~/.ssh/id_ed25519 -p 2222 ubuntu@HYPERVISOR_IP`.
- The UI prefers the cloud-init user (e.g. `ubuntu`) over stored SSH prefs when available.
- The UI copies hypervisor host + NAT port automatically when a rule exists (default SSH preset: host `2222` → guest `22`).
- Private guest IPs are **not** reachable directly from your laptop — always connect via the hypervisor address and NAT port.

## In-browser SSH (ConsoleHub Shell)

- Uses the hypervisor daemon’s SSH keys, not your cloud-init key.
- When NAT is exposed, ConsoleHub dials `127.0.0.1:NAT_PORT` on the hypervisor.
- If login fails, expose SSH and connect from your laptop with your private key.

## VNC

- **Platform:** `/platform/vms/{id}/consolehub` — Machine Cockpit with Display / Serial / Shell lenses.
- **Classic:** `/vms/{name}/console` — same noVNC viewer.
- Linux cloud images often log in on **Serial** first; use ConsoleHub’s Serial lens or the recovery card when password login is not configured.

## Ports (platform)

Requires QEMU guest agent / Machina guest tools. Lists in-guest listeners via `GET /api/v1/zeus-firewall/vms/{id}/guest-ports`.

**NAT port forwarding** is managed on **VM detail → Access** or **Network → Hypervisor NAT**, in ConsoleHub **Network** lens, and inline from scanned ports / guest-access banners.

Known presets include SSH (2222→22), HTTP (9080→80), HTTPS (9443→443), databases, Grafana, etc. Custom named services can be saved per VM (server-side templates).

## Platform Finder

In **Virtual Machines** column/list view, the inspector shows **VNC**, **SSH**, and **Copy IP** when a guest address is cached on the VM record.

## API reference

| Method | Path | Use |
|--------|------|-----|
| `GET` | `/api/v1/vms/{id}/console` | VNC WebSocket path |
| `GET` | `/api/v1/vms/{id}/consolehub/plan` | ConsoleHub plan incl. `guest_access`, `ssh_connect_host/port` |
| `GET` | `/api/v1/vms/{id}/guest/health` | Guest IP, tools status |
| `GET` | `/api/v1/zeus-firewall/vms/{id}/guest-ports` | Listening ports |
| `GET` | `/api/v1/vms/{id}/port-forwards` | Active NAT rules for guest IP |
| `POST` | `/api/v1/vms/{id}/port-forwards` | Create NAT rule |
| `GET/POST` | `/api/v1/vms/{id}/port-forward-templates` | Custom service templates |
| `GET` | `/api/v1/vms/{id}/spec` | VM spec JSON |
| `GET` | `/api/v1/vms/{id}/domain-xml` | Live libvirt XML |

List VMs may include `guest_ip` and `guest_tools_status` when synced from health checks.

## Automation

```bash
./scripts/e2e-vm-lifecycle-remote.sh user host --ssh-key ~/.ssh/id_ed25519
```

Verifies console API, guest ports, SSH login, and spec/XML export. Platform smoke tests cover NAT expose and ConsoleHub guest-access actions when VMs exist.

## See also

- [VM lifecycle and SSH keys](vm-lifecycle-ssh.md)

# Windows golden images (dockur / Podman)

Machina builds Windows 10/11 golden `qcow2` disks via [dockur/windows](https://github.com/dockur/windows) inside **Podman** on the hypervisor.

## Prerequisites

- Linux host with **KVM** (`/dev/kvm`)
- **Podman** (Machina `install.sh` installs it on supported distros)
- `[libvirt] dockur_windows_allowed = true` in `/etc/machina/config.toml`
- Free disk: default **64G** qcow2 per build (override with `dockur_disk_size` in config.toml)

## API / UI

- `POST /api/v1/jobs/packer-golden-build` with `{"guest":"win11"}`, `{"guest":"win10"}`, `{"guest":"windows-server-2022"}`, or `{"guest":"windows-server-2025"}`
- Create VM → **Golden Forge** → pick Windows profile → **Build golden qcow2**
- Artifact: `/var/lib/machina/packer-builds/{job-id}/work/output-win11/win11.qcow2` (same pattern for each guest id)

## Script (manual)

```bash
cd /var/lib/machina/packer-builds/manual
sudo /usr/local/share/machina/packer/build-windows-dockur.sh win11 work
```

## Environment overrides

| Variable | Default |
|----------|---------|
| `MACHINA_DOCKUR_IMAGE` | `docker.io/dockurr/windows` |
| `MACHINA_DOCKUR_DISK_SIZE` | `64G` |
| `MACHINA_DOCKUR_RAM_SIZE` | `4G` |
| `MACHINA_DOCKUR_CPU_CORES` | `2` |
| `MACHINA_DOCKUR_USERNAME` | `Docker` |
| `MACHINA_DOCKUR_PASSWORD` | `admin` |
| `MACHINA_DOCKUR_MAX_WAIT_SECS` | `7200` |

## Security

Default credentials are **known** (`Docker` / `admin`). Rotate before exposing VMs to untrusted networks. For manual unattended Packer + VirtIO, see `../windows-qemu/HOWTO.txt`.

## Vessel (Podman / Docker)

Windows templates can also run **directly as containers** (no libvirt clone):

```bash
sudo /usr/local/share/machina/packer/run-windows-dockur.sh win11
# or API: POST /api/v1/vessel/windows-dockur  {"guest":"win11","use_golden":true}
```

- Runtime: **Podman preferred**, **Docker** if Podman is absent
- Uses golden `/var/lib/libvirt/images/{guest}.qcow2` when present; otherwise installs into `/var/lib/machina/vessel-windows/{guest}`
- Web viewer: `http://127.0.0.1:8006` · RDP: `127.0.0.1:3389`
- UI: **Containers** → **Windows 11** / **Windows 10**

Requires `[libvirt] dockur_windows_allowed = true` and `/dev/kvm`.

## libvirt clone + KubeVirt

- Firmware: **UEFI** (required for dockur goldens)
- Disk bus: **virtio** (dockur installs VirtIO drivers on clean install)
- Graphics: SPICE or VNC per your console policy
- After a successful Golden Forge job, Machina copies the disk to `/var/lib/libvirt/images/{win10|win11}.qcow2`, writes `/var/lib/machina/templates/{guest}.json`, and seeds Platform marketplace templates `win10` / `win11`.
- **KubeVirt:** Disk Images → KubeVirt YAML (or `GET /api/v1/kubevirt/qcow2-bundle?qcow2_path=/var/lib/libvirt/images/win11.qcow2&guest_os=windows`). Generated manifests include **EFI** + **TPM** + RDP 3389 so the dockur qcow2 can boot on KubeVirt.

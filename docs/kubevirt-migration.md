# KubeVirt migration (libvirt → CDI + VirtualMachine)

machina can emit a **CDI upload DataVolume** plus a **KubeVirt VirtualMachine** manifest from an existing libvirt domain, similar in spirit to [hyper2kvm](https://github.com/ssahani/hyper2kvm): root disk as a DataVolume, virtio disks in the guest, and a **virtio-win CDROM** expressed as a `containerDisk` (cluster-pullable image) instead of attaching `virtio-win.iso` on the hypervisor.

Installing or upgrading the **daemon on the hypervisor** (Rust build, web UI, systemd) is covered in the main [README.md](../README.md): `install.sh`, `scripts/deploy-remote.sh` (including `--remote-check` / `--remote-build` for compile-only runs after rsync).

## API

- `GET /api/v1/vms/{name}/kubevirt-bundle` — JSON with `yaml`, `virtctl_image_upload_example`, `libvirt_root_disk`, Kubernetes names, `upload_size_gi`, and `cluster_exec_enabled`.
- Optional cluster execution on the **daemon host** (same machine as libvirt), gated by config:
  - `POST /api/v1/vms/{name}/kubevirt/apply` — `kubectl apply -f` the generated YAML.
  - `POST /api/v1/vms/{name}/kubevirt/upload` — `virtctl image-upload` from `libvirt_root_disk` into the upload DataVolume (can run a long time; the HTTP request waits until `virtctl` exits).
  - `POST /api/v1/vms/{name}/kubevirt/start` — `virtctl start` for the generated VM.

POST bodies accept the same optional overrides as the GET query (`namespace`, `k8s_vm_name`, `datavolume_name`, `storage_gi`, `storage_class`, `include_virtio_cdrom`). Responses are `{ "exit_code", "stdout", "stderr" }`; **always check `exit_code`** — non-zero still returns HTTP 200 so the UI can show logs.

## Configuration (`[kubevirt]`)

See `examples/config.toml`. Important keys:

| Key | Role |
|-----|------|
| `exec_enabled` | Must be `true` for POST `kubevirt/*` to run `kubectl` / `virtctl`. |
| `kubectl_binary` / `virtctl_binary` | Defaults `kubectl` / `virtctl` on `PATH`. |
| `kubeconfig_path` | If set, exported as `KUBECONFIG` for those commands. |
| `upload_timeout_minutes` | Passed to `virtctl image-upload --upload-image-timeout=…m`. |
| `default_namespace`, `default_storage_class`, `datavolume_padding_gi`, `virtio_container_disk_image`, `machine_type` | Affect generated YAML and sizing. |

## UI and TUI

- **Web**: VM details → **KubeVirt YAML** opens the bundle; when `cluster_exec_enabled` is true, buttons run apply / upload / start on the daemon.
- **TUI** (`:` command mode): `:browse` / `:browse /path`, `:kubevirt-bundle <vm>`, `:kubevirt-apply <vm>`, `:kubevirt-upload <vm>`, `:kubevirt-start <vm>`.

## Typical workflow

1. Ensure the cluster has CDI and KubeVirt; pick namespace and storage class in config or query/body overrides.
2. `virtctl image-upload` (or POST upload) so the libvirt root image fills the upload DataVolume.
3. `kubectl apply` the YAML (or POST apply) — DataVolume + VirtualMachine.
4. `virtctl start` (or POST start) when you want the guest running on Kubernetes.

The generated VM uses `runStrategy: Halted` until you start it.

## Windows (dockur Golden Forge) → KubeVirt

1. Enable `[libvirt] dockur_windows_allowed = true`, then build `win10` / `win11` via Golden Forge (`POST /api/v1/jobs/packer-golden-build`).
2. On success the daemon copies the disk to `/var/lib/libvirt/images/{guest}.qcow2` and registers a saved template.
3. From **Disk Images** (or `GET /api/v1/kubevirt/qcow2-bundle?qcow2_path=/var/lib/libvirt/images/win11.qcow2&guest_os=windows`) generate CDI + VM YAML.
4. Windows manifests include **EFI** (`secureBoot: false`), **TPM**, Hyper-V enlightenments, virtio root, and RDP port 3389 — required for dockur UEFI goldens to boot on KubeVirt.

Marketplace template ids are `win10` / `win11` (replacing the older `windows-11` / `windows-server-*` catalog rows).

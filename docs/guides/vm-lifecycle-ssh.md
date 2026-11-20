# VM lifecycle and SSH keys

Machina does **not** store private SSH keys. Use your existing OpenSSH workflow: inject a **public** key at create time, keep the private key on your workstation or in your agent.

## Platform UI

### Create VM

1. Open **Virtual Machines** → **Create VM** (or the dashboard launchpad).
2. Expand **Advanced** → **SSH public key**.
3. Paste a public key or use **Import public key (.pub)**.
4. The key is written into the VM spec as `cloud_init.ssh_pubkey` and applied on first boot via NoCloud.

Use the same private key when you connect: `ssh -i ~/.ssh/id_ed25519 ubuntu@<guest-ip>`.

### VM detail — power and access

| Action | Description |
|--------|-------------|
| **Shutdown** | Graceful ACPI shutdown |
| **Pause** / **Resume** | Libvirt pause/resume |
| **Force stop** | Immediate destroy (was “Stop”) |
| **SSH** | Opens the in-browser terminal at `/ssh` when guest tools report an IP |
| **Download spec / domain XML** | Export JSON spec and live libvirt XML |

**SSH** uses `GET /api/v1/vms/{id}/guest/health` (`guest_ip`) and the cloud-init user from the VM spec (default `ubuntu`).

## Controller API

| Method | Path |
|--------|------|
| `POST` | `/api/v1/vms/{id}/shutdown` |
| `POST` | `/api/v1/vms/{id}/pause` |
| `POST` | `/api/v1/vms/{id}/resume` |
| `POST` | `/api/v1/vms/{id}/stop` (force) |
| `GET` | `/api/v1/vms/{id}/domain-xml` |
| `GET` | `/api/v1/vms/{id}/guest/health` |
| `POST` | `/api/v1/vms/from-template` (body: `cloud_init_ssh_pubkey`, `cloud_init_user`) |

Power actions enqueue `vm.power` tasks on the agent (libvirt).

## Remote automation

### Deploy with a specific key

```bash
./scripts/deploy-remote.sh user@host --platform --ssh-key ~/.ssh/id_ed25519
```

Sets `IdentityFile` for rsync and SSH (with `IdentitiesOnly=yes`).

### Full lifecycle E2E

```bash
export VSPASS='your-pam-password'
./scripts/e2e-vm-lifecycle-remote.sh user <ephemeral-ip> \
  --ssh-key ~/.ssh/id_ed25519 \
  --template ubuntu-24.04 \
  [--deploy] [--keep-vm]
```

The script:

1. Derives the public key with `ssh-keygen -y`
2. Optionally runs `deploy-remote.sh --ssh-key`
3. Creates a VM from template with cloud-init SSH access
4. Exercises pause, resume, snapshot, guest SSH, spec/XML export, shutdown, and force stop
5. Deletes the VM unless `--keep-vm` is set

Platform smoke and `scripts/e2e-platform-test.sh` also cover pause/shutdown/guest-health endpoints when VMs exist on the controller.

For VNC, guest ports, copy shortcuts, and the unified **Daily access** panel, see [VM daily access](vm-daily-access.md).

## Out of scope (v1)

- Private key vault in Machina
- QCOW2 disk download/export
- KubeVirt VM power (libvirt platform VMs only)

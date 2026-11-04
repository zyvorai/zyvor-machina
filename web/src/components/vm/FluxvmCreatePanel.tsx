// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { createVMWithProgress, type CreateVmRequest } from '../../api/vm'
import { formatUserError } from '../../utils/apiError'

export const FLUXVM_HYPERVISORS = ['auto', 'qemu', 'cloud-hypervisor', 'firecracker', 'flux-vm'] as const

/** Every mode but `user`/`none` gives the guest a tap, which FluxVM's eBPF VM edge attaches to. */
export const FLUXVM_NETWORKS = [
  { id: 'netns', label: 'eBPF namespace (default)', hint: 'Tap in its own network namespace with DHCP + NAT; eBPF VM edge attached' },
  { id: 'bridge', label: 'eBPF on a host bridge', hint: 'Tap on an existing host bridge; eBPF VM edge attached' },
  { id: 'direct', label: 'eBPF direct uplink', hint: 'No bridge: TC/eBPF redirect between a host NIC and the tap' },
  { id: 'user', label: 'User-mode NAT (forces QEMU)', hint: 'No tap, so no eBPF policy or accounting; auto picks QEMU' },
  { id: 'none', label: 'No network', hint: '' },
] as const
export type FluxvmNetwork = (typeof FLUXVM_NETWORKS)[number]['id']

type Props = {
  initialName?: string
  defaultHypervisor?: string
  onCreated: (name: string) => void
  onError: (message: string) => void
}

/** Build the daemon create request for a FluxVM guest (`backend: 'fluxvm'`). */
export function buildFluxvmCreateRequest(p: {
  name: string
  vcpus: number
  memoryMb: number
  diskGb: number
  hypervisor: string
  image: string
  network?: FluxvmNetwork
  bridge?: string
  directUplink?: string
  directMode?: string
  directGuestIps?: string
  cloudInitUser?: string
  cloudInitSshKey?: string
  kernel?: string
  initrd?: string
  kernelArgs?: string
  agent?: boolean
  sharedDisk?: boolean
  /** Install ISO paths, one per line or comma-separated. */
  isos?: string
  secureBoot?: boolean
  vtpm?: boolean
  firmware?: string
  balloon?: boolean
}): CreateVmRequest {
  const network = p.network ?? 'netns'
  const isos = splitIsos(p.isos)
  const net: Partial<CreateVmRequest> =
    network === 'bridge'
      ? { fluxvm_bridge: p.bridge?.trim() || undefined }
      : network === 'direct'
        ? {
            fluxvm_direct_uplink: p.directUplink?.trim() || undefined,
            fluxvm_direct_mode: p.directMode?.trim() || undefined,
            fluxvm_direct_guest_ips: (p.directGuestIps ?? '').split(/[\s,]+/).filter(Boolean),
          }
        : { fluxvm_network: network }
  return {
    name: p.name.trim(),
    vcpus: p.vcpus,
    memory_mb: p.memoryMb,
    disk_gb: p.diskGb,
    backend: 'fluxvm',
    fluxvm_backend: p.hypervisor,
    fluxvm_image: p.image.trim(),
    ...net,
    cloud_init_user: p.cloudInitUser?.trim() || undefined,
    cloud_init_ssh_pubkey: p.cloudInitSshKey?.trim() || undefined,
    fluxvm_kernel: p.kernel?.trim() || undefined,
    fluxvm_initrd: p.initrd?.trim() || undefined,
    fluxvm_kernel_args: p.kernelArgs?.trim() || undefined,
    fluxvm_agent: p.agent === false ? false : undefined,
    fluxvm_shared_disk: p.sharedDisk || undefined,
    fluxvm_isos: isos.length > 0 ? isos : undefined,
    secure_boot: p.secureBoot || undefined,
    vtpm: p.vtpm || undefined,
    fluxvm_firmware: p.firmware?.trim() || undefined,
    fluxvm_balloon: p.balloon || undefined,
  }
}

export const FLUXVM_MAX_ISOS = 4

function splitIsos(raw?: string): string[] {
  return (raw ?? '').split(/[\n,]+/).map((s) => s.trim()).filter(Boolean)
}

export default function FluxvmCreatePanel({ initialName = '', defaultHypervisor, onCreated, onError }: Props) {
  const initial = FLUXVM_HYPERVISORS.find((h) => h === defaultHypervisor) ?? 'auto'
  const [vmName, setVmName] = useState(initialName)
  const [vcpus, setVcpus] = useState(2)
  const [memoryMb, setMemoryMb] = useState(2048)
  const [diskGb, setDiskGb] = useState(20)
  const [cloudInitUser, setCloudInitUser] = useState('')
  const [cloudInitSshKey, setCloudInitSshKey] = useState('')
  const [hypervisor, setHypervisor] = useState<string>(initial)
  const [image, setImage] = useState('')
  const [network, setNetwork] = useState<FluxvmNetwork>('netns')
  const [bridge, setBridge] = useState('')
  const [directUplink, setDirectUplink] = useState('')
  const [directMode, setDirectMode] = useState('l2-uplink')
  const [directGuestIps, setDirectGuestIps] = useState('')
  const [kernel, setKernel] = useState('')
  const [initrd, setInitrd] = useState('')
  const [kernelArgs, setKernelArgs] = useState('')
  const [agent, setAgent] = useState(true)
  const [sharedDisk, setSharedDisk] = useState(false)
  const [isos, setIsos] = useState('')
  const [secureBoot, setSecureBoot] = useState(false)
  const [vtpm, setVtpm] = useState(false)
  const [firmware, setFirmware] = useState('')
  const [balloon, setBalloon] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [log, setLog] = useState<string[]>([])

  const blocked = !vmName.trim()
    ? 'Enter a VM name first.'
    : !image.trim()
      ? 'Enter the base image path on the FluxVM host.'
      : network === 'bridge' && !bridge.trim()
        ? 'Enter the host bridge.'
        : network === 'direct' && !directUplink.trim()
          ? 'Enter the host uplink NIC.'
          : network === 'user' && hypervisor !== 'qemu' && hypervisor !== 'auto'
            ? 'User-mode NAT needs the qemu (or auto) hypervisor.'
            : sharedDisk && hypervisor === 'flux-vm'
              ? 'flux-vm needs FluxVM-managed storage; pick another hypervisor for a shared disk.'
              : splitIsos(isos).length > 0 && hypervisor !== 'qemu' && hypervisor !== 'auto'
                ? 'Install ISOs need the qemu (or auto) hypervisor.'
                : splitIsos(isos).length > FLUXVM_MAX_ISOS
                  ? `At most ${FLUXVM_MAX_ISOS} install ISOs.`
                  : null

  const submit = async () => {
    if (blocked) return
    const req = buildFluxvmCreateRequest({
      name: vmName, vcpus, memoryMb, diskGb, hypervisor, image,
      network, bridge, directUplink, directMode, directGuestIps,
      cloudInitUser, cloudInitSshKey, kernel, initrd, kernelArgs, agent, sharedDisk, isos,
      secureBoot, vtpm, firmware, balloon,
    })
    setSubmitting(true)
    setLog([])
    try {
      await createVMWithProgress(req, (line) => setLog((prev) => [...prev, line]))
      onCreated(req.name)
    } catch (e: unknown) {
      onError(formatUserError(e))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)] space-y-4" data-testid="fluxvm-create-panel">
      <div>
        <h3 className="text-sm font-semibold text-[var(--text-primary)]">FluxVM guest</h3>
        <p className="text-xs text-[var(--text-muted)]">
          Created through fluxvm-api instead of libvirt. Lifecycle, metrics, consoles, snapshots, backups (QEMU), hot-add (QEMU / Cloud Hypervisor) and live migration (QEMU on a shared disk) work from Machina.
        </p>
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="sm:col-span-2">
          <label htmlFor="fluxvm-name" className="block text-sm text-[var(--text-muted)] mb-1">Name *</label>
          <input id="fluxvm-name" type="text" value={vmName} onChange={(e) => setVmName(e.target.value)} className="input-field w-full text-sm" placeholder="my-microvm" />
        </div>
        <div>
          <label htmlFor="fluxvm-vcpus" className="block text-sm text-[var(--text-muted)] mb-1">vCPUs</label>
          <input id="fluxvm-vcpus" type="number" min={1} max={255} value={vcpus} onChange={(e) => setVcpus(Math.max(1, Number(e.target.value) || 1))} className="input-field w-full text-sm" />
        </div>
        <div>
          <label htmlFor="fluxvm-memory" className="block text-sm text-[var(--text-muted)] mb-1">Memory (MiB)</label>
          <input id="fluxvm-memory" type="number" min={128} step={128} value={memoryMb} onChange={(e) => setMemoryMb(Math.max(128, Number(e.target.value) || 128))} className="input-field w-full text-sm" />
        </div>
        <div>
          <label htmlFor="fluxvm-disk" className="block text-sm text-[var(--text-muted)] mb-1">Disk (GiB, 0 = image size)</label>
          <input id="fluxvm-disk" type="number" min={0} value={diskGb} onChange={(e) => setDiskGb(Math.max(0, Number(e.target.value) || 0))} className="input-field w-full text-sm" />
        </div>
        <div>
          <label htmlFor="fluxvm-hypervisor" className="block text-sm text-[var(--text-muted)] mb-1">Hypervisor</label>
          <select id="fluxvm-hypervisor" value={hypervisor} onChange={(e) => setHypervisor(e.target.value)} className="input-field w-full text-sm">
            {FLUXVM_HYPERVISORS.map((h) => (
              <option key={h} value={h}>{h}</option>
            ))}
          </select>
        </div>
        <div>
          <label htmlFor="fluxvm-network" className="block text-sm text-[var(--text-muted)] mb-1">Network</label>
          <select id="fluxvm-network" value={network} onChange={(e) => setNetwork(e.target.value as FluxvmNetwork)} className="input-field w-full text-sm">
            {FLUXVM_NETWORKS.map((n) => (
              <option key={n.id} value={n.id}>{n.label}</option>
            ))}
          </select>
          <p className="text-xs text-[var(--text-muted)] mt-1">{FLUXVM_NETWORKS.find((n) => n.id === network)?.hint}</p>
        </div>
        {network === 'bridge' && (
          <div className="sm:col-span-2">
            <label htmlFor="fluxvm-bridge" className="block text-sm text-[var(--text-muted)] mb-1">Host bridge *</label>
            <input id="fluxvm-bridge" type="text" value={bridge} onChange={(e) => setBridge(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="br0" />
          </div>
        )}
        {network === 'direct' && (
          <>
            <div>
              <label htmlFor="fluxvm-uplink" className="block text-sm text-[var(--text-muted)] mb-1">Host uplink NIC *</label>
              <input id="fluxvm-uplink" type="text" value={directUplink} onChange={(e) => setDirectUplink(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="eno1" />
            </div>
            <div>
              <label htmlFor="fluxvm-direct-mode" className="block text-sm text-[var(--text-muted)] mb-1">Redirect mode</label>
              <select id="fluxvm-direct-mode" value={directMode} onChange={(e) => setDirectMode(e.target.value)} className="input-field w-full text-sm">
                <option value="l2-uplink">l2-uplink (physical / bond NIC)</option>
                <option value="peer-veth">peer-veth (CNI pod eth0)</option>
              </select>
            </div>
            {directMode === 'l2-uplink' && (
              <div className="sm:col-span-2">
                <label htmlFor="fluxvm-guest-ips" className="block text-sm text-[var(--text-muted)] mb-1">Guest IPv4s for ARP steering (optional, up to 8)</label>
                <input id="fluxvm-guest-ips" type="text" value={directGuestIps} onChange={(e) => setDirectGuestIps(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="192.0.2.10, 192.0.2.11" />
              </div>
            )}
          </>
        )}
        <div className="sm:col-span-2">
          <label htmlFor="fluxvm-image" className="block text-sm text-[var(--text-muted)] mb-1">{sharedDisk ? 'Shared raw disk on the FluxVM host *' : 'Base image on the FluxVM host *'}</label>
          <input
            id="fluxvm-image"
            type="text"
            value={image}
            onChange={(e) => setImage(e.target.value)}
            className="input-field w-full font-mono text-sm"
            placeholder={sharedDisk ? '/var/lib/fluxvm/shared/web-1.raw' : '/var/lib/fluxvm/images/ubuntu-24.04.qcow2'}
          />
          <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] mt-2">
            <input type="checkbox" checked={sharedDisk} onChange={(e) => setSharedDisk(e.target.checked)} data-testid="fluxvm-shared-disk" />
            Use this raw file / block device in place (shared storage) — needed for live migration and HA; never deleted
          </label>
        </div>
        <div className="sm:col-span-2">
          <label htmlFor="fluxvm-isos" className="block text-sm text-[var(--text-muted)] mb-1">Install ISOs (optional, QEMU, up to {FLUXVM_MAX_ISOS})</label>
          <textarea id="fluxvm-isos" rows={2} value={isos} onChange={(e) => setIsos(e.target.value)} className="input-field w-full font-mono text-sm" placeholder={'/var/lib/fluxvm/images/win11.iso\n/var/lib/fluxvm/images/virtio-win.iso'} />
          <p className="text-xs text-[var(--text-muted)] mt-1">Attached as CD-ROMs install, cd2, … For a fresh install, point the image at an empty raw file. Eject them from the Manage tab before migrating.</p>
        </div>
        <div>
          <label htmlFor="fluxvm-kernel" className="block text-sm text-[var(--text-muted)] mb-1">Kernel (optional)</label>
          <input id="fluxvm-kernel" type="text" value={kernel} onChange={(e) => setKernel(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="/var/lib/fluxvm/kernels/vmlinux (QEMU: bzImage)" />
        </div>
        <div>
          <label htmlFor="fluxvm-initrd" className="block text-sm text-[var(--text-muted)] mb-1">Initrd (optional)</label>
          <input id="fluxvm-initrd" type="text" value={initrd} onChange={(e) => setInitrd(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="/var/lib/fluxvm/kernels/initrd.img" />
        </div>
        <div className="sm:col-span-2">
          <label htmlFor="fluxvm-kernel-args" className="block text-sm text-[var(--text-muted)] mb-1">Extra kernel args (optional)</label>
          <input id="fluxvm-kernel-args" type="text" value={kernelArgs} onChange={(e) => setKernelArgs(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="root=/dev/vda rw" />
          <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] mt-2">
            <input type="checkbox" checked={agent} onChange={(e) => setAgent(e.target.checked)} data-testid="fluxvm-agent" />
            FluxVM guest agent (agent console and exec; the image must run fluxvm-agent)
          </label>
        </div>
        <div className="sm:col-span-2 grid gap-3 sm:grid-cols-2">
          <div>
            <label htmlFor="fluxvm-firmware" className="block text-sm text-[var(--text-muted)] mb-1">Firmware path (optional, QEMU)</label>
            <input id="fluxvm-firmware" type="text" value={firmware} onChange={(e) => setFirmware(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="/usr/share/OVMF/OVMF_CODE_4M.fd" />
          </div>
          <div className="flex flex-col gap-2 justify-end pb-1">
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
              <input type="checkbox" checked={secureBoot} onChange={(e) => setSecureBoot(e.target.checked)} data-testid="fluxvm-secure-boot" />
              UEFI Secure Boot (QEMU; needs host OVMF SB keys)
            </label>
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
              <input type="checkbox" checked={vtpm} onChange={(e) => setVtpm(e.target.checked)} data-testid="fluxvm-tpm" />
              TPM 2.0 (swtpm; Windows 11)
            </label>
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
              <input type="checkbox" checked={balloon} onChange={(e) => setBalloon(e.target.checked)} data-testid="fluxvm-balloon" />
              Virtio balloon (QEMU)
            </label>
          </div>
        </div>
        <div>
          <label htmlFor="fluxvm-ci-user" className="block text-sm text-[var(--text-muted)] mb-1">Cloud-init user (optional)</label>
          <input id="fluxvm-ci-user" type="text" value={cloudInitUser} onChange={(e) => setCloudInitUser(e.target.value)} className="input-field w-full text-sm" placeholder="ubuntu" />
        </div>
        <div>
          <label htmlFor="fluxvm-ci-key" className="block text-sm text-[var(--text-muted)] mb-1">SSH public key (optional)</label>
          <input id="fluxvm-ci-key" type="text" value={cloudInitSshKey} onChange={(e) => setCloudInitSshKey(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="ssh-ed25519 AAAA…" />
        </div>
      </div>
      {log.length > 0 && (
        <pre className="text-xs font-mono text-[var(--text-secondary)] bg-[var(--surface-hover)] rounded-lg p-3 max-h-40 overflow-auto">{log.join('\n')}</pre>
      )}
      <div className="flex items-center gap-3 flex-wrap">
        <button type="button" onClick={submit} disabled={submitting || !!blocked} className="btn-primary text-sm disabled:opacity-50 px-8 py-3">
          {submitting ? 'Creating…' : 'Create FluxVM guest'}
        </button>
        {blocked && <span className="text-xs text-[var(--text-muted)]">{blocked}</span>}
      </div>
    </div>
  )
}

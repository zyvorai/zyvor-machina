// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { MacGlassPanel } from './mac/PlatformMacUi'
import CollapsibleCodeBlock from '../CollapsibleCodeBlock'
import {
  getVmDomainXml,
  type VmLibvirtDetails,
} from '../../api/platform'
import {
  invokeVmLibvirt,
  putVmDomainXml,
  queryHostLibvirt,
  queryVmLibvirt,
} from '../../api/platformVmLibvirt'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'

type Props = {
  vmId: string
  hostId?: string | null
  vmName: string
  managed?: boolean
  libvirtDetails: VmLibvirtDetails | null
  onChanged?: () => void
}

export default function PlatformVmAdvanced({
  vmId,
  hostId,
  vmName,
  managed,
  libvirtDetails,
  onChanged,
}: Props) {
  const toast = useToastContext()
  const disabled = managed === false

  const [blockDisk, setBlockDisk] = useState('vda')
  const [blockJob, setBlockJob] = useState<unknown>(null)
  const [cputune, setCputune] = useState<{ shares?: number; period?: number; quota?: number } | null>(null)
  const [memtune, setMemtune] = useState<{ hard_limit_kb?: number; soft_limit_kb?: number; swap_hard_limit_kb?: number }>({})
  const [bootDevices, setBootDevices] = useState('hd,cdrom')
  const [liveVcpus, setLiveVcpus] = useState('')
  const [liveMemoryGiB, setLiveMemoryGiB] = useState('')
  const [pinVcpu, setPinVcpu] = useState('0')
  const [pinCpus, setPinCpus] = useState('0,1')
  const [diskTuneTarget, setDiskTuneTarget] = useState('vda')
  const [diskCache, setDiskCache] = useState('writeback')
  const [nicMac, setNicMac] = useState('')
  const [usbVid, setUsbVid] = useState('')
  const [usbPid, setUsbPid] = useState('')
  const [pciBdf, setPciBdf] = useState('')
  const [uefi, setUefi] = useState(false)
  const [domainXml, setDomainXml] = useState('')
  // Once the user edits the Domain XML, don't let a background refetch (fired by
  // any sibling Apply / hardware refresh) clobber their unsaved edits.
  const xmlDirty = useRef(false)
  const [isos, setIsos] = useState<Array<{ name: string; path?: string; size_bytes?: number }>>([])
  const [usbDevices, setUsbDevices] = useState<Array<{ vendor_id: string; product_id: string; description?: string }>>([])
  const [pciDevices, setPciDevices] = useState<Array<{ slot: string; vendor?: string; device?: string }>>([])
  const [loading, setLoading] = useState(false)

  const run = useCallback(
    async (label: string, fn: () => Promise<unknown>) => {
      try {
        await fn()
        toast.success(label)
        onChanged?.()
      } catch (e: unknown) {
        toast.error(formatUserError(e))
      }
    },
    [onChanged, toast],
  )

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [xml, ct, mt, boot] = await Promise.all([
        getVmDomainXml(vmId).catch(() => ({ xml: '' })),
        queryVmLibvirt<{ shares?: number; period?: number; quota?: number }>(vmId, 'cputune.get').catch(() => null),
        queryVmLibvirt<{ hard_limit_kb?: number; soft_limit_kb?: number; swap_hard_limit_kb?: number }>(vmId, 'memtune.get').catch(() => null),
        queryVmLibvirt<{ boot_devices: string[]; firmware: string }>(vmId, 'boot.get').catch(() => null),
      ])
      if (!xmlDirty.current) setDomainXml(xml.xml ?? '')
      setCputune(ct)
      if (mt) setMemtune(mt)
      if (boot) {
        setBootDevices(boot.boot_devices.join(','))
        setUefi(boot.firmware.toLowerCase().includes('efi') || boot.firmware.toLowerCase().includes('ovmf'))
      }
      if (libvirtDetails) {
        setLiveVcpus(String(libvirtDetails.vcpus))
        setLiveMemoryGiB(String(Math.round(libvirtDetails.memory_mb / 1024)))
        const firstNic = libvirtDetails.interfaces[0]?.mac_address ?? ''
        setNicMac(firstNic)
        const firstDisk = libvirtDetails.disks.find((d) => d.device === 'disk')?.target ?? 'vda'
        setBlockDisk(firstDisk)
        setDiskTuneTarget(firstDisk)
      }
      if (hostId) {
        const [isoRes, usbRes, pciRes] = await Promise.all([
          queryHostLibvirt<{ files: Array<{ name: string; path?: string; size_bytes?: number }> }>(hostId, 'browse.isos').catch(() => null),
          queryHostLibvirt<Array<{ vendor_id: string; product_id: string; description?: string }>>(hostId, 'host.usb').catch(() => []),
          queryHostLibvirt<Array<{ slot: string; vendor?: string; device?: string }>>(hostId, 'host.pci').catch(() => []),
        ])
        setIsos(isoRes?.files ?? [])
        setUsbDevices(Array.isArray(usbRes) ? usbRes : [])
        setPciDevices(Array.isArray(pciRes) ? pciRes : [])
      }
    } finally {
      setLoading(false)
    }
  }, [hostId, libvirtDetails, vmId])

  useEffect(() => {
    void load()
  }, [load])

  // Switching to a different VM clears the dirty guard so its XML loads fresh.
  useEffect(() => {
    xmlDirty.current = false
  }, [vmId])

  const refreshBlockJob = () =>
    void queryVmLibvirt(vmId, 'block.job', { disk: blockDisk })
      .then(setBlockJob)
      .catch(() => setBlockJob(null))

  if (loading && !domainXml) {
    return <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading advanced controls…</p>
  }

  return (
    <div className="space-y-4 pt-2" data-testid="vm-advanced-panel">
      <MacGlassPanel title="Block jobs" subtitle="Commit, pull, or abort backing-chain operations">
        <div className="flex flex-wrap gap-3 items-end text-sm">
          <label className="text-xs text-[var(--text-muted)]">Disk target
            <input className="input mt-1 block w-24" value={blockDisk} onChange={(e) => setBlockDisk(e.target.value)} />
          </label>
          <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('Block commit started', () => invokeVmLibvirt(vmId, 'block.commit', { disk: blockDisk }))}>Commit</button>
          <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('Block pull started', () => invokeVmLibvirt(vmId, 'block.pull', { disk: blockDisk }))}>Pull</button>
          <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('Block job aborted', () => invokeVmLibvirt(vmId, 'block.job.abort', { disk: blockDisk }))}>Abort</button>
          <button type="button" className="btn-secondary text-xs" onClick={refreshBlockJob}>Refresh status</button>
        </div>
        {blockJob != null && <pre className="text-xs text-[var(--text-muted)] mt-3 overflow-auto">{JSON.stringify(blockJob, null, 2)}</pre>}
      </MacGlassPanel>

      <MacGlassPanel title="Disk & NIC tuning">
        <div className="grid gap-4 md:grid-cols-2 text-sm">
          <div className="space-y-2">
            <p className="text-xs text-[var(--text-muted)]">Disk cache / bus</p>
            <input className="input w-full" placeholder="target vda" value={diskTuneTarget} onChange={(e) => setDiskTuneTarget(e.target.value)} />
            <select className="input w-full" aria-label="Disk cache mode" value={diskCache} onChange={(e) => setDiskCache(e.target.value)}>
              <option value="none">none</option>
              <option value="writethrough">writethrough</option>
              <option value="writeback">writeback</option>
              <option value="directsync">directsync</option>
              <option value="unsafe">unsafe</option>
            </select>
            <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('Disk tune applied', () => invokeVmLibvirt(vmId, 'disk.tune', { target: diskTuneTarget, cache: diskCache }))}>Apply disk tune</button>
          </div>
          <div className="space-y-2">
            <p className="text-xs text-[var(--text-muted)]">NIC model override</p>
            <input aria-label="MAC address" className="input w-full font-mono" placeholder="MAC" value={nicMac} onChange={(e) => setNicMac(e.target.value)} />
            <button type="button" className="btn-secondary text-xs" disabled={disabled || !nicMac} onClick={() => void run('NIC tune applied', () => invokeVmLibvirt(vmId, 'nic.tune', { mac_address: nicMac, model: 'virtio' }))}>Set virtio model</button>
          </div>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title="CPU / memory tuning" subtitle="Scheduler, memtune, live hotplug, vCPU pinning">
        <div className="grid gap-4 md:grid-cols-2 text-sm">
          <div className="space-y-2">
            <p className="text-xs text-[var(--text-muted)]">Scheduler (shares / period / quota)</p>
            <div className="flex gap-2">
              <input aria-label="CPU shares" className="input w-20" placeholder="shares" defaultValue={cputune?.shares ?? ''} id={`sched-shares-${vmId}`} />
              <input aria-label="CPU period" className="input w-20" placeholder="period" defaultValue={cputune?.period ?? ''} id={`sched-period-${vmId}`} />
              <input aria-label="CPU quota" className="input w-20" placeholder="quota" defaultValue={cputune?.quota ?? ''} id={`sched-quota-${vmId}`} />
            </div>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={disabled}
              onClick={() => {
                const shares = (document.getElementById(`sched-shares-${vmId}`) as HTMLInputElement).value
                const period = (document.getElementById(`sched-period-${vmId}`) as HTMLInputElement).value
                const quota = (document.getElementById(`sched-quota-${vmId}`) as HTMLInputElement).value
                void run('Scheduler updated', () => invokeVmLibvirt(vmId, 'scheduler.set', {
                  cpu_shares: shares ? Number(shares) : undefined,
                  vcpu_period: period ? Number(period) : undefined,
                  vcpu_quota: quota ? Number(quota) : undefined,
                }))
              }}
            >
              Apply scheduler
            </button>
          </div>
          <div className="space-y-2">
            <p className="text-xs text-[var(--text-muted)]">Memtune (KiB)</p>
            <div className="flex gap-2">
              <input aria-label="Hard limit (KiB)" className="input w-24" placeholder="hard" value={memtune.hard_limit_kb ?? ''} onChange={(e) => setMemtune({ ...memtune, hard_limit_kb: e.target.value ? Number(e.target.value) : undefined })} />
              <input aria-label="Soft limit (KiB)" className="input w-24" placeholder="soft" value={memtune.soft_limit_kb ?? ''} onChange={(e) => setMemtune({ ...memtune, soft_limit_kb: e.target.value ? Number(e.target.value) : undefined })} />
            </div>
            <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('Memtune updated', () => invokeVmLibvirt(vmId, 'memtune.set', memtune))}>Apply memtune</button>
          </div>
          <div className="space-y-2">
            <p className="text-xs text-[var(--text-muted)]">Live vCPU / memory (running guest)</p>
            <div className="flex gap-2">
              <input aria-label="Live vCPU count" className="input w-20" type="number" min={1} value={liveVcpus} onChange={(e) => setLiveVcpus(e.target.value)} />
              <input aria-label="Live memory (GiB)" className="input w-20" type="number" min={1} value={liveMemoryGiB} onChange={(e) => setLiveMemoryGiB(e.target.value)} />
            </div>
            <div className="flex gap-2">
              <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('Live vCPUs updated', () => invokeVmLibvirt(vmId, 'live.vcpus', { count: Number(liveVcpus) }))}>Live vCPUs</button>
              <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('Live memory updated', () => invokeVmLibvirt(vmId, 'live.memory', { memory_mb: Number(liveMemoryGiB) * 1024 }))}>Live memory</button>
            </div>
          </div>
          <div className="space-y-2">
            <p className="text-xs text-[var(--text-muted)]">Pin vCPU to host CPUs (comma list → bitmask)</p>
            <div className="flex gap-2">
              <input aria-label="vCPU index" className="input w-16" value={pinVcpu} onChange={(e) => setPinVcpu(e.target.value)} />
              <input aria-label="Host CPU list" className="input flex-1 font-mono" value={pinCpus} onChange={(e) => setPinCpus(e.target.value)} placeholder="0,1,2" />
            </div>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={disabled}
              onClick={() => {
                const indices = pinCpus.split(',').map((s) => Number(s.trim())).filter((n) => !Number.isNaN(n))
                const max = indices.length ? Math.max(...indices) + 1 : 1
                const cpus = Array.from({ length: max }, (_, i) => indices.includes(i))
                void run('vCPU pinned', () => invokeVmLibvirt(vmId, 'vcpu.pin', { vcpu: Number(pinVcpu), cpus }))
              }}
            >
              Pin vCPU
            </button>
          </div>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title="USB & PCI passthrough" data-testid="vm-hostdev-panel">
        <div className="grid gap-4 md:grid-cols-2 text-sm">
          <div>
            <p className="text-xs text-[var(--text-muted)] mb-2">USB devices on host</p>
            <ul className="text-xs text-[var(--text-muted)] space-y-1 max-h-32 overflow-auto mb-2">
              {usbDevices.map((d) => (
                <li key={`${d.vendor_id}-${d.product_id}`}>
                  <button type="button" className="hover:text-[var(--link)]" onClick={() => { setUsbVid(d.vendor_id); setUsbPid(d.product_id) }}>
                    {d.vendor_id}:{d.product_id} {d.description ?? ''}
                  </button>
                </li>
              ))}
            </ul>
            <div className="flex gap-2">
              <input aria-label="USB vendor ID" className="input w-20 font-mono" placeholder="vid" value={usbVid} onChange={(e) => setUsbVid(e.target.value)} />
              <input aria-label="USB product ID" className="input w-20 font-mono" placeholder="pid" value={usbPid} onChange={(e) => setUsbPid(e.target.value)} />
              <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('USB attached', () => invokeVmLibvirt(vmId, 'usb.attach', { vendor_id: usbVid, product_id: usbPid }))}>Attach</button>
              <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('USB detached', () => invokeVmLibvirt(vmId, 'usb.detach', { vendor_id: usbVid, product_id: usbPid }))}>Detach</button>
            </div>
          </div>
          <div>
            <p className="text-xs text-[var(--text-muted)] mb-2">PCI devices</p>
            <ul className="text-xs text-[var(--text-muted)] space-y-1 max-h-32 overflow-auto mb-2">
              {pciDevices.map((d) => (
                <li key={d.slot}>
                  <button type="button" className="hover:text-[var(--link)] font-mono" onClick={() => setPciBdf(d.slot)}>{d.slot} {d.vendor ?? ''} {d.device ?? ''}</button>
                </li>
              ))}
            </ul>
            <div className="flex gap-2">
              <input aria-label="PCI BDF address" className="input flex-1 font-mono" placeholder="0000:03:00.0" value={pciBdf} onChange={(e) => setPciBdf(e.target.value)} />
              <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('PCI attached', () => invokeVmLibvirt(vmId, 'pci.attach', { pci: pciBdf }))}>Attach</button>
              <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('PCI detached', () => invokeVmLibvirt(vmId, 'pci.detach', { pci: pciBdf }))}>Detach</button>
            </div>
          </div>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title="Firmware, TPM & boot order">
        <div className="flex flex-wrap gap-3 items-center text-sm mb-3">
          <label className="flex items-center gap-2 text-[var(--text-muted)]">
            <input type="checkbox" checked={uefi} onChange={(e) => setUefi(e.target.checked)} /> UEFI firmware
          </label>
          <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('Firmware updated', () => invokeVmLibvirt(vmId, 'firmware.set', { uefi }))}>Apply firmware</button>
          <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('TPM attached', () => invokeVmLibvirt(vmId, 'tpm.attach'))}>Attach TPM</button>
          <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => void run('TPM detached', () => invokeVmLibvirt(vmId, 'tpm.detach'))}>Detach TPM</button>
        </div>
        <label className="text-xs text-[var(--text-muted)] block">Boot order (comma-separated: hd, cdrom, network)
          <input className="input mt-1 block w-full font-mono" value={bootDevices} onChange={(e) => setBootDevices(e.target.value)} />
        </label>
        <button
          type="button"
          className="btn-secondary text-xs mt-2"
          disabled={disabled}
          onClick={() => void run('Boot order updated', () => invokeVmLibvirt(vmId, 'boot.set', { devices: bootDevices.split(',').map((s) => s.trim()).filter(Boolean) }))}
        >
          Save boot order
        </button>
      </MacGlassPanel>

      {hostId && (
        <MacGlassPanel title="ISO library" subtitle={`Host ${hostId.slice(0, 8)}…`}>
          <ul className="text-xs text-[var(--text-muted)] space-y-1 max-h-40 overflow-auto">
            {isos.length === 0 && <li>No ISO files found on host scan paths.</li>}
            {isos.map((f) => (
              <li key={f.name} className="font-mono">{f.name}{f.size_bytes ? ` · ${Math.round(f.size_bytes / 1024 / 1024)} MiB` : ''}</li>
            ))}
          </ul>
        </MacGlassPanel>
      )}

      <MacGlassPanel title="Domain XML" subtitle={`Edit persistent definition for ${vmName}`}>
        <textarea
          aria-label="Domain XML"
          className="input w-full font-mono text-xs min-h-[16rem]"
          value={domainXml}
          onChange={(e) => { xmlDirty.current = true; setDomainXml(e.target.value) }}
          spellCheck={false}
        />
        <div className="flex gap-2 mt-2">
          <button type="button" className="btn-secondary text-sm" disabled={disabled} onClick={() => { xmlDirty.current = false; void run('Domain XML updated', () => putVmDomainXml(vmId, domainXml)) }}>Save XML (define)</button>
          <button type="button" className="btn-secondary text-sm" onClick={() => { xmlDirty.current = false; void load() }}>Reload</button>
        </div>
        <CollapsibleCodeBlock title="XML preview" content={domainXml} className="mt-3" />
      </MacGlassPanel>
    </div>
  )
}

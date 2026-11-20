// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { FolderOpen, HardDrive, Plug, Settings, Usb } from 'lucide-react'
import type { VmLibvirtDetails } from '../../api/platform'
import { invokeVmLibvirt, queryHostLibvirt } from '../../api/platformVmLibvirt'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { MacGlassPanel, MacListRow } from './mac/PlatformMacUi'

interface VmDevicesPanelProps {
  vmId: string
  hostId?: string | null
  details: VmLibvirtDetails | null
  domainXml?: string
  loading?: boolean
  vmState?: string
  onChanged?: () => void
}

function countXmlTag(xml: string, tag: string): number {
  const re = new RegExp(`<${tag}[\\s/>]`, 'g')
  return (xml.match(re) ?? []).length
}

export default function VmDevicesPanel({ vmId, hostId, details, domainXml = '', loading, vmState, onChanged }: VmDevicesPanelProps) {
  const toast = useToastContext()
  const filesystems = details?.filesystems ?? []
  const watchdogCount = countXmlTag(domainXml, 'watchdog')
  const hostdevCount = countXmlTag(domainXml, 'hostdev')
  const tpmPresent = domainXml.includes('<tpm')
  const vsockPresent = domainXml.includes('<vsock')
  const shutOff = vmState === 'shutoff' || vmState === 'stopped' || vmState === 'shut off'

  const [fsSource, setFsSource] = useState('/var/share')
  const [fsTag, setFsTag] = useState('share')
  const [fsXattr, setFsXattr] = useState(false)
  const [wdModel, setWdModel] = useState('i6300esb')
  const [wdAction, setWdAction] = useState('reset')
  const [busy, setBusy] = useState(false)
  const [usbDevices, setUsbDevices] = useState<Array<{ vendor_id: string; product_id: string; description?: string }>>([])
  const [pciDevices, setPciDevices] = useState<Array<{ slot: string; vendor?: string; device?: string }>>([])

  useEffect(() => {
    if (!hostId) return
    let cancelled = false
    void (async () => {
      try {
        const [usb, pci] = await Promise.all([
          queryHostLibvirt<Array<{ vendor_id: string; product_id: string; description?: string }>>(hostId, 'host.usb'),
          queryHostLibvirt<Array<{ slot: string; vendor?: string; device?: string }>>(hostId, 'host.pci'),
        ])
        if (!cancelled) {
          setUsbDevices(usb ?? [])
          setPciDevices(pci ?? [])
        }
      } catch {
        if (!cancelled) {
          setUsbDevices([])
          setPciDevices([])
        }
      }
    })()
    return () => { cancelled = true }
  }, [hostId])

  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await fn()
      toast.success(label)
      onChanged?.()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const runVsock = async (action: 'vsock.attach' | 'vsock.detach') => {
    await run(action === 'vsock.attach' ? 'Vsock attached' : 'Vsock detached', () => invokeVmLibvirt(vmId, action))
  }

  return (
    <div className="space-y-4" data-testid="vm-devices-panel">
      <MacGlassPanel title="Devices & passthrough">
        <p className="text-xs text-[var(--text-muted)] mb-4">
          TPM, watchdog, vsock, virtiofs shares, and host devices — Cockpit Machines parity. Some changes require the guest to be shut off.
        </p>
        {loading && <p className="text-sm text-[var(--text-muted)]">Loading libvirt inventory…</p>}
        {!loading && (
          <div className="space-y-3">
            <MacListRow
              title="TPM"
              subtitle={tpmPresent ? 'Emulated TPM 2.0 attached' : 'Not configured'}
            />
            <div className="rounded-lg border border-white/[0.06] px-3 py-2 space-y-2" data-testid="vm-watchdog-row">
              <MacListRow
                title="Watchdog"
                subtitle={watchdogCount > 0 ? `${watchdogCount} device(s)` : 'Not configured'}
              />
              {watchdogCount === 0 && (
                <div className="flex flex-wrap gap-2 items-end pt-1">
                  <label className="text-xs text-[var(--text-muted)]">
                    Model
                    <select className="input mt-1 block text-sm" value={wdModel} onChange={(e) => setWdModel(e.target.value)}>
                      <option value="i6300esb">i6300esb</option>
                      <option value="ib700">ib700</option>
                      <option value="diag288">diag288</option>
                    </select>
                  </label>
                  <label className="text-xs text-[var(--text-muted)]">
                    Action
                    <select className="input mt-1 block text-sm" value={wdAction} onChange={(e) => setWdAction(e.target.value)}>
                      <option value="reset">reset</option>
                      <option value="shutdown">shutdown</option>
                      <option value="poweroff">poweroff</option>
                      <option value="pause">pause</option>
                      <option value="none">none</option>
                      <option value="dump">dump</option>
                    </select>
                  </label>
                  <button
                    type="button"
                    className="btn-secondary text-xs"
                    disabled={busy}
                    onClick={() => void run('Watchdog attached', () => invokeVmLibvirt(vmId, 'watchdog.attach', { model: wdModel, action: wdAction }))}
                  >
                    Attach watchdog
                  </button>
                </div>
              )}
            </div>
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-white/[0.06] px-3 py-2" data-testid="vm-vsock-row">
              <MacListRow
                title="Vsock"
                subtitle={vsockPresent ? 'Virtio vsock attached (guest↔host)' : 'Not configured'}
              />
              <div className="flex gap-2 shrink-0">
                {!vsockPresent && (
                  <button type="button" className="btn-secondary text-xs" onClick={() => void runVsock('vsock.attach')}>
                    Attach
                  </button>
                )}
                {vsockPresent && (
                  <button type="button" className="btn-secondary text-xs" onClick={() => void runVsock('vsock.detach')}>
                    Detach
                  </button>
                )}
              </div>
            </div>
            <MacListRow
              title="Host devices"
              subtitle={hostdevCount > 0 ? `${hostdevCount} PCI/USB passthrough` : 'None attached'}
            />
          </div>
        )}
      </MacGlassPanel>

      {hostId && (usbDevices.length > 0 || pciDevices.length > 0) && (
        <MacGlassPanel title="Attach host device" data-testid="vm-hostdev-attach-panel">
          <p className="text-xs text-[var(--text-muted)] mb-3">USB and PCI passthrough from the hypervisor — Cockpit hostdev parity.</p>
          {usbDevices.length > 0 && (
            <div className="space-y-2 mb-3">
              <p className="text-xs font-medium text-[var(--text-muted)]">USB</p>
              {usbDevices.slice(0, 8).map((d) => (
                <div key={`${d.vendor_id}:${d.product_id}`} className="flex flex-wrap items-center justify-between gap-2 text-sm">
                  <span className="text-[var(--text-secondary)] truncate">{d.description || `${d.vendor_id}:${d.product_id}`}</span>
                  <button
                    type="button"
                    className="btn-secondary text-xs shrink-0"
                    disabled={busy}
                    onClick={() => void run('USB attached', () => invokeVmLibvirt(vmId, 'usb.attach', { vendor_id: d.vendor_id, product_id: d.product_id }))}
                  >
                    Attach
                  </button>
                </div>
              ))}
            </div>
          )}
          {pciDevices.length > 0 && (
            <div className="space-y-2">
              <p className="text-xs font-medium text-[var(--text-muted)]">PCI</p>
              {pciDevices.slice(0, 8).map((d) => {
                const label = [d.vendor, d.device].filter(Boolean).join(' ')
                return (
                  <div key={d.slot} className="flex flex-wrap items-center justify-between gap-2 text-sm">
                    <span className="text-[var(--text-secondary)] font-mono text-xs truncate">{d.slot}{label ? ` · ${label}` : ''}</span>
                    <button
                      type="button"
                      className="btn-secondary text-xs shrink-0"
                      disabled={busy}
                      onClick={() => void run('PCI attached', () => invokeVmLibvirt(vmId, 'pci.attach', { pci: d.slot }))}
                    >
                      Attach
                    </button>
                  </div>
                )
              })}
            </div>
          )}
        </MacGlassPanel>
      )}

      <MacGlassPanel title="Shared directories (virtiofs)">
        <p className="text-xs text-[var(--text-muted)] mb-3">
          Host directory sharing via virtiofs. VM must be shut off to add or remove shares.
        </p>
        {filesystems.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No virtiofs mounts configured.</p>
        ) : (
          <div className="space-y-2">
            {filesystems.map((fs) => (
              <div key={`${fs.source}-${fs.mount_tag}`} className="flex flex-wrap items-center justify-between gap-2">
                <MacListRow
                  title={fs.mount_tag || 'share'}
                  subtitle={`${fs.source}${fs.accessmode ? ` · ${fs.accessmode}` : ''}`}
                />
                {shutOff && (
                  <button
                    type="button"
                    className="btn-secondary text-xs"
                    disabled={busy}
                    onClick={() => void run(`Removed ${fs.mount_tag}`, () => invokeVmLibvirt(vmId, 'virtiofs.remove', { mount_tag: fs.mount_tag }))}
                  >
                    Remove
                  </button>
                )}
              </div>
            ))}
            <p className="text-xs text-[var(--text-muted)] pt-2">
              In the guest: <code className="text-[var(--text-secondary)]">mount -t virtiofs &lt;tag&gt; /mnt</code>
            </p>
          </div>
        )}
        {shutOff && (
          <div className="mt-4 flex flex-wrap gap-2 items-end border-t border-white/[0.06] pt-3">
            <label className="text-xs text-[var(--text-muted)]">
              Host path
              <input className="input mt-1 block min-w-[16rem]" value={fsSource} onChange={(e) => setFsSource(e.target.value)} />
            </label>
            <label className="text-xs text-[var(--text-muted)]">
              Mount tag
              <input className="input mt-1 block w-28" value={fsTag} onChange={(e) => setFsTag(e.target.value)} />
            </label>
            <label className="text-xs text-[var(--text-muted)] flex items-center gap-2 mt-5">
              <input type="checkbox" checked={fsXattr} onChange={(e) => setFsXattr(e.target.checked)} />
              xattr
            </label>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={busy || !fsSource.trim() || !fsTag.trim()}
              onClick={() => void run('Virtiofs share added', () => invokeVmLibvirt(vmId, 'virtiofs.add', { source_dir: fsSource.trim(), mount_tag: fsTag.trim(), xattr: fsXattr }))}
            >
              Add share
            </button>
          </div>
        )}
        {!shutOff && filesystems.length === 0 && (
          <p className="text-xs text-amber-400/90 mt-2">Shut off the VM to add virtiofs shares.</p>
        )}
      </MacGlassPanel>

      {details && (
        <MacGlassPanel title="Block devices summary">
          <div className="space-y-2">
            {details.disks.map((d) => (
              <MacListRow
                key={d.target}
                title={d.target}
                subtitle={`${d.device} · ${d.driver}${d.cache ? ` · cache ${d.cache}` : ''}${d.capacity_bytes ? ` · ${(d.capacity_bytes / (1024 ** 3)).toFixed(1)} GiB cap` : ''}${d.physical_bytes ? ` · ${(d.physical_bytes / (1024 ** 3)).toFixed(1)} GiB on host` : ''}`}
              />
            ))}
          </div>
        </MacGlassPanel>
      )}
    </div>
  )
}

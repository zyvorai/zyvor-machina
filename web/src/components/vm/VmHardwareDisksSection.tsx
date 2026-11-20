// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { FolderOpen, Loader2 } from 'lucide-react'
import {
  attachVmDisk,
  detachVmDisk,
  resizeVmDisk,
  type VmDiskRow,
  type VmLibvirtDetails,
  type VmPendingConfig,
} from '../../api/platform'
import { invokeVmLibvirt } from '../../api/platformVmLibvirt'
import { formatUserError } from '../../utils/apiError'
import { listIsos, type ImageFile } from '../../api/extras'
import { BrowseHostPathModal, isHostDiskImageFileName, isIsoFileName } from '../BrowseHostPathModal'
import ConfirmDialog from '../ConfirmDialog'
import VmPendingBadge from '../platform/VmPendingBadge'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { formatBytes } from '../../utils/vm'

type Props = {
  vmId: string
  managed?: boolean
  details: VmLibvirtDetails | null
  pending: VmPendingConfig | null
  loading?: boolean
  platformDisks?: VmDiskRow[]
  canBrowseHost?: boolean
  compact?: boolean
  onChanged?: () => void
  onNotify?: (msg: string) => void
  onError?: (msg: string) => void
}

export default function VmHardwareDisksSection({
  vmId,
  managed,
  details,
  pending,
  loading,
  platformDisks = [],
  canBrowseHost = false,
  compact = false,
  onChanged,
  onNotify,
  onError,
}: Props) {
  const disabled = managed === false
  const [attachPath, setAttachPath] = useState('/var/lib/libvirt/images/data.qcow2')
  const [attachDev, setAttachDev] = useState('vdb')
  const [isoPath, setIsoPath] = useState('/var/lib/libvirt/images/debian-12.iso')
  const [isoTarget, setIsoTarget] = useState('')
  const [isoFiles, setIsoFiles] = useState<ImageFile[]>([])
  const [isoBrowseOpen, setIsoBrowseOpen] = useState(false)
  const [attachDiskBrowseOpen, setAttachDiskBrowseOpen] = useState(false)
  const [resizeTarget, setResizeTarget] = useState('')
  const [resizeGb, setResizeGb] = useState('10')
  const [diskEditTarget, setDiskEditTarget] = useState<string | null>(null)
  const [diskEditCache, setDiskEditCache] = useState('none')
  const [diskEditBus, setDiskEditBus] = useState('virtio')
  const [diskEditReadonly, setDiskEditReadonly] = useState(false)
  const [busy, setBusy] = useState(false)
  const [detachTarget, setDetachTarget] = useState<string | null>(null)

  useEffect(() => {
    void listIsos()
      .then((r) => setIsoFiles(r.files ?? []))
      .catch(() => setIsoFiles([]))
  }, [])

  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await fn()
      onNotify?.(label)
      onChanged?.()
    } catch (e: unknown) {
      onError?.(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const panelTitle = compact ? undefined : 'Libvirt disks'
  const subtitle = compact ? undefined : 'Live hypervisor inventory'

  return (
    <div className="space-y-3" data-testid="vm-hardware-disks-section">
      <MacGlassPanel title={panelTitle ?? 'Disks'} subtitle={subtitle}>
        {loading ? (
          <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading…</p>
        ) : details?.disks.length ? (
          <ul className="text-sm text-[var(--text-muted)] space-y-3">
            {details.disks.map((d) => (
              <li key={d.target} className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.04] pb-2">
                <span className="flex items-center gap-2 flex-wrap">
                  <span className="font-mono text-[var(--text-primary)]">{d.target}</span>
                  <VmPendingBadge pending={pending} category="disk" />
                  {' · '}{d.device}
                  {d.bus ? ` · ${d.bus}` : ''}
                  {d.cache ? ` · cache ${d.cache}` : ''}
                  {!compact && d.source ? ` · ${d.source}` : ''}
                  {d.capacity_bytes ? ` · ${formatBytes(d.capacity_bytes)} cap` : ''}
                </span>
                <div className="flex gap-2">
                  {d.device === 'disk' && (
                    <>
                      <button
                        type="button"
                        className="btn-secondary text-xs"
                        disabled={disabled || busy}
                        onClick={() => {
                          setDiskEditTarget(d.target)
                          setDiskEditCache(d.cache || 'none')
                          setDiskEditBus(d.bus || 'virtio')
                          setDiskEditReadonly(Boolean(d.readonly))
                        }}
                      >
                        Edit
                      </button>
                      <button
                        type="button"
                        className="btn-secondary text-xs"
                        disabled={disabled || busy}
                        onClick={() => setDetachTarget(d.target)}
                      >
                        Detach
                      </button>
                    </>
                  )}
                  {d.device === 'cdrom' && d.source && (
                    <button
                      type="button"
                      className="btn-secondary text-xs"
                      disabled={disabled || busy}
                      onClick={() => void run('CD-ROM ejected', () => invokeVmLibvirt(vmId, 'cdrom.eject', { target: d.target }))}
                    >
                      Eject
                    </button>
                  )}
                  {d.device === 'cdrom' && (
                    // Eject only blanks the media; this removes the drive, which
                    // previously needed `virsh detach-disk` on the hypervisor.
                    <button
                      type="button"
                      className="btn-secondary text-xs"
                      disabled={disabled || busy}
                      title="Remove the CD-ROM drive itself"
                      onClick={() => void run('CD-ROM drive removed', () => invokeVmLibvirt(vmId, 'cdrom.detach', { target: d.target }))}
                    >
                      Remove drive
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-[var(--text-muted)]">No disks reported from libvirt.</p>
        )}
      </MacGlassPanel>

      {diskEditTarget && (
        <MacGlassPanel title={`Edit disk ${diskEditTarget}`}>
          <div className="flex flex-wrap gap-3 items-end">
            <label className="text-xs text-[var(--text-muted)]">
              Bus
              <select className="input mt-1 block" value={diskEditBus} onChange={(e) => setDiskEditBus(e.target.value)}>
                <option value="virtio">virtio</option>
                <option value="sata">sata</option>
                <option value="scsi">scsi</option>
                <option value="ide">ide</option>
              </select>
            </label>
            <label className="text-xs text-[var(--text-muted)]">
              Cache
              <select className="input mt-1 block" value={diskEditCache} onChange={(e) => setDiskEditCache(e.target.value)}>
                <option value="none">none</option>
                <option value="writethrough">writethrough</option>
                <option value="writeback">writeback</option>
                <option value="directsync">directsync</option>
                <option value="unsafe">unsafe</option>
              </select>
            </label>
            <label className="text-xs text-[var(--text-muted)] flex items-center gap-2 mt-5">
              <input type="checkbox" checked={diskEditReadonly} onChange={(e) => setDiskEditReadonly(e.target.checked)} />
              Read-only
            </label>
            <button
              type="button"
              className="btn-secondary text-sm"
              disabled={busy}
              onClick={() => void run('Disk updated', async () => {
                await invokeVmLibvirt(vmId, 'disk.tune', {
                  target: diskEditTarget,
                  bus: diskEditBus,
                  cache: diskEditCache,
                  readonly: diskEditReadonly,
                })
                setDiskEditTarget(null)
              })}
            >
              Apply
            </button>
            <button type="button" className="btn-secondary text-sm" onClick={() => setDiskEditTarget(null)}>Cancel</button>
          </div>
        </MacGlassPanel>
      )}

      {!compact && platformDisks.length > 0 && (
        <MacGlassPanel title="Platform disk records">
          <ul className="text-sm text-[var(--text-muted)] space-y-2">
            {platformDisks.map((d) => (
              <li key={d.id}>{d.name} · {d.size_gib} GiB · {d.storage_class}{d.path ? ` · ${d.path}` : ''}</li>
            ))}
          </ul>
        </MacGlassPanel>
      )}

      <MacGlassPanel title="Attach disk">
        <div className="flex flex-wrap gap-3 items-end">
          <label className="text-xs text-[var(--text-muted)]">
            Path
            <div className="mt-1 flex gap-2 min-w-[14rem]">
              <input className="input flex-1 min-w-0 font-mono text-xs" value={attachPath} onChange={(e) => setAttachPath(e.target.value)} />
              {canBrowseHost ? (
                <button type="button" className="btn-secondary shrink-0 text-xs" onClick={() => setAttachDiskBrowseOpen(true)}>
                  <FolderOpen className="w-3.5 h-3.5" />
                </button>
              ) : null}
            </div>
          </label>
          <label className="text-xs text-[var(--text-muted)]">
            Target
            <input className="input mt-1 block w-20" value={attachDev} onChange={(e) => setAttachDev(e.target.value)} />
          </label>
          <button type="button" className="btn-secondary text-xs" disabled={disabled || busy} onClick={() => void run('Attach disk queued', () => attachVmDisk(vmId, { disk_path: attachPath, target_dev: attachDev }))}>
            Attach
          </button>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title="Insert ISO">
        <div className="flex flex-wrap gap-3 items-end">
          <label className="text-xs text-[var(--text-muted)]">
            ISO path
            <div className="mt-1 flex gap-2 min-w-[14rem]">
              <input list="vm-hw-iso-list" className="input flex-1 font-mono text-xs" value={isoPath} onChange={(e) => setIsoPath(e.target.value)} />
              <datalist id="vm-hw-iso-list">
                {isoFiles.map((f) => {
                  const p = f.path ?? f.name
                  return <option key={p} value={p}>{f.name}</option>
                })}
              </datalist>
              {canBrowseHost ? (
                <button type="button" className="btn-secondary shrink-0 text-xs" onClick={() => setIsoBrowseOpen(true)}>
                  <FolderOpen className="w-3.5 h-3.5" />
                </button>
              ) : null}
            </div>
          </label>
          <label className="text-xs text-[var(--text-muted)]">
            CD-ROM
            <select className="input mt-1 block w-24" value={isoTarget} onChange={(e) => setIsoTarget(e.target.value)}>
              {/* Empty = the daemon picks a free target. Hard-coding sda collided
                  with the root disk on every SATA guest. */}
              <option value="">auto</option>
              {(details?.disks ?? []).filter((d) => d.device === 'cdrom').map((d) => (
                <option key={d.target} value={d.target}>{d.target}</option>
              ))}
            </select>
          </label>
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={disabled || busy || !isoPath.trim()}
            onClick={() => void run('ISO inserted', () => invokeVmLibvirt(vmId, 'cdrom.insert', { iso_path: isoPath.trim(), target: isoTarget }))}
          >
            Insert
          </button>
        </div>
      </MacGlassPanel>

      {!compact && (
        <MacGlassPanel title="Resize block device">
          <div className="flex flex-wrap gap-3 items-end">
            <label className="text-xs text-[var(--text-muted)]">
              Target
              <select className="input mt-1 block" value={resizeTarget} onChange={(e) => setResizeTarget(e.target.value)}>
                <option value="">Select…</option>
                {(details?.disks ?? []).filter((d) => d.device === 'disk').map((d) => (
                  <option key={d.target} value={d.target}>{d.target}</option>
                ))}
              </select>
            </label>
            <label className="text-xs text-[var(--text-muted)]">
              GiB
              <input type="number" min={1} className="input mt-1 block w-20" value={resizeGb} onChange={(e) => setResizeGb(e.target.value)} />
            </label>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={disabled || busy || !resizeTarget || !resizeGb}
              onClick={() => void run('Resize disk queued', () => resizeVmDisk(vmId, resizeTarget, Number(resizeGb)))}
            >
              Resize
            </button>
          </div>
        </MacGlassPanel>
      )}

      <BrowseHostPathModal open={isoBrowseOpen} onClose={() => setIsoBrowseOpen(false)} title="Browse for ISO" canSelectFile={isIsoFileName} onSelectPath={(p) => setIsoPath(p)} />
      <BrowseHostPathModal open={attachDiskBrowseOpen} onClose={() => setAttachDiskBrowseOpen(false)} title="Browse for disk" canSelectFile={isHostDiskImageFileName} onSelectPath={(p) => setAttachPath(p)} />

      <ConfirmDialog
        open={detachTarget !== null}
        title="Detach disk"
        message={`This will detach disk '${detachTarget}' from the VM. The disk image will not be deleted.`}
        confirmLabel="Detach"
        variant="warning"
        onCancel={() => setDetachTarget(null)}
        onConfirm={() => {
          const target = detachTarget
          setDetachTarget(null)
          if (target) void run(`Detach ${target} queued`, () => detachVmDisk(vmId, target))
        }}
      />
    </div>
  )
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Loader2, Plug, Unplug, X } from 'lucide-react'
import { invokeVmLibvirt, queryHostLibvirt } from '../../api/platformVmLibvirt'
import HardwareApplyBadge from './HardwareApplyBadge'
import { resolveHardwareBadges } from '../../utils/hardwareApplyBadges'
import {
  filterNodeDevices,
  lookupPciIommu,
  nodeDeviceAttachAction,
  nodeDeviceLabel,
  parseAttachedHostDevices,
  pciBdfFromNodeDevice,
  type AttachedHostDevice,
  type HostPciInfo,
  type LibvirtNodeDevice,
} from '../../utils/nodeDeviceAttach'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { useFocusTrap } from '../../hooks/useFocusTrap'

type Props = {
  open: boolean
  onClose: () => void
  vmId: string
  vmName: string
  hostId?: string | null
  domainXml?: string
  managed?: boolean
  readOnly?: boolean
  onAttached?: () => void
}

const FILTERS = [
  { id: 'all' as const, label: 'All' },
  { id: 'pci' as const, label: 'PCI / VFIO' },
  { id: 'usb' as const, label: 'USB' },
  { id: 'other' as const, label: 'Other' },
]

export default function VmHostDeviceAttachDrawer({
  open,
  onClose,
  vmId,
  vmName,
  hostId,
  domainXml = '',
  managed,
  readOnly = false,
  onAttached,
}: Props) {
  const toast = useToastContext()
  const ignoreBackdropClose = useRef(false)
  const panelRef = useRef<HTMLElement>(null)
  const [loading, setLoading] = useState(false)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [devices, setDevices] = useState<LibvirtNodeDevice[]>([])
  const [pciInventory, setPciInventory] = useState<HostPciInfo[]>([])
  const [filter, setFilter] = useState<(typeof FILTERS)[number]['id']>('all')
  const disabled = readOnly || managed === false

  const attached = useMemo(() => parseAttachedHostDevices(domainXml), [domainXml])

  const refresh = useCallback(async () => {
    if (!hostId) {
      setDevices([])
      setPciInventory([])
      return
    }
    setLoading(true)
    try {
      const [list, pci] = await Promise.all([
        queryHostLibvirt<LibvirtNodeDevice[]>(hostId, 'host.node_devices').catch(() => []),
        queryHostLibvirt<HostPciInfo[]>(hostId, 'host.pci').catch(() => []),
      ])
      setDevices(Array.isArray(list) ? list : [])
      setPciInventory(Array.isArray(pci) ? pci : [])
    } catch {
      setDevices([])
      setPciInventory([])
    } finally {
      setLoading(false)
    }
  }, [hostId])

  useEffect(() => {
    if (open) {
      ignoreBackdropClose.current = true
      const timer = window.setTimeout(() => {
        ignoreBackdropClose.current = false
      }, 0)
      void refresh()
      return () => window.clearTimeout(timer)
    }
  }, [open, refresh])

  useFocusTrap(panelRef, open, onClose)

  if (!open) return null

  const filtered = filterNodeDevices(devices, filter)

  const attach = async (dev: LibvirtNodeDevice) => {
    const action = nodeDeviceAttachAction(dev)
    if (action.kind === 'unsupported') {
      toast.error(action.reason)
      return
    }
    setBusyId(dev.name)
    try {
      if (action.kind === 'pci') {
        await invokeVmLibvirt(vmId, 'pci.attach', { pci: action.pci })
      } else {
        await invokeVmLibvirt(vmId, 'usb.attach', {
          vendor_id: action.vendor_id,
          product_id: action.product_id,
        })
      }
      toast.success(`Attached ${action.label}`)
      onAttached?.()
      await refresh()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusyId(null)
    }
  }

  const detach = async (dev: AttachedHostDevice) => {
    setBusyId(dev.id)
    try {
      if (dev.kind === 'pci') {
        await invokeVmLibvirt(vmId, 'pci.detach', { pci: dev.pci })
      } else {
        await invokeVmLibvirt(vmId, 'usb.detach', {
          vendor_id: dev.vendor_id,
          product_id: dev.product_id,
        })
      }
      toast.success(`Detached ${dev.label}`)
      onAttached?.()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <>
      <button
        type="button"
        className="fixed inset-0 z-[92] bg-black/50 backdrop-blur-sm"
        aria-label="Close Attach Device"
        onClick={() => {
          if (ignoreBackdropClose.current) return
          onClose()
        }}
      />
      <aside
        ref={panelRef}
        className="fixed top-0 right-0 z-[95] h-full w-full max-w-xl bg-[var(--apple-surface)]/98 border-l border-white/10 shadow-2xl flex flex-col overflow-hidden"
        role="dialog"
        aria-modal="true"
        aria-label="Attach host device"
        data-testid="vm-hostdev-attach-drawer"
      >
        <header className="flex items-center justify-between px-4 py-3 border-b border-white/10 shrink-0">
          <div>
            <h2 className="font-semibold text-[var(--text-primary)] flex items-center gap-2">
              <Plug className="w-4 h-4 text-[var(--link)]" /> Host devices
            </h2>
            <p className="text-xs text-[var(--text-muted)]">{vmName} · attach, detach, and IOMMU hints</p>
          </div>
          <button type="button" onClick={onClose} aria-label="Close" className="p-1.5 rounded hover:bg-white/10 text-[var(--text-muted)]"><X className="w-5 h-5" aria-hidden="true" /></button>
        </header>

        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          <section data-testid="vm-hostdev-attached-section">
            <p className="text-xs font-medium text-[var(--text-secondary)] mb-2 flex items-center gap-1.5">
              <Unplug className="w-3.5 h-3.5 text-amber-600" /> Attached to this VM
            </p>
            {attached.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No host devices attached.</p>
            ) : (
              <div className="space-y-2">
                {attached.map((dev) => (
                  <div
                    key={dev.id}
                    className="rounded-lg border border-amber-500/20 bg-amber-950/10 px-3 py-2 flex items-start justify-between gap-3"
                    data-testid={`vm-hostdev-attached-${dev.id}`}
                  >
                    <div className="min-w-0">
                      <p className="text-xs text-[var(--text-primary)] truncate">{dev.label}</p>
                      <p className="text-[11px] text-[var(--text-muted)]">{dev.kind === 'pci' ? 'PCI passthrough' : 'USB passthrough'}</p>
                    </div>
                    <button
                      type="button"
                      className="btn-secondary text-xs shrink-0"
                      disabled={disabled || busyId === dev.id}
                      onClick={() => void detach(dev)}
                    >
                      {busyId === dev.id ? 'Detaching…' : 'Detach'}
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section>
            <p className="text-xs font-medium text-[var(--text-secondary)] mb-2">Available on host</p>
            <div className="flex flex-wrap gap-2 mb-3">
              {FILTERS.map((f) => (
                <button
                  key={f.id}
                  type="button"
                  className={`text-xs px-2 py-1 rounded border ${filter === f.id ? 'border-[var(--accent)]/50 text-[var(--link)] bg-[var(--accent-soft)]' : 'border-white/10 text-[var(--text-muted)]'}`}
                  onClick={() => setFilter(f.id)}
                >
                  {f.label}
                </button>
              ))}
            </div>

            {!hostId ? (
              <p className="text-sm text-[var(--text-muted)]">No hypervisor assigned for this VM.</p>
            ) : loading ? (
              <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Discovering node devices…</p>
            ) : filtered.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No attachable node devices found on this host.</p>
            ) : (
              <div className="space-y-2">
                {filtered.slice(0, 40).map((dev) => {
                  const action = nodeDeviceAttachAction(dev)
                  const attachable = action.kind !== 'unsupported'
                  const pci = action.kind === 'pci' ? action.pci : pciBdfFromNodeDevice(dev)
                  const iommu = pci ? lookupPciIommu(pci, pciInventory) : null
                  return (
                    <div
                      key={dev.name}
                      className="rounded-lg border border-white/[0.08] bg-[var(--apple-surface)] px-3 py-2 flex items-start justify-between gap-3"
                      data-testid={`vm-hostdev-row-${dev.name}`}
                    >
                      <div className="min-w-0">
                        <p className="text-xs text-[var(--text-primary)] truncate">{nodeDeviceLabel(dev)}</p>
                        <p className="text-[11px] text-[var(--text-muted)]">
                          {dev.capability_type}{dev.driver ? ` · ${dev.driver}` : ''}{iommu ? ` · IOMMU ${iommu}` : ''}
                        </p>
                        <div className="flex flex-wrap gap-1 mt-1">
                          {resolveHardwareBadges(['restart_required', 'migration_unsafe', 'advanced']).map((b) => (
                            <HardwareApplyBadge key={b.id} badge={b} compact />
                          ))}
                        </div>
                      </div>
                      <button
                        type="button"
                        className="btn-secondary text-xs shrink-0"
                        disabled={disabled || !attachable || busyId === dev.name}
                        onClick={() => void attach(dev)}
                      >
                        {busyId === dev.name ? 'Attaching…' : attachable ? 'Attach' : 'View only'}
                      </button>
                    </div>
                  )
                })}
              </div>
            )}
          </section>
        </div>
      </aside>
    </>
  )
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, Loader2, X } from 'lucide-react'
import { useFocusTrap } from '../../hooks/useFocusTrap'
import type { UseVmHardwareResult } from '../../hooks/useVmHardware'
import type { VmHardwareSummaryReport } from '../../api/platform'
import HardwareApplyBadge from './HardwareApplyBadge'
import { resolveHardwareBadges } from '../../utils/hardwareApplyBadges'
import VmCpuTopologyModal from '../platform/VmCpuTopologyModal'
import VmMemorySizingModal from '../platform/VmMemorySizingModal'
import VmDevicesPanel from '../platform/VmDevicesPanel'
import VmGraphicsPanel from '../platform/VmGraphicsPanel'
import PlatformVmAdvanced from '../platform/PlatformVmAdvanced'
import VmHardwareDisksSection from './VmHardwareDisksSection'
import VmHardwareNetworkSection from './VmHardwareNetworkSection'
import { invokeVmLibvirt, putVmDomainXml } from '../../api/platformVmLibvirt'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'

type SectionId =
  | 'cpu'
  | 'memory'
  | 'firmware'
  | 'display'
  | 'storage'
  | 'network'
  | 'hostdev'
  | 'performance'
  | 'xml'

const SECTIONS: { id: SectionId; label: string }[] = [
  { id: 'cpu', label: 'CPU' },
  { id: 'memory', label: 'Memory' },
  { id: 'firmware', label: 'Firmware & Security' },
  { id: 'display', label: 'Display & Access' },
  { id: 'storage', label: 'Storage' },
  { id: 'network', label: 'Network' },
  { id: 'hostdev', label: 'Host Devices' },
  { id: 'performance', label: 'Performance' },
  { id: 'xml', label: 'Advanced XML' },
]

type Props = {
  open: boolean
  onClose: () => void
  vmId: string
  vmName: string
  hostId?: string | null
  managed?: boolean
  vmState?: string
  hardware: UseVmHardwareResult
  canBrowseHost?: boolean
  initialSection?: SectionId
}

export default function VmEditHardwareDrawer({
  open,
  onClose,
  vmId,
  vmName,
  hostId,
  managed,
  vmState,
  hardware,
  canBrowseHost,
  initialSection = 'cpu',
}: Props) {
  const toast = useToastContext()
  const [expanded, setExpanded] = useState<SectionId | null>(initialSection)
  const [cpuOpen, setCpuOpen] = useState(false)
  const [memoryOpen, setMemoryOpen] = useState(false)
  const [xmlDraft, setXmlDraft] = useState('')
  const [xmlSaving, setXmlSaving] = useState(false)
  const [uefiBusy, setUefiBusy] = useState(false)
  const panelRef = useRef<HTMLElement>(null)
  useFocusTrap(panelRef, open, onClose)

  // `expanded` was only a useState initializer, so opening via "Advanced XML"
  // (initialSection='xml') still showed the CPU pane because the drawer mounted
  // earlier with the default 'cpu'. Sync it whenever the drawer opens or the
  // requested section changes.
  useEffect(() => {
    if (open) setExpanded(initialSection)
  }, [open, initialSection])

  // Drop any unsaved XML draft when (re)opening or switching VM, so VM A's edit
  // doesn't bleed over VM B's domain (the component stays mounted across nav).
  useEffect(() => {
    if (open) setXmlDraft('')
  }, [open, vmId])

  if (!open) return null

  const running = vmState === 'running'
  const disabled = managed === false
  const { details, domainXml, pending, loading, refresh, report } = hardware

  const sectionBadges = (id: SectionId): string[] => {
    const map: Partial<Record<SectionId, keyof VmHardwareSummaryReport>> = {
      cpu: 'cpu',
      memory: 'memory',
      firmware: 'firmware',
      display: 'display',
      hostdev: 'host_devices',
      performance: 'memory',
    }
    const key = map[id]
    if (!key || !report) return []
    const sec = report[key]
    if (sec && typeof sec === 'object' && 'badges' in sec) {
      return (sec as { badges?: string[] }).badges ?? []
    }
    return []
  }

  const toggle = (id: SectionId) => setExpanded((cur) => (cur === id ? null : id))

  const saveXml = async () => {
    setXmlSaving(true)
    try {
      await putVmDomainXml(vmId, xmlDraft)
      toast.success('Domain XML updated')
      await refresh(true)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setXmlSaving(false)
    }
  }

  const setFirmware = async (uefi: boolean) => {
    setUefiBusy(true)
    try {
      await invokeVmLibvirt(vmId, 'firmware.set', { uefi })
      toast.success(uefi ? 'UEFI firmware set' : 'BIOS firmware set')
      await refresh(true)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setUefiBusy(false)
    }
  }

  return (
    <>
      <button type="button" className="fixed inset-0 z-[85] bg-black/50 backdrop-blur-sm" aria-label="Close Edit Hardware" onClick={onClose} />
      <aside
        ref={panelRef}
        className="fixed top-0 right-0 z-[90] h-full w-full max-w-xl bg-[var(--apple-surface)]/98 border-l border-white/10 shadow-2xl flex flex-col overflow-hidden"
        role="dialog"
        aria-modal="true"
        aria-label="Edit hardware"
        data-testid="vm-edit-hardware-drawer"
      >
        <header className="flex items-center justify-between px-4 py-3 border-b border-white/10 shrink-0">
          <div>
            <h2 className="font-semibold text-[var(--text-primary)]">Edit Hardware</h2>
            <p className="text-xs text-[var(--text-muted)]">{vmName}</p>
          </div>
          <button type="button" onClick={onClose} aria-label="Close" className="p-1.5 rounded hover:bg-white/10 text-[var(--text-muted)]"><X className="w-5 h-5" aria-hidden="true" /></button>
        </header>

        <div className="flex-1 overflow-y-auto p-4 space-y-2">
          {loading && !details ? (
            <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading…</p>
          ) : null}

          {SECTIONS.map(({ id, label }) => {
            const isOpen = expanded === id
            return (
              <div key={id} className="rounded-lg border border-white/[0.08] overflow-hidden" data-testid={`vm-edit-hardware-section-${id}`}>
                <button
                  type="button"
                  className="w-full flex items-center gap-2 px-3 py-2.5 text-sm text-[var(--text-primary)] hover:bg-white/[0.04]"
                  onClick={() => toggle(id)}
                >
                  {isOpen ? <ChevronDown className="w-4 h-4 text-[var(--text-muted)]" /> : <ChevronRight className="w-4 h-4 text-[var(--text-muted)]" />}
                  <span className="flex-1 text-left">{label}</span>
                  <span className="flex flex-wrap gap-1 justify-end">
                    {resolveHardwareBadges(sectionBadges(id)).map((b) => (
                      <HardwareApplyBadge key={b.id} badge={b} compact />
                    ))}
                  </span>
                </button>
                {isOpen ? (
                  <div className="px-3 pb-3 border-t border-white/[0.06] pt-3 space-y-3">
                    {id === 'cpu' && (
                      <>
                        <p className="text-xs text-[var(--text-muted)]">vCPU count, sockets, cores, threads, CPU mode.</p>
                        <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => setCpuOpen(true)}>
                          Edit CPU topology
                        </button>
                      </>
                    )}
                    {id === 'memory' && (
                      <>
                        <p className="text-xs text-[var(--text-muted)]">Current RAM, max RAM, balloon (live when running).</p>
                        <button type="button" className="btn-secondary text-xs" disabled={disabled} onClick={() => setMemoryOpen(true)}>
                          Edit memory
                        </button>
                      </>
                    )}
                    {id === 'firmware' && (
                      <>
                        <p className="text-xs text-[var(--text-muted)]">BIOS/UEFI, Secure Boot, TPM, RNG — restart required for most changes.</p>
                        <div className="flex flex-wrap gap-2">
                          <button type="button" className="btn-secondary text-xs" disabled={disabled || uefiBusy} onClick={() => void setFirmware(true)}>Set UEFI</button>
                          <button type="button" className="btn-secondary text-xs" disabled={disabled || uefiBusy} onClick={() => void setFirmware(false)}>Set BIOS</button>
                        </div>
                        <VmDevicesPanel
                          vmId={vmId}
                          hostId={hostId}
                          details={details}
                          domainXml={domainXml}
                          loading={loading}
                          vmState={vmState}
                          onChanged={() => void refresh(true)}
                        />
                      </>
                    )}
                    {id === 'display' && (
                      <VmGraphicsPanel vmId={vmId} domainXml={domainXml} disabled={disabled} onChanged={() => void refresh(true)} />
                    )}
                    {id === 'storage' && (
                      <VmHardwareDisksSection
                        vmId={vmId}
                        managed={managed}
                        details={details}
                        pending={pending}
                        loading={loading}
                        canBrowseHost={canBrowseHost}
                        compact
                        onChanged={() => void refresh(true)}
                        onNotify={(m) => toast.success(m)}
                        onError={(m) => toast.error(m)}
                      />
                    )}
                    {id === 'network' && (
                      <VmHardwareNetworkSection
                        vmId={vmId}
                        managed={managed}
                        details={details}
                        pending={pending}
                        loading={loading}
                        compact
                        onChanged={() => void refresh(true)}
                        onNotify={(m) => toast.success(m)}
                        onError={(m) => toast.error(m)}
                      />
                    )}
                    {id === 'hostdev' && (
                      <VmDevicesPanel
                        vmId={vmId}
                        hostId={hostId}
                        details={details}
                        domainXml={domainXml}
                        loading={loading}
                        vmState={vmState}
                        onChanged={() => void refresh(true)}
                      />
                    )}
                    {id === 'performance' && (
                      <PlatformVmAdvanced
                        vmId={vmId}
                        hostId={hostId}
                        vmName={vmName}
                        managed={managed}
                        libvirtDetails={details}
                        onChanged={() => void refresh(true)}
                      />
                    )}
                    {id === 'xml' && (
                      <>
                        <textarea
                          aria-label="Domain XML"
                          className="input w-full font-mono text-xs min-h-[12rem]"
                          value={xmlDraft || domainXml}
                          onChange={(e) => setXmlDraft(e.target.value)}
                          spellCheck={false}
                        />
                        <button type="button" className="btn-secondary text-xs" disabled={disabled || xmlSaving} onClick={() => void saveXml()}>
                          {xmlSaving ? 'Saving…' : 'Save XML (define)'}
                        </button>
                      </>
                    )}
                  </div>
                ) : null}
              </div>
            )
          })}
        </div>
      </aside>

      <VmCpuTopologyModal
        open={cpuOpen}
        vmId={vmId}
        vmName={vmName}
        onClose={() => setCpuOpen(false)}
        onSaved={() => void refresh(true)}
        onNotify={(m) => toast.success(m)}
        onError={(m) => toast.error(m)}
      />
      <VmMemorySizingModal
        open={memoryOpen}
        vmId={vmId}
        vmName={vmName}
        running={running}
        onClose={() => setMemoryOpen(false)}
        onSaved={() => void refresh(true)}
        onNotify={(m) => toast.success(m)}
        onError={(m) => toast.error(m)}
      />
    </>
  )
}

export type { SectionId }

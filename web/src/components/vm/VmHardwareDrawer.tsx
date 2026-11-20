// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useRef, useState } from 'react'
import { Loader2, Monitor, X } from 'lucide-react'
import { useFocusTrap } from '../../hooks/useFocusTrap'
import { Link } from 'react-router'
import type { UseVmHardwareResult } from '../../hooks/useVmHardware'
import type { VmHardwareSection as VmHardwareSectionDto, VmHardwareSummaryReport } from '../../api/platform'
import type { VmPortForwardRule } from '../../api/platform'
import { exposeGuestPortOnVm } from '../../utils/vmPortForwardServices'
import VmHardwareSection from './VmHardwareSection'
import VmEditHardwareDrawer, { type SectionId } from './VmEditHardwareDrawer'
import VmWindowsReadinessPanel from './VmWindowsReadinessPanel'
import VmHardwareCompatPanel from './VmHardwareCompatPanel'
import VmHostDeviceAttachDrawer from './VmHostDeviceAttachDrawer'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { cinemaHubPath } from '../../utils/consoleExperienceMode'
import type { VmHardwareSummary } from '../../utils/vmHardwareSummary'

type Props = {
  open: boolean
  onClose: () => void
  vmId: string
  vmName: string
  hostId?: string | null
  managed?: boolean
  vmState?: string
  hardware: UseVmHardwareResult
  portForwardRules?: VmPortForwardRule[]
  protocols?: string[]
  canBrowseHost?: boolean
  readOnly?: boolean
  onPlanRefresh?: () => void
}

function sectionFromReport(
  report: VmHardwareSummaryReport | null,
  key: keyof VmHardwareSummaryReport,
  fallback: string,
): { value: string; badges: string[] } {
  const raw = report?.[key]
  if (raw && typeof raw === 'object' && 'value' in raw) {
    const sec = raw as VmHardwareSectionDto
    return { value: sec.value, badges: sec.badges ?? [] }
  }
  return { value: fallback, badges: [] }
}

export default function VmHardwareDrawer({
  open,
  onClose,
  vmId,
  vmName,
  hostId,
  managed,
  vmState,
  hardware,
  portForwardRules = [],
  protocols = [],
  canBrowseHost,
  readOnly = false,
  onPlanRefresh,
}: Props) {
  const toast = useToastContext()
  const [editOpen, setEditOpen] = useState(false)
  const [editSection, setEditSection] = useState<SectionId>('cpu')
  const [exposeBusy, setExposeBusy] = useState(false)
  const [compatOpen, setCompatOpen] = useState(false)
  const [attachOpen, setAttachOpen] = useState(false)
  const { loading, summary, report, pending, refresh, checkCompat, compat, domainCaps, compatLoading, compatError } = hardware
  const panelRef = useRef<HTMLElement>(null)
  useFocusTrap(panelRef, open, onClose)

  if (!open) return null

  const s = summary as VmHardwareSummary | null
  const cpu = sectionFromReport(report, 'cpu', s?.cpu ?? '—')
  const memory = sectionFromReport(report, 'memory', s?.memory ?? '—')
  const firmware = sectionFromReport(report, 'firmware', s?.firmware ?? '—')
  const tpm = sectionFromReport(report, 'tpm', s?.tpm ?? '—')
  const display = sectionFromReport(report, 'display', s?.display ?? '—')
  const video = sectionFromReport(report, 'video', s?.video ?? '—')
  const diskBus = sectionFromReport(report, 'disk_bus', s?.diskBus ?? '—')
  const nic = sectionFromReport(report, 'nic', s?.nic ?? '—')
  const guestAgent = sectionFromReport(report, 'guest_agent', s?.guestAgent ?? '—')
  const hostDevices = sectionFromReport(report, 'host_devices', s?.hostDevices ?? '—')
  const migration = sectionFromReport(report, 'migration', s?.migration ?? '—')

  const pendingBadge = pending?.needs_shutdown && !report ? ('restart' as const) : null
  const rdpExposed = s?.rdpExposed ?? false
  const rdpHostPort = s?.rdpHostPort ?? null
  const primaryAccess = s?.primaryAccess ?? display.value

  const openEdit = (section: SectionId = 'cpu') => {
    setEditSection(section)
    setEditOpen(true)
  }

  const runCompatCheck = async () => {
    setCompatOpen(true)
    await checkCompat()
  }

  const exposeRdp = async () => {
    setExposeBusy(true)
    try {
      await exposeGuestPortOnVm(vmId, vmName, 3389, portForwardRules)
      toast.success('RDP port 3389 exposed on hypervisor')
      onPlanRefresh?.()
      await refresh(true)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setExposeBusy(false)
    }
  }

  const hasRdp = protocols.some((p) => p.includes('rdp'))
  const hasVnc = protocols.some((p) => p === 'novnc' || p.includes('vnc'))
  const hasSpice = protocols.some((p) => p === 'spice' || p === 'webrtc_spice')

  return (
    <>
      <button type="button" className="fixed inset-0 z-[75] bg-black/40 backdrop-blur-sm" aria-label="Close Hardware" onClick={onClose} />
      <aside
        ref={panelRef}
        className="fixed top-0 right-0 z-[80] h-full w-full max-w-md bg-[var(--apple-surface)]/95 border-l border-white/10 shadow-2xl flex flex-col overflow-hidden animate-in slide-in-from-right duration-200"
        role="dialog"
        aria-modal="true"
        aria-label="Hardware"
        data-testid="vm-hardware-drawer"
      >
        <header className="flex items-center justify-between px-4 py-3 border-b border-white/10 shrink-0">
          <div>
            <h2 className="font-semibold text-[var(--text-primary)]">Hardware</h2>
            <p className="text-xs text-[var(--text-muted)]">{vmName}</p>
          </div>
          <button type="button" onClick={onClose} aria-label="Close" className="p-1.5 rounded hover:bg-white/10 text-[var(--text-muted)]"><X className="w-5 h-5" aria-hidden="true" /></button>
        </header>

        <div className="flex-1 overflow-y-auto p-4 space-y-4 text-sm">
          {loading && !summary && !report ? (
            <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading hardware…</p>
          ) : summary || report ? (
            <div className="rounded-lg border border-white/[0.08] bg-[var(--apple-surface)] px-3 py-1">
              <VmHardwareSection label="CPU" value={cpu.value} badges={cpu.badges} badge={pendingBadge} testId="vm-hardware-cpu" />
              <VmHardwareSection label="Memory" value={memory.value} badges={memory.badges} badge={pendingBadge} testId="vm-hardware-memory" />
              <VmHardwareSection label="Firmware" value={firmware.value} badges={firmware.badges} badge={pendingBadge} testId="vm-hardware-firmware" />
              <VmHardwareSection label="TPM" value={tpm.value} badges={tpm.badges} badge={pendingBadge} testId="vm-hardware-tpm" />
              <VmHardwareSection label="Display" value={display.value} badges={display.badges} testId="vm-hardware-display" />
              <VmHardwareSection label="Video" value={video.value} badges={video.badges} testId="vm-hardware-video" />
              <VmHardwareSection label="Disk bus" value={diskBus.value} badges={diskBus.badges} testId="vm-hardware-disk-bus" />
              <VmHardwareSection label="NIC" value={nic.value} badges={nic.badges} testId="vm-hardware-nic" />
              <VmHardwareSection label="Guest agent" value={guestAgent.value} badges={guestAgent.badges} testId="vm-hardware-guest-agent" />
              <VmHardwareSection label="Host devices" value={hostDevices.value} badges={hostDevices.badges} testId="vm-hardware-host-devices" />
              <VmHardwareSection label="Migration" value={migration.value} badges={migration.badges} testId="vm-hardware-migration" />
            </div>
          ) : (
            <p className="text-sm text-[var(--text-muted)]">Hardware details unavailable.</p>
          )}

          {report?.windows_readiness ? (
            <VmWindowsReadinessPanel
              report={report.windows_readiness}
              rdpExposed={rdpExposed}
              rdpHostPort={rdpHostPort}
            />
          ) : null}

          {compatOpen ? (
            <VmHardwareCompatPanel loading={compatLoading} report={compat} domainCaps={domainCaps} error={compatError} />
          ) : null}

          <div className="rounded-lg border border-violet-500/20 bg-[var(--apple-surface)] p-3 space-y-2" data-testid="vm-hardware-access-section">
            <p className="text-xs font-medium text-[var(--text-primary)] flex items-center gap-1.5"><Monitor className="w-3.5 h-3.5" /> Display &amp; Access</p>
            <p className="text-xs text-[var(--text-muted)]">Primary: {primaryAccess}</p>
            <p className="text-xs text-[var(--text-muted)]">
              RDP 3389: {rdpExposed ? `exposed :${rdpHostPort}` : 'not exposed'}
            </p>
            <div className="flex flex-wrap gap-2 pt-1">
              {hasRdp ? (
                <Link to={cinemaHubPath(vmId)} className="btn-secondary text-xs" onClick={onClose}>
                  Open RDP
                </Link>
              ) : null}
              {hasVnc ? (
                <Link to={cinemaHubPath(vmId)} className="btn-secondary text-xs" onClick={onClose}>
                  VNC fallback
                </Link>
              ) : null}
              {hasSpice ? (
                <Link to={cinemaHubPath(vmId)} className="btn-secondary text-xs" onClick={onClose}>
                  SPICE console
                </Link>
              ) : null}
              {!readOnly && !rdpExposed ? (
                <button type="button" className="btn-primary text-xs" disabled={exposeBusy} onClick={() => void exposeRdp()}>
                  {exposeBusy ? 'Exposing…' : 'Expose 3389'}
                </button>
              ) : null}
            </div>
          </div>

          <div className="flex flex-wrap gap-2">
            <button type="button" className="btn-primary text-xs" disabled={readOnly || managed === false} onClick={() => openEdit('cpu')} data-testid="vm-hardware-edit">
              Edit Hardware
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={readOnly || managed === false}
              onClick={(e) => {
                e.stopPropagation()
                setAttachOpen(true)
              }}
              data-testid="vm-hardware-attach-device"
            >
              Attach Device
            </button>
            <button type="button" className="btn-secondary text-xs" disabled={readOnly || managed === false} onClick={() => openEdit('xml')}>
              Advanced XML
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={readOnly || managed === false || compatLoading}
              onClick={() => void runCompatCheck()}
              data-testid="vm-hardware-compat-check"
            >
              {compatLoading ? 'Checking…' : 'Compatibility Check'}
            </button>
          </div>
        </div>
      </aside>

      <VmEditHardwareDrawer
        open={editOpen}
        onClose={() => setEditOpen(false)}
        vmId={vmId}
        vmName={vmName}
        hostId={hostId}
        managed={managed}
        vmState={vmState}
        hardware={hardware}
        canBrowseHost={canBrowseHost}
        initialSection={editSection}
      />
      <VmHostDeviceAttachDrawer
        open={attachOpen}
        onClose={() => setAttachOpen(false)}
        vmId={vmId}
        vmName={vmName}
        hostId={hostId}
        domainXml={hardware.domainXml}
        managed={managed}
        readOnly={readOnly}
        onAttached={() => void refresh(true)}
      />
    </>
  )
}

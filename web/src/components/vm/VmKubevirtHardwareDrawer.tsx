// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { Copy, Loader2, Monitor, X } from 'lucide-react'
import { useFocusTrap } from '../../hooks/useFocusTrap'
import { Link } from 'react-router'
import type { UseKubevirtHardwareResult } from '../../hooks/useKubevirtHardware'
import { patchK8sKubevirtVmSpec } from '../../api/k8s'
import VmHardwareSection from './VmHardwareSection'
import { cinemaHubPath } from '../../utils/consoleExperienceMode'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

type Props = {
  open: boolean
  onClose: () => void
  vmId: string
  vmName: string
  hardware: UseKubevirtHardwareResult
}

function kubevirtVmStopped(hardware: UseKubevirtHardwareResult): boolean {
  const phase = hardware.row?.vmi_phase?.toLowerCase() ?? ''
  const status = hardware.row?.vm_printable_status?.toLowerCase() ?? ''
  const observed = hardware.vm?.observed_state?.toLowerCase() ?? ''
  if (observed === 'stopped' || observed === 'shutoff') return true
  if (status.includes('stop') || status.includes('halt')) return true
  return phase === '' || phase === 'succeeded' || phase === 'failed'
}

export default function VmKubevirtHardwareDrawer({ open, onClose, vmId, vmName, hardware }: Props) {
  const toast = useToastContext()
  const { loading, error, summary, row, vm, refresh } = hardware
  const [vcpusDraft, setVcpusDraft] = useState('')
  const [memoryGiDraft, setMemoryGiDraft] = useState('')
  const [saving, setSaving] = useState(false)
  const panelRef = useRef<HTMLElement>(null)
  useFocusTrap(panelRef, open, onClose)

  useEffect(() => {
    if (!vm) return
    setVcpusDraft(vm.vcpus > 0 ? String(vm.vcpus) : '')
    const gi = vm.memory_mib > 0 ? Math.max(1, Math.round(vm.memory_mib / 1024)) : 0
    setMemoryGiDraft(gi > 0 ? String(gi) : '')
  }, [vm])

  if (!open) return null

  const canEdit = Boolean(vm) && kubevirtVmStopped(hardware)
  const ns = vm?.k8s_namespace ?? row?.namespace ?? 'default'

  const copy = async (label: string, value: string) => {
    try {
      await navigator.clipboard.writeText(value)
      toast.success(`${label} copied`)
    } catch {
      toast.error('Clipboard unavailable')
    }
  }

  const saveSpec = async () => {
    if (!vm || !canEdit) return
    const vcpus = vcpusDraft.trim() ? Number.parseInt(vcpusDraft, 10) : undefined
    const memoryGi = memoryGiDraft.trim() ? Number.parseInt(memoryGiDraft, 10) : undefined
    if (vcpus !== undefined && (!Number.isFinite(vcpus) || vcpus <= 0)) {
      toast.error('vCPUs must be a positive integer')
      return
    }
    if (memoryGi !== undefined && (!Number.isFinite(memoryGi) || memoryGi <= 0)) {
      toast.error('Memory must be a positive GiB value')
      return
    }
    if (vcpus === undefined && memoryGi === undefined) {
      toast.error('Change CPU or memory before saving')
      return
    }
    setSaving(true)
    try {
      await patchK8sKubevirtVmSpec(ns, vm.name, {
        vcpus,
        memory_mib: memoryGi !== undefined ? memoryGi * 1024 : undefined,
      })
      toast.success('VirtualMachine template updated')
      await refresh(true)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <>
      <button type="button" className="fixed inset-0 z-[75] bg-black/40 backdrop-blur-sm" aria-label="Close Hardware" onClick={onClose} />
      <aside
        ref={panelRef}
        className="fixed top-0 right-0 z-[80] h-full w-full max-w-md bg-[var(--apple-surface)]/95 border-l border-white/10 shadow-2xl flex flex-col overflow-hidden animate-in slide-in-from-right duration-200"
        role="dialog"
        aria-modal="true"
        aria-label="KubeVirt hardware"
        data-testid="vm-kubevirt-hardware-drawer"
      >
        <header className="flex items-center justify-between px-4 py-3 border-b border-white/10 shrink-0">
          <div>
            <h2 className="font-semibold text-[var(--text-primary)]">Hardware</h2>
            <p className="text-xs text-[var(--text-muted)]">{vmName} · KubeVirt</p>
          </div>
          <button type="button" onClick={onClose} aria-label="Close" className="p-1.5 rounded hover:bg-white/10 text-[var(--text-muted)]"><X className="w-5 h-5" aria-hidden="true" /></button>
        </header>

        <div className="flex-1 overflow-y-auto p-4 space-y-4 text-sm">
          {loading && !summary ? (
            <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading cluster hardware…</p>
          ) : error ? (
            <p className="text-sm text-rose-600">{error}</p>
          ) : summary ? (
            <>
              <div className="rounded-lg border border-[var(--accent)]/40 bg-[var(--accent-soft)] p-3 text-xs text-[var(--text-primary)]" data-testid="vm-kubevirt-hardware-note">
                {canEdit
                  ? 'Stopped VM — patch CPU and memory on the VirtualMachine template below. Start the VM after saving.'
                  : 'Running VM — stop the guest before editing template CPU or memory.'}
              </div>

              {canEdit ? (
                <div className="rounded-lg border border-emerald-500/20 bg-[var(--apple-surface)] p-3 space-y-3" data-testid="vm-kubevirt-hardware-edit">
                  <p className="text-xs font-medium text-emerald-800">Edit template</p>
                  <div className="grid grid-cols-2 gap-3">
                    <label className="text-xs text-[var(--text-muted)]">
                      vCPUs
                      <input
                        type="number"
                        min={1}
                        className="input-field w-full mt-1"
                        value={vcpusDraft}
                        onChange={(e) => setVcpusDraft(e.target.value)}
                        data-testid="vm-kubevirt-hardware-vcpus"
                      />
                    </label>
                    <label className="text-xs text-[var(--text-muted)]">
                      Memory (GiB)
                      <input
                        type="number"
                        min={1}
                        className="input-field w-full mt-1"
                        value={memoryGiDraft}
                        onChange={(e) => setMemoryGiDraft(e.target.value)}
                        data-testid="vm-kubevirt-hardware-memory"
                      />
                    </label>
                  </div>
                  <button
                    type="button"
                    className="btn-primary text-xs"
                    disabled={saving}
                    onClick={() => void saveSpec()}
                    data-testid="vm-kubevirt-hardware-save"
                  >
                    {saving ? 'Saving…' : 'Save template'}
                  </button>
                </div>
              ) : null}

              <div className="rounded-lg border border-white/[0.08] bg-[var(--apple-surface)] px-3 py-1">
                <VmHardwareSection label="CPU" value={summary.cpu} testId="vm-hardware-cpu" />
                <VmHardwareSection label="Memory" value={summary.memory} testId="vm-hardware-memory" />
                <VmHardwareSection label="Firmware" value={summary.firmware} testId="vm-hardware-firmware" />
                <VmHardwareSection label="TPM" value={summary.tpm} testId="vm-hardware-tpm" />
                <VmHardwareSection label="Display" value={summary.display} testId="vm-hardware-display" />
                <VmHardwareSection label="Video" value={summary.video} testId="vm-hardware-video" />
                <VmHardwareSection label="Disk bus" value={summary.diskBus} testId="vm-hardware-disk-bus" />
                <VmHardwareSection label="NIC" value={summary.nic} testId="vm-hardware-nic" />
                <VmHardwareSection label="Guest agent" value={summary.guestAgent} testId="vm-hardware-guest-agent" />
                <VmHardwareSection label="Host devices" value={summary.hostDevices} testId="vm-hardware-host-devices" />
                <VmHardwareSection label="Migration" value={summary.migration} testId="vm-hardware-migration" />
                <VmHardwareSection label="Cluster" value={summary.cluster} testId="vm-kubevirt-hardware-cluster" />
                <VmHardwareSection label="Node" value={summary.node} testId="vm-kubevirt-hardware-node" />
                <VmHardwareSection label="VMI phase" value={summary.vmiPhase} testId="vm-kubevirt-hardware-vmi" />
              </div>

              <div className="rounded-lg border border-violet-500/20 bg-[var(--apple-surface)] p-3 space-y-2" data-testid="vm-kubevirt-hardware-access">
                <p className="text-xs font-medium text-[var(--text-primary)] flex items-center gap-1.5"><Monitor className="w-3.5 h-3.5" /> Console access</p>
                <p className="text-xs text-[var(--text-muted)]">{summary.primaryAccess}</p>
                <div className="space-y-2">
                  <div className="flex items-start gap-2">
                    <code className="text-[11px] text-[var(--text-secondary)] break-all flex-1">{summary.virtctlVnc}</code>
                    <button type="button" className="btn-secondary text-xs shrink-0" onClick={() => void copy('virtctl VNC', summary.virtctlVnc)}>
                      <Copy className="w-3 h-3" />
                    </button>
                  </div>
                  <div className="flex items-start gap-2">
                    <code className="text-[11px] text-[var(--text-secondary)] break-all flex-1">{summary.virtctlConsole}</code>
                    <button type="button" className="btn-secondary text-xs shrink-0" onClick={() => void copy('virtctl console', summary.virtctlConsole)}>
                      <Copy className="w-3 h-3" />
                    </button>
                  </div>
                </div>
                {row?.vnc_subresource_path ? (
                  <p className="text-[11px] text-[var(--text-muted)]">VNC API: {row.vnc_subresource_path}</p>
                ) : null}
              </div>
            </>
          ) : (
            <p className="text-sm text-[var(--text-muted)]">Hardware details unavailable.</p>
          )}

          <div className="flex flex-wrap gap-2">
            <Link to={cinemaHubPath(vmId)} className="btn-primary text-xs" onClick={onClose}>
              Open Cinema
            </Link>
            <Link to="/k8s/workloads" className="btn-secondary text-xs" onClick={onClose}>
              K8s workloads
            </Link>
          </div>
        </div>
      </aside>
    </>
  )
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import {
  Camera,
  Pause,
  Play,
  Power,
  RotateCcw,
  Square,
  Trash2,
  X,
} from 'lucide-react'
import PageLayout from '../../../components/PageLayout'
import ConfirmDialog from '../../../components/ConfirmDialog'
import { StructuredErrorBanner } from '../../../components/StructuredErrorBanner'
import SimpleCreateVmWizard from '../../../components/platform/SimpleCreateVmWizard'
import WindowsCreateWizard from '../../../components/platform/WindowsCreateWizard'
import MigratePrecheckModal from '../../../components/platform/MigratePrecheckModal'
import { installStateTone } from '../../../components/platform/GuestAgentDiagnosticsPanel'
import { statusPillClasses } from '../../../utils/semanticColors'
import { useToastContext } from '../../../contexts/ToastContext'
import { toastQueuedOperation } from '../../../utils/platformTaskToast'
import VmPlatformSshConnectDialog from '../../../components/vm/VmPlatformSshConnectDialog'
import MachineFinderBriefing from './MachineFinderBriefing'
import MachineFinderResourceStrip from '../../../components/platform/MachineFinderResourceStrip'
import MachineFinderCanvas from './MachineFinderCanvas'
import MachineFinderCommandBar from './MachineFinderCommandBar'
import MachineFinderCommandCenter from './MachineFinderCommandCenter'
import MachineFinderLensBar from './MachineFinderLensBar'
import MachineFinderSmartFolders from './MachineFinderSmartFolders'
import VmGalleryLauncher, { categorizeVmsForGallery } from '../../../components/platform/VmGalleryLauncher'
import { useMachineFinder } from './useMachineFinder'

export default function MachineFinderPage() {
  const toast = useToastContext()
  const state = useMachineFinder()
  const {
    error,
    fleetGuestReport,
    setFleetGuestReport,
    wizardOpen,
    setWizardOpen,
    wizardInitial,
    windowsOpen,
    setWindowsOpen,
    migrateModal,
    setMigrateModal,
    sshVm,
    setSshVm,
    selectedVmIds,
    batchDeleteOpen,
    setBatchDeleteOpen,
    batchDeleteBusy,
    batchPowerBusy,
    handleCreate,
    handleBatchDelete,
    handleBatchPower,
    handleBatchSnapshot,
    load,
    tier,
    lens,
    filteredVms,
  } = state

  const showSidebar = lens === 'grid' || lens === 'table' || lens === 'migration' || lens === 'gallery'
  const galleryRows = categorizeVmsForGallery(filteredVms)

  return (
    <PageLayout compact hideHeader contentClassName="machine-finder-page pb-[calc(var(--dock-height,0px)+5rem)]">
      <section className="machine-finder-root flex flex-col gap-4 min-h-0" data-testid="machine-finder-page">
        <MachineFinderCommandBar state={state} />

        {error && (
          <div className="space-y-2">
            <StructuredErrorBanner error={error} />
            <div className="flex flex-wrap gap-2 pl-1">
              <button type="button" className="btn-secondary text-xs" onClick={() => void load()}>Retry</button>
              <Link to="/platform/settings?section=general" className="btn-secondary text-xs">Controller settings</Link>
            </div>
          </div>
        )}

        {fleetGuestReport && (
          <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm relative shadow-[var(--shadow-1)]">
            <button type="button" className="absolute top-3 right-3 p-1 text-[var(--text-muted)] hover:text-[var(--text-primary)]" aria-label="Dismiss" onClick={() => setFleetGuestReport(null)}>
              <X className="w-4 h-4" />
            </button>
            <p className="text-[var(--text-primary)] pr-8">{fleetGuestReport.summary}</p>
            <p className="text-xs text-[var(--text-muted)] mt-1">{fleetGuestReport.matched_count} matched · {fleetGuestReport.scanned_count} scanned</p>
            {fleetGuestReport.matched_count > 0 && (
              <ul className="mt-2 text-xs text-[var(--text-secondary)] space-y-1 max-h-32 overflow-y-auto">
                {fleetGuestReport.vms.map((v) => (
                  <li key={v.vm_id}>
                    <Link to={`/platform/vms/${v.vm_id}?tab=guestHealth`} className="font-mono text-[var(--link)] hover:underline">{v.vm_name}</Link>
                    {' — '}
                    <span className={statusPillClasses(installStateTone(v.install_state))}>{v.install_state}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        <div className="flex flex-col gap-4 flex-1 min-h-0 w-full">
          {showSidebar && <MachineFinderSmartFolders state={state} />}
          {showSidebar && <MachineFinderBriefing state={state} />}
          <MachineFinderResourceStrip />
          <MachineFinderLensBar state={state} />
          {lens === 'gallery' ? (
            <div className="space-y-6" data-testid="machine-finder-gallery">
              <VmGalleryLauncher vms={filteredVms.slice(0, 8)} title="Continue working" />
              <VmGalleryLauncher vms={galleryRows.running} title="Running machines" />
              <VmGalleryLauncher vms={galleryRows.linux} title="Linux machines" />
              <VmGalleryLauncher vms={galleryRows.windows} title="Windows machines" />
              <VmGalleryLauncher vms={galleryRows.needsAttention} title="Needs attention" />
            </div>
          ) : (
            <MachineFinderCanvas state={state} />
          )}
          {showSidebar && <MachineFinderCommandCenter state={state} />}
        </div>
      </section>

      <SimpleCreateVmWizard open={wizardOpen} onClose={() => setWizardOpen(false)} onCreate={handleCreate} initial={wizardInitial} />
      <WindowsCreateWizard open={windowsOpen} onClose={() => setWindowsOpen(false)} onCreate={handleCreate} />
      {migrateModal && (
        <MigratePrecheckModal
          vm={migrateModal.vm}
          destHostId={migrateModal.destId}
          destHostName={migrateModal.destName}
          onClose={() => setMigrateModal(null)}
          onDone={(taskId) => {
            if (taskId) toastQueuedOperation(toast, `Migrating ${migrateModal.vm.name}`, taskId, tier)
            void load()
          }}
        />
      )}
      {sshVm && (
        <VmPlatformSshConnectDialog
          open
          vm={sshVm}
          hosts={state.hosts}
          guestIp={state.displayGuestIp(sshVm)}
          onClose={() => setSshVm(null)}
          onNotify={(m) => toast.success(m)}
        />
      )}

      {selectedVmIds.size > 0 && (
        <div className="fixed bottom-24 left-1/2 -translate-x-1/2 z-[80] bg-[var(--apple-surface-elevated)] backdrop-blur border border-[var(--apple-hairline)] rounded-2xl shadow-[var(--shadow-3)] px-4 py-3 flex items-center gap-3 flex-wrap" data-testid="platform-vm-bulk-bar">
          <span className="text-sm font-medium text-[var(--text-primary)]">{selectedVmIds.size} selected</span>
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" disabled={batchPowerBusy} onClick={() => void handleBatchPower('start')}><Play className="w-4 h-4" /> Start</button>
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" disabled={batchPowerBusy} onClick={() => void handleBatchPower('resume')}><RotateCcw className="w-4 h-4" /> Resume</button>
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" disabled={batchPowerBusy} onClick={() => void handleBatchPower('shutdown')}><Power className="w-4 h-4" /> Shutdown</button>
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" disabled={batchPowerBusy} onClick={() => void handleBatchPower('stop')}><Square className="w-4 h-4" /> Stop</button>
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" disabled={batchPowerBusy} onClick={() => void handleBatchPower('pause')}><Pause className="w-4 h-4" /> Pause</button>
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" disabled={batchPowerBusy} onClick={() => void handleBatchSnapshot()}><Camera className="w-4 h-4" /> Snapshot</button>
          <button type="button" className="btn-destructive text-sm inline-flex items-center gap-1" disabled={batchDeleteBusy} onClick={() => setBatchDeleteOpen(true)}><Trash2 className="w-4 h-4" /> Delete</button>
          <button type="button" aria-label="Clear selection" onClick={() => state.setSelectedVmIds(new Set())} className="p-1.5 hover:bg-[var(--surface-hover)] rounded-lg"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
        </div>
      )}

      <ConfirmDialog
        open={batchDeleteOpen}
        title="Delete machines"
        message={`Permanently delete ${selectedVmIds.size} VM(s)? Type DELETE to confirm.`}
        confirmLabel="Delete all"
        typeToMatch="DELETE"
        typeToMatchLabel="Type DELETE (all caps) to confirm bulk delete:"
        onConfirm={() => void handleBatchDelete()}
        onCancel={() => setBatchDeleteOpen(false)}
      />
      <ConfirmDialog
        open={state.confirmPrune}
        title="Remove missing VM records"
        message={`Remove ${state.filteredVms.length} missing VM record(s) from inventory? This cannot be undone.`}
        confirmLabel="Remove records"
        variant="danger"
        onCancel={() => state.setConfirmPrune(false)}
        onConfirm={() => { state.setConfirmPrune(false); void state.doPruneMissing() }}
      />
      <ConfirmDialog
        open={state.deleteVmTarget !== null}
        title="Delete VM"
        message={`Delete ${state.deleteVmTarget?.name}? This cannot be undone.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => state.setDeleteVmTarget(null)}
        onConfirm={() => {
          const vm = state.deleteVmTarget
          state.setDeleteVmTarget(null)
          if (vm) void state.doVmDeleteAction(vm)
        }}
      />
    </PageLayout>
  )
}

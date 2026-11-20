// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { MacGlassPanel } from './mac/PlatformMacUi'
import ConfirmDialog from '../ConfirmDialog'
import type { HostLinuxUpdates } from '../../api/platform'
import { applyHostPackageUpgrade, previewHostPackageUpgrade } from '../../api/platform'
import { runHostCockpitAction } from '../../api/platformHostCockpit'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

type Props = {
  hostId: string
  updates: HostLinuxUpdates | null
  maintenanceMode: boolean
  packagekit?: { available: boolean; running: boolean; version: string; summary: string }
  onRefresh: () => void
}

export default function HostPackageKitPanel({ hostId, updates, maintenanceMode, packagekit, onRefresh }: Props) {
  const toast = useToastContext()
  const [busy, setBusy] = useState(false)
  const [installPkg, setInstallPkg] = useState('')
  const [removePkg, setRemovePkg] = useState('')
  const [purge, setPurge] = useState(false)
  const [confirmApplyAll, setConfirmApplyAll] = useState(false)

  const run = async (fn: () => Promise<void>) => {
    setBusy(true)
    try {
      await fn()
      onRefresh()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <MacGlassPanel
      title="PackageKit & updates"
      subtitle={packagekit?.summary ?? `${updates?.backend ?? 'distro'} package manager`}
      data-testid="host-packagekit-panel"
    >
      <div className="space-y-3 text-sm">
        {packagekit ? (
          <p className="text-xs text-[var(--text-muted)]">
            PackageKit {packagekit.running ? 'running' : packagekit.available ? 'installed' : 'not detected'}
            {packagekit.version ? ` · ${packagekit.version}` : ''}
          </p>
        ) : null}

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={busy}
            onClick={() => void run(async () => {
              const r = await runHostCockpitAction(hostId, 'cockpit.packagekit.refresh', {})
              toast.success(r.message ?? 'Cache refreshed')
            })}
          >
            Refresh cache
          </button>
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={busy}
            onClick={() => void run(async () => {
              const r = await previewHostPackageUpgrade(hostId)
              toast.info(r.summary ?? 'Preview complete')
            })}
          >
            Preview upgrades
          </button>
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={busy || !maintenanceMode}
            onClick={() => setConfirmApplyAll(true)}
          >
            Apply all updates
          </button>
        </div>

        <div className="grid gap-2 sm:grid-cols-[1fr_auto]">
          <input aria-label="Install package name" className="input text-xs" value={installPkg} onChange={(e) => setInstallPkg(e.target.value)} placeholder="Install package name" />
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={busy || !maintenanceMode || !installPkg.trim()}
            onClick={() => void run(async () => {
              const r = await runHostCockpitAction(hostId, 'host.package.install', { packages: [installPkg.trim()] })
              toast.success(r.message ?? 'Installed')
              setInstallPkg('')
            })}
          >
            Install
          </button>
        </div>

        <div className="grid gap-2 sm:grid-cols-[1fr_auto] items-center">
          <input aria-label="Remove package name" className="input text-xs" value={removePkg} onChange={(e) => setRemovePkg(e.target.value)} placeholder="Remove package name" />
          <div className="flex items-center gap-2">
            <label className="text-xs text-[var(--text-muted)] inline-flex items-center gap-1">
              <input type="checkbox" checked={purge} onChange={(e) => setPurge(e.target.checked)} /> Purge
            </label>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={busy || !maintenanceMode || !removePkg.trim()}
              onClick={() => void run(async () => {
                const r = await runHostCockpitAction(hostId, 'host.package.remove', { packages: [removePkg.trim()], purge })
                toast.success(r.message ?? 'Removed')
                setRemovePkg('')
              })}
            >
              Remove
            </button>
          </div>
        </div>

        {!maintenanceMode ? (
          <p className="text-xs text-amber-600/90">Enter maintenance mode before install/remove/apply operations.</p>
        ) : null}
      </div>
      <ConfirmDialog
        open={confirmApplyAll}
        title="Apply all package updates"
        message="Apply all pending package upgrades on this host?"
        confirmLabel="Apply all"
        variant="warning"
        onCancel={() => setConfirmApplyAll(false)}
        onConfirm={() => {
          setConfirmApplyAll(false)
          void run(async () => {
            const r = await applyHostPackageUpgrade(hostId)
            toast.success(r.summary ?? `Task ${r.task_id}`)
          })
        }}
      />
    </MacGlassPanel>
  )
}

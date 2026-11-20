// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { migratePrecheck, vmMigrate, type MigratePrecheckResult, type PlatformVm } from '../../api/platform'
import { statusToneClass } from '../../utils/semanticColors'
import { useFocusTrap } from '../../hooks/useFocusTrap'

interface MigratePrecheckModalProps {
  vm: PlatformVm
  destHostId: string
  destHostName: string
  onClose: () => void
  onDone: (taskId?: string) => void
}

export default function MigratePrecheckModal({ vm, destHostId, destHostName, onClose, onDone }: MigratePrecheckModalProps) {
  const [precheck, setPrecheck] = useState<MigratePrecheckResult | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  useFocusTrap(panelRef, true, onClose)

  useEffect(() => {
    void (async () => {
      try {
        setPrecheck(await migratePrecheck(vm.id, destHostId))
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'Pre-check failed')
      }
    })()
  }, [vm.id, destHostId])

  const migrate = async () => {
    setBusy(true)
    try {
      const r = await vmMigrate(vm.id, { dest_host_id: destHostId })
      onDone(r.task_id)
      onClose()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Migration failed')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="fixed inset-0 z-[80] bg-black/60 backdrop-blur-sm flex items-center justify-center p-4" onClick={onClose}>
      <div ref={panelRef} className="w-full max-w-2xl max-h-[85dvh] overflow-y-auto rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] shadow-2xl" role="dialog" aria-modal="true" aria-label="Migrate precheck" onClick={(e) => e.stopPropagation()}>
        <div className="p-5 border-b border-[var(--apple-hairline)]">
          <h2 className="text-lg font-semibold">Live migrate {vm.name}</h2>
          <p className="text-sm text-[var(--text-muted)] mt-1">Target host: {destHostName}</p>
        </div>
        <div className="p-5 space-y-3 text-sm">
          {error && <p className={statusToneClass('error')}>{error}</p>}
          {!precheck && !error && <p className="text-[var(--text-muted)]">Running pre-checks…</p>}
          {precheck && (
            <>
              <p className={precheck.ok ? statusToneClass('ok') : statusToneClass('warn')}>
                {precheck.ok ? 'Ready to migrate with minimal downtime (<2s expected)' : 'Some checks failed — review before continuing'}
              </p>
              <ul className="space-y-2 text-xs">
                {precheck.checks.map((c) => (
                  <li key={c.name} className={c.passed ? statusToneClass('ok') : statusToneClass('error')}>
                    {c.name}: {c.message}
                    {c.remediation && !c.passed && <p className="text-[var(--text-muted)] mt-0.5">→ {c.remediation}</p>}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
        <div className="p-5 border-t border-[var(--apple-hairline)] flex justify-end gap-2">
          <button type="button" className="btn-secondary text-sm" onClick={onClose}>Cancel</button>
          <button type="button" className="btn-primary text-sm" disabled={busy || !precheck} onClick={() => void migrate()}>
            {busy ? 'Starting…' : 'Start migration'}
          </button>
        </div>
      </div>
    </div>
  )
}

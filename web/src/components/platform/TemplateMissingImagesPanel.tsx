// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Link } from 'react-router'
import { Download, HardDrive, Loader2 } from 'lucide-react'
import type { MissingTemplateImage } from '../../api/platform'
import { prefetchMissingTemplateImages } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { toastQueuedOperation } from '../../utils/platformTaskToast'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { hubLinkClasses } from '../../utils/semanticColors'

type Props = {
  summary: string
  missing: MissingTemplateImage[]
  autoFetchCount?: number
  onPrefetchQueued?: () => void
}

export default function TemplateMissingImagesPanel({
  summary,
  missing,
  autoFetchCount,
  onPrefetchQueued,
}: Props) {
  const toast = useToastContext()
  const [tier] = usePlatformDesktopTier()
  const [busy, setBusy] = useState(false)
  if (missing.length === 0) return null

  const fetchable = autoFetchCount ?? missing.filter((m) => m.auto_fetch).length
  const manualCount = missing.length - fetchable

  const prefetchAll = async () => {
    setBusy(true)
    try {
      const r = await prefetchMissingTemplateImages()
      if (!r?.task_id) throw new Error('Prefetch did not return a task id')
      toastQueuedOperation(toast, 'Downloading missing golden images', r.task_id, tier)
      if (onPrefetchQueued) window.setTimeout(() => void onPrefetchQueued(), 0)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="rounded-xl border border-amber-500/25 bg-amber-500/10 p-4 space-y-3">
      <div className="flex items-start gap-2">
        <HardDrive className="w-4 h-4 text-amber-600 shrink-0 mt-0.5" />
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-amber-800">Marketplace golden images</p>
          <p className="text-xs text-amber-700/80 mt-0.5">{summary}</p>
          {manualCount > 0 && (
            <p className="text-xs text-amber-700/70 mt-1">
              {manualCount} template{manualCount === 1 ? '' : 's'} need a manual image (Windows: build via Golden Forge / dockur; databases & appliances: upload).
            </p>
          )}
        </div>
      </div>
      <ul className="text-xs space-y-1.5 max-h-32 overflow-y-auto">
        {missing.slice(0, 8).map((m) => (
          <li key={`${m.name}@${m.version}`} className="flex items-center justify-between gap-2">
            <span className="text-[var(--text-primary)] truncate">
              {m.icon ? `${m.icon} ` : ''}{m.name}
              <span className="text-[var(--text-muted)]"> @{m.version}</span>
            </span>
            {m.auto_fetch ? (
              <span className="shrink-0 inline-flex items-center gap-0.5 text-emerald-600/90">
                <Download className="w-3 h-3" /> auto on create
              </span>
            ) : (
              <span className="shrink-0 text-amber-600/70">upload required</span>
            )}
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap items-center gap-3">
        {fetchable > 0 && (
          <button
            type="button"
            className="btn-secondary text-xs inline-flex items-center gap-1"
            disabled={busy}
            onClick={() => void prefetchAll()}
          >
            {busy ? <Loader2 className="w-3 h-3 animate-spin" /> : <Download className="w-3 h-3" />}
            Download {fetchable} image{fetchable === 1 ? '' : 's'}
          </button>
        )}
        <Link to="/platform/templates" className={`text-xs ${hubLinkClasses()}`}>
          Open Templates →
        </Link>
      </div>
    </div>
  )
}

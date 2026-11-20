// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Link } from 'react-router'
import { AlertTriangle, CheckCircle2, Download, Loader2 } from 'lucide-react'
import { getTemplateReadiness, prefetchMissingTemplateImages, type TemplateReadiness } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { formatUserError } from '../../utils/apiError'
import { toastQueuedOperation } from '../../utils/platformTaskToast'
import { statusSurfaceClasses, hubLinkClasses } from '../../utils/semanticColors'

export type { TemplateReadiness }

type Props = {
  loading: boolean
  readiness: TemplateReadiness | null
  templateName?: string
  templateVersion?: string
  onReadinessChange?: (readiness: TemplateReadiness) => void
}

async function pollReadiness(
  name: string,
  version: string,
  attempts = 15,
  intervalMs = 2000,
): Promise<TemplateReadiness | null> {
  for (let i = 0; i < attempts; i += 1) {
    await new Promise((r) => window.setTimeout(r, intervalMs))
    try {
      const next = await getTemplateReadiness(name, version)
      if (next.disk_exists) return next
    } catch {
      /* retry */
    }
  }
  return null
}

export default function VmWizardReadinessBanner({
  loading,
  readiness,
  templateName,
  templateVersion,
  onReadinessChange,
}: Props) {
  const toast = useToastContext()
  const [tier] = usePlatformDesktopTier()
  const [prefetchBusy, setPrefetchBusy] = useState(false)

  if (loading) {
    return (
      <p className="text-sm text-[var(--text-muted)] flex items-center gap-2">
        <Loader2 className="w-4 h-4 animate-spin" /> Checking template readiness…
      </p>
    )
  }
  if (!readiness) return null

  const canPrefetch = !readiness.disk_exists && readiness.auto_fetch && readiness.host_online > 0
  const pendingFetch = readiness.ready && canPrefetch
  const tone = readiness.ready ? (pendingFetch ? 'info' : 'ok') : 'warn'

  const prefetchNow = async () => {
    if (!templateName || !templateVersion) {
      toast.error('Template name required for prefetch')
      return
    }
    setPrefetchBusy(true)
    try {
      const r = await prefetchMissingTemplateImages()
      if (r?.task_id) {
        toastQueuedOperation(toast, 'Downloading golden images', r.task_id, tier)
      }
      const updated = await pollReadiness(templateName, templateVersion)
      if (updated) {
        onReadinessChange?.(updated)
        toast.success('Disk image ready on host')
      } else {
        toast.success('Download started — image will be ready shortly')
        try {
          onReadinessChange?.(await getTemplateReadiness(templateName, templateVersion))
        } catch {
          /* ignore */
        }
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setPrefetchBusy(false)
    }
  }

  return (
    <div className={`rounded-xl border p-3 text-sm ${statusSurfaceClasses(tone)}`}>
      <p className="font-medium flex items-center gap-2">
        {readiness.ready ? (
          pendingFetch ? <Download className="w-4 h-4" /> : <CheckCircle2 className="w-4 h-4" />
        ) : (
          <AlertTriangle className="w-4 h-4" />
        )}
        {readiness.ready
          ? pendingFetch
            ? 'Will download on first create'
            : 'Ready to deploy'
          : readiness.host_online === 0
            ? 'No online hosts'
            : 'Missing disk image'}
      </p>
      <p className="text-xs mt-1 opacity-90">{readiness.remediation}</p>
      {!readiness.disk_exists && (
        <p className="text-xs mt-2 font-mono text-[var(--text-muted)] break-all">{readiness.source_disk}</p>
      )}
      {canPrefetch && templateName && templateVersion && (
        <button
          type="button"
          className="btn-secondary text-xs mt-3 inline-flex items-center gap-1"
          disabled={prefetchBusy}
          onClick={() => void prefetchNow()}
        >
          {prefetchBusy ? <Loader2 className="w-3 h-3 animate-spin" /> : <Download className="w-3 h-3" />}
          Download now
        </button>
      )}
      {!readiness.ready && !readiness.auto_fetch && (
        <p className="text-xs mt-2 opacity-90">
          This template has no public download URL. Copy the golden image to the path above on an online host (SSH/SCP), or upload via Content Library.
        </p>
      )}
      {!readiness.ready && !readiness.auto_fetch && (
        <Link to="/platform/content" className={`text-xs hover:underline mt-2 inline-block ${hubLinkClasses()}`}>
          Content Library →
        </Link>
      )}
    </div>
  )
}

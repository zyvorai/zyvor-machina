// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { AlertCircle } from 'lucide-react'
import { statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'

export interface StructuredPlatformError {
  message: string
  error_code?: string
  remediation?: string
}

export function StructuredErrorBanner({ error }: { error: StructuredPlatformError | string | null }) {
  if (!error) return null
  const e = typeof error === 'string' ? { message: error } : error
  return (
    <div className={`rounded-lg border p-4 text-sm space-y-2 ${statusSurfaceClasses('error')}`}>
      <div className={`flex gap-2 items-start ${statusToneClass('error')}`}>
        <AlertCircle className="w-5 h-5 shrink-0 mt-0.5" />
        <div>
          {e.error_code && (
            <p className={`text-xs uppercase tracking-wide mb-1 opacity-80 ${statusToneClass('error')}`}>{e.error_code.replace(/_/g, ' ')}</p>
          )}
          <p>{e.message}</p>
        </div>
      </div>
      {e.remediation && (
        <p className="text-[var(--text-muted)] pl-7 text-xs">
          <span className="text-[var(--text-muted)]">Remediation:</span> {e.remediation}
        </p>
      )}
    </div>
  )
}

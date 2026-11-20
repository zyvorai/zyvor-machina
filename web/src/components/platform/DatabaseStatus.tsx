// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { apiGet } from '../../api/client'
import { PLATFORM_CONTROLLER_PROXY } from '../../api/platform'
import { formatUserError } from '../../utils/apiError'

interface ControllerHealth {
  database?: string
  database_backend?: string
  database_pool?: { size: number; idle: number }
}

const BACKEND_LABEL: Record<string, string> = {
  sqlite: 'SQLite (embedded)',
  postgres: 'PostgreSQL',
}

/** Which database the controller runs on and whether it answers (from the controller's /health). */
export default function DatabaseStatus() {
  const [health, setHealth] = useState<ControllerHealth | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    apiGet<ControllerHealth>(`${PLATFORM_CONTROLLER_PROXY}/api/v1/health`)
      .then((h) => { if (live) setHealth(h) })
      .catch((e: unknown) => { if (live) setError(formatUserError(e)) })
    return () => { live = false }
  }, [])

  if (error) return <p className="text-xs text-[var(--text-muted)]">Could not read the controller's database status: {error}</p>
  if (!health) return <p className="text-xs text-[var(--text-muted)]">Reading database status…</p>

  const backend = health.database_backend ?? 'sqlite'
  const ok = health.database === 'ok'
  return (
    <div data-testid="database-status" className="text-sm text-[var(--text-primary)] space-y-1">
      <p>
        Backend: <strong>{BACKEND_LABEL[backend] ?? backend}</strong>
        {' · '}
        <span className={ok ? 'text-[var(--success,#1a7f37)]' : 'text-[var(--danger,#b42318)]'}>{ok ? 'answering' : 'not answering'}</span>
        {health.database_pool ? ` · connections ${health.database_pool.size - health.database_pool.idle} in use of ${health.database_pool.size}` : ''}
      </p>
      <p className="text-xs text-[var(--text-muted)]">
        {backend === 'sqlite'
          ? 'Embedded SQLite suits one or two machines. For hundreds of machines or several controllers, set up PostgreSQL: '
          : 'Change or back up the database from a shell: '}
        <code>machinactl db status</code>, <code>machinactl db backup</code>, <code>machinactl db setup …</code>
      </p>
    </div>
  )
}

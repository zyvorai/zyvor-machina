// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useParams, Link, useSearchParams } from 'react-router'
import { ArrowLeft } from 'lucide-react'
import SSHConsole from '../components/SSHConsole'
import AiTerminalSuggestStrip from '../components/ai/AiTerminalSuggestStrip'
import { statusActionLinkClasses } from '../utils/semanticColors'

export default function SSHPage() {
  const { host: pathHost } = useParams<{ host?: string }>()
  const [searchParams] = useSearchParams()
  const hostFromQuery = searchParams.get('host') ?? ''
  const userFromQuery = searchParams.get('user') ?? 'root'
  const portFromQuery = parseInt(searchParams.get('port') ?? '22', 10)
  const sshPort = Number.isFinite(portFromQuery) && portFromQuery > 0 ? portFromQuery : undefined
  const vmIdFromQuery = searchParams.get('vmId') ?? undefined
  const vmNameFromQuery = searchParams.get('vmName') ?? undefined

  const raw = pathHost ?? hostFromQuery
  const host = raw ? decodeURIComponent(raw) : ''

  if (!host.trim()) {
    return (
      <div className="space-y-4 animate-fade-in text-center text-[var(--text-muted)] py-12">
        <h1 className="sr-only">SSH terminal</h1>
        <p>No host specified.</p>
        <p className="text-sm">
          Use <code className="text-[var(--text-muted)]">/ssh?host=192.168.122.10&amp;user=root</code> or open SSH from a VM details page.
        </p>
        <Link to="/vms" className={statusActionLinkClasses('info')}>Back to VMs</Link>
      </div>
    )
  }

  return (
    <div className="space-y-4 animate-fade-in">
      <h1 className="sr-only">SSH terminal</h1>
      <div className="flex items-center gap-4">
        <Link to="/vms" className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" aria-label="Back">
          <ArrowLeft className="w-5 h-5" aria-hidden="true" />
        </Link>
        <h1 className="text-xl font-bold">SSH — {userFromQuery}@{host}{sshPort && sshPort !== 22 ? `:${sshPort}` : ''}</h1>
      </div>
      {(vmIdFromQuery || vmNameFromQuery) && (
        <AiTerminalSuggestStrip vmId={vmIdFromQuery} vmName={vmNameFromQuery} defaultOpen={false} />
      )}
      <SSHConsole host={host} sshUser={userFromQuery} sshPort={sshPort} />
    </div>
  )
}

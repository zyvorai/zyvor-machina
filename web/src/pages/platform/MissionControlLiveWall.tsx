// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Monitor, RefreshCw } from 'lucide-react'
import PageLayout from '../../components/PageLayout'
import { listPlatformVms, type PlatformVm } from '../../api/platform'
import ConsoleTheatrePreview from '../../components/platform/fleet/ConsoleTheatrePreview'
import { cinemaHubPath } from '../../utils/consoleExperienceMode'
import { formatUserError } from '../../utils/apiError'
import VmStatusBadge from '../../components/VmStatusBadge'

const MAX_LIVE = 6

export default function MissionControlLiveWall() {
  const [vms, setVms] = useState<PlatformVm[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const all = await listPlatformVms()
      setVms(all.filter((v) => (v.observed_state ?? '').toLowerCase().includes('run')).slice(0, MAX_LIVE))
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  return (
    <PageLayout title="Live Preview Wall" subtitle={`Running VMs with live VNC thumbnails (max ${MAX_LIVE} concurrent).`} compact>
      <div className="space-y-4" data-testid="mission-control-live-wall">
        <div className="flex items-center justify-end gap-2">
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" onClick={() => void load()}>
            <RefreshCw className="w-4 h-4" /> Refresh
          </button>
        </div>
        {loading ? (
          <p className="text-sm text-[var(--text-muted)]">Loading fleet previews…</p>
        ) : error ? (
          <div className="text-sm text-red-400">
            Couldn&rsquo;t load fleet previews: {error}{' '}
            <button type="button" className="underline hover:text-red-600" onClick={() => void load()}>Retry</button>
          </div>
        ) : vms.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No running VMs to preview.</p>
        ) : (
          <ul className="divide-y divide-[var(--apple-hairline)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] overflow-hidden">
            {vms.map((vm) => (
              <li key={vm.id} className="flex flex-col" data-testid={`live-wall-tile-${vm.name}`}>
                <div className="flex items-center justify-between gap-3 px-4 py-3 border-b border-[var(--apple-hairline)]">
                  <span className="font-medium text-sm text-[var(--text-primary)] truncate">{vm.name}</span>
                  <VmStatusBadge state={vm.observed_state ?? 'unknown'} />
                </div>
                <div className="min-h-[12rem] bg-[var(--apple-fill-tertiary)]">
                  <ConsoleTheatrePreview vmId={vm.id} vmName={vm.name} connected variant="tile" />
                </div>
                <div className="flex gap-2 p-3 border-t border-[var(--apple-hairline)]">
                  <Link to={cinemaHubPath(vm.id)} data-testid="live-wall-open-cinema" className="btn-primary text-xs flex-1 text-center inline-flex items-center justify-center gap-1">
                    <Monitor className="w-3.5 h-3.5" /> Open Cinema
                  </Link>
                  <Link to={`/platform/vms/${vm.id}`} className="btn-secondary text-xs flex-1 text-center">
                    VM detail
                  </Link>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </PageLayout>
  )
}

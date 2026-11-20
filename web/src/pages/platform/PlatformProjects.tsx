// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { LayoutGrid, Server } from 'lucide-react'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformPageChrome, { PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { TahoeListEmpty, TahoeTableWrap, TahoeToolbar } from '../../components/platform/tahoe/TahoeListKit'
import { getFleetSpaces, type FleetSpacesOverview } from '../../api/platform'
import { useActiveWorkspace } from '../../hooks/useActiveWorkspace'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

export default function PlatformProjects({ embedded }: { embedded?: boolean } = {}) {
  const navigate = useNavigate()
  const { workspace, setWorkspace } = useActiveWorkspace()
  const [fleet, setFleet] = useState<FleetSpacesOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [search, setSearch] = useState('')

  const load = useCallback(async () => {
    setError(null)
    try {
      setFleet(await getFleetSpaces())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const selectSpace = (name: string) => {
    const normalized = name === 'default' ? '' : name
    setWorkspace(normalized)
    if (normalized) navigate(`/platform/vms?project=${encodeURIComponent(normalized)}`)
    else navigate('/platform/vms')
  }

  const filteredSpaces = useMemo(() => {
    if (!fleet) return []
    const q = search.trim().toLowerCase()
    if (!q) return fleet.spaces
    return fleet.spaces.filter((s) => s.name.toLowerCase().includes(q))
  }, [fleet, search])

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      loading={loading && !fleet}
      error={error}
      onErrorRetry={() => void load()}
      title={embedded ? undefined : 'Workspace Spaces'}
      subtitle={embedded ? undefined : (
        fleet
          ? (
            <span className="flex flex-col gap-1">
              <span className="text-[var(--text-muted)]">macOS Stage Manager metaphor — each project is a space grouping fleet VMs.</span>
              {platformStatSubtitle([
                { label: 'Spaces', value: fleet.space_count },
                { label: 'Total VMs', value: fleet.total_vms },
                { label: 'Running', value: fleet.running_vms },
                { label: 'Active', value: workspace || 'All' },
              ])}
            </span>
          )
          : 'macOS Stage Manager metaphor — each project is a space grouping fleet VMs. Click a space to focus the active workspace.'
      )}
      icon={embedded ? undefined : <LayoutGrid className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <OperatingSurfaceLayout testId="platform-projects-page">
        {fleet && <p className="text-sm text-[var(--text-muted)]">{fleet.summary}</p>}

        <TahoeToolbar search={search} onSearchChange={setSearch} placeholder="Search spaces…" />

        {fleet && fleet.spaces.length === 0 ? (
          <TahoeListEmpty
            icon={LayoutGrid}
            title="No workspace spaces yet"
            description="Assign VMs to a project label in Machine Finder to create tenant spaces."
            primaryAction={{ label: 'Open Machine Finder', onClick: () => navigate('/platform/vms') }}
          />
        ) : fleet && fleet.spaces.length > 0 ? (
          <TahoeTableWrap>
            <table className="apple-table w-full text-sm" aria-label="Workspace spaces">
              <thead>
                <tr>
                  <th scope="col">Space</th>
                  <th scope="col" className="text-center">VMs</th>
                  <th scope="col" className="text-center">Running</th>
                  <th scope="col" className="text-center">Hosts</th>
                  <th scope="col" className="text-center">Isolation</th>
                  <th scope="col" className="text-center">Quota</th>
                  <th scope="col" className="text-right" />
                </tr>
              </thead>
              <tbody>
                <tr
                  className={`cursor-pointer hover:bg-[var(--surface-hover)] ${!workspace ? 'bg-[var(--accent)]/5' : ''}`}
                  onClick={() => selectSpace('')}
                >
                  <td className="font-medium text-[var(--text-primary)]">
                    All spaces
                    {!workspace && (
                      <span className="ml-2 text-[10px] text-[var(--link)] border border-[var(--apple-hairline)] px-2 py-0.5 rounded">active</span>
                    )}
                  </td>
                  <td className="text-center">{fleet.total_vms}</td>
                  <td className={`text-center ${statusToneClass('ok')}`}>{fleet.running_vms}</td>
                  <td className="text-center">—</td>
                  <td className="text-center text-xs uppercase text-[var(--text-muted)]">—</td>
                  <td className="text-center text-xs text-[var(--text-muted)]">—</td>
                  <td className="text-right">
                    <Link to="/platform/vms" className={`text-xs ${hubLinkClasses()}`} onClick={(e) => e.stopPropagation()}>
                      Focus
                    </Link>
                  </td>
                </tr>
                {filteredSpaces.map((s) => {
                  const normalized = s.name === 'default' ? '' : s.name
                  const active = workspace === normalized
                  return (
                    <tr
                      key={s.name}
                      className={`cursor-pointer hover:bg-[var(--surface-hover)] ${active ? 'bg-[var(--accent)]/5' : ''}`}
                      onClick={() => selectSpace(s.name)}
                    >
                      <td className="font-medium text-[var(--text-primary)]">
                        {s.name}
                        {active && (
                          <span className="ml-2 text-[10px] text-[var(--link)] border border-[var(--apple-hairline)] px-2 py-0.5 rounded">active</span>
                        )}
                      </td>
                      <td className="text-center">{s.vm_count}</td>
                      <td className={`text-center ${statusToneClass('ok')}`}>{s.running_count}</td>
                      <td className="text-center">
                        <span className="inline-flex items-center gap-1 justify-center">
                          <Server className="w-3 h-3 text-[var(--text-muted)]" />
                          {s.host_count}
                        </span>
                      </td>
                      <td className="text-center text-xs uppercase text-[var(--text-muted)]">{s.network_isolation}</td>
                      <td className="text-center text-xs text-[var(--text-muted)]">{s.quota_status}</td>
                      <td className="text-right">
                        <Link
                          to={`/platform/vms?project=${encodeURIComponent(s.name === 'default' ? '' : s.name)}`}
                          className={`text-xs ${hubLinkClasses()}`}
                          onClick={(e) => { e.stopPropagation(); selectSpace(s.name) }}
                        >
                          Focus space
                        </Link>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </TahoeTableWrap>
        ) : null}

        {fleet && filteredSpaces.length === 0 && fleet.spaces.length > 0 && (
          <p className="text-sm text-center text-[var(--text-muted)] py-6">No spaces match your search.</p>
        )}
      </OperatingSurfaceLayout>
    </PlatformPageChrome>
  )
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { Server } from 'lucide-react'
import PlatformEmptyState from '../../../components/platform/PlatformEmptyState'
import InfrastructureEarthGlobe from '../../../components/platform/InfrastructureEarthGlobe'
import MachineFinderGeography from '../../../components/platform/MachineFinderGeography'
import {
  defaultRackForSite,
  defaultSiteForMission,
  findHostInMission,
  hostsForSiteRack,
} from '../../../utils/machineFinderSelection'
import { getFleetMission, listPlatformVms, type FleetMissionOverview, type PlatformVm } from '../../../api/platform'
import { formatUserError } from '../../../utils/apiError'
import { hubLinkClasses } from '../../../utils/semanticColors'
import type { MachineFinderState } from './useMachineFinder'

type Props = {
  state: MachineFinderState
}

export default function MachineFinderTopologyLens({ state }: Props) {
  const { searchParams, setSearchParams, search } = state
  const [mission, setMission] = useState<FleetMissionOverview | null>(null)
  const [vms, setVms] = useState<PlatformVm[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const site = searchParams.get('site')
  const rack = searchParams.get('rack')
  const hostId = searchParams.get('host')
  const vmId = searchParams.get('vm')

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [m, v] = await Promise.all([getFleetMission(), listPlatformVms()])
      setMission(m)
      setVms(v)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const defaultSite = useMemo(() => (mission ? defaultSiteForMission(mission) : null), [mission])
  const effectiveSite = site ?? defaultSite
  const effectiveRack = useMemo(() => {
    if (!mission || !effectiveSite) return null
    if (rack) return rack
    return defaultRackForSite(mission, effectiveSite)
  }, [mission, effectiveSite, rack])

  const patchParams = useCallback(
    (next: Record<string, string | null>) => {
      const p = new URLSearchParams(searchParams)
      p.set('lens', 'topology')
      for (const [key, val] of Object.entries(next)) {
        if (val == null || val === '') p.delete(key)
        else p.set(key, val)
      }
      setSearchParams(p, { replace: true })
    },
    [searchParams, setSearchParams],
  )

  useEffect(() => {
    if (!mission || !hostId) return
    const ctx = findHostInMission(mission, hostId)
    if (!ctx) return
    if (site === ctx.site && rack === ctx.rack) return
    patchParams({ site: ctx.site, rack: ctx.rack, host: hostId, vm: vmId })
  }, [mission, hostId, site, rack, vmId, patchParams])

  useEffect(() => {
    if (!mission || site || !defaultSite) return
    patchParams({
      site: defaultSite,
      rack: defaultRackForSite(mission, defaultSite),
      host: null,
      vm: null,
    })
  }, [mission, site, defaultSite, patchParams])

  useEffect(() => {
    if (!mission || !effectiveSite || !effectiveRack || hostId) return
    const hosts = hostsForSiteRack(mission, effectiveSite, effectiveRack)
    if (hosts.length !== 1) return
    patchParams({ site: effectiveSite, rack: effectiveRack, host: hosts[0].id, vm: null })
  }, [mission, effectiveSite, effectiveRack, hostId, patchParams])

  const filteredMission = useMemo((): FleetMissionOverview | null => {
    if (!mission) return null
    const q = search.trim().toLowerCase()
    if (!q) return mission
    const matchHost = (hostname: string) => hostname.toLowerCase().includes(q)
    const matchVmHostIds = new Set(
      vms.filter((v) => v.name.toLowerCase().includes(q) && v.host_id).map((v) => v.host_id as string),
    )
    const filterHosts = (hosts: typeof mission.unassigned_hosts) =>
      hosts.filter((h) => matchHost(h.hostname) || matchVmHostIds.has(h.id))
    return {
      ...mission,
      sites: mission.sites
        .map((s) => ({
          ...s,
          racks: s.racks.map((r) => ({ ...r, hosts: filterHosts(r.hosts) })).filter((r) => r.hosts.length > 0),
        }))
        .filter((s) => s.racks.length > 0),
      unassigned_hosts: filterHosts(mission.unassigned_hosts),
    }
  }, [mission, vms, search])

  const hasGeography = filteredMission && (
    filteredMission.sites.length > 0 || filteredMission.unassigned_hosts.length > 0
  )

  if (loading && !mission) {
    return <p className="text-sm text-[var(--text-muted)] p-4">Loading topology…</p>
  }

  if (error) {
    return (
      <div className="rounded-xl border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-800">
        {error}
        <button type="button" className="btn-secondary text-xs ml-2" onClick={() => void load()}>Retry</button>
      </div>
    )
  }

  return (
    <div className="space-y-4" data-testid="machine-finder-topology">
      <div className="flex flex-wrap items-center gap-2">
        <Link to="/platform?mission=1" className="btn-secondary text-sm">Mission Control</Link>
        <button type="button" className="btn-secondary text-sm" onClick={() => void load()}>Refresh</button>
      </div>

      {mission && <InfrastructureEarthGlobe mission={mission} className="mx-0" />}

      {!loading && !hasGeography && (
        <PlatformEmptyState icon={Server} title="No infrastructure geography" subtitle="Enroll hosts and set site/rack metadata.">
          <Link to="/platform/enroll" className="tahoe-btn-primary text-sm">Add host</Link>
          <Link to="/platform/hosts" className={`tahoe-btn-ghost text-sm ${hubLinkClasses()}`}>Browse hosts</Link>
        </PlatformEmptyState>
      )}

      {filteredMission && hasGeography && (
        <MachineFinderGeography
          mission={filteredMission}
          vms={vms}
          selection={{ site: effectiveSite, rack: effectiveRack, hostId, vmId }}
          onSelectSite={(s) => patchParams({ site: s, rack: null, host: null, vm: null })}
          onSelectRack={(r) => patchParams({ site: effectiveSite, rack: r, host: null, vm: null })}
          onSelectHost={(id) => patchParams({ site: effectiveSite, rack: effectiveRack, host: id, vm: null })}
          onSelectVm={(id) => patchParams({ site: effectiveSite, rack: effectiveRack, host: hostId, vm: id })}
        />
      )}
    </div>
  )
}

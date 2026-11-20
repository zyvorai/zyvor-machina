// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { FleetMissionOverview, MissionHost } from '../api/platform'

export const UNASSIGNED_SITE = '__unassigned__'
export const UNASSIGNED_RACK = 'All hosts'

type MachineFinderHostContext = {
  site: string
  rack: string
  host: MissionHost
}

export function findHostInMission(mission: FleetMissionOverview, hostId: string): MachineFinderHostContext | null {
  for (const host of mission.unassigned_hosts) {
    if (host.id === hostId) {
      return { site: UNASSIGNED_SITE, rack: UNASSIGNED_RACK, host }
    }
  }
  for (const site of mission.sites) {
    for (const rack of site.racks) {
      const host = rack.hosts.find((h) => h.id === hostId)
      if (host) return { site: site.name, rack: rack.name, host }
    }
  }
  return null
}

export function hostsForSiteRack(
  mission: FleetMissionOverview,
  site: string | null,
  rack: string | null,
): MissionHost[] {
  if (!site) return []
  if (site === UNASSIGNED_SITE) {
    return rack === UNASSIGNED_RACK || !rack ? mission.unassigned_hosts : []
  }
  const siteNode = mission.sites.find((s) => s.name === site)
  if (!siteNode) return []
  if (!rack) return siteNode.racks.flatMap((r) => r.hosts)
  return siteNode.racks.find((r) => r.name === rack)?.hosts ?? []
}

export function defaultSiteForMission(mission: FleetMissionOverview): string | null {
  if (mission.sites.length > 0) return mission.sites[0].name
  if (mission.unassigned_hosts.length > 0) return UNASSIGNED_SITE
  return null
}

export function defaultRackForSite(mission: FleetMissionOverview, site: string | null): string | null {
  if (!site) return null
  if (site === UNASSIGNED_SITE) return UNASSIGNED_RACK
  return mission.sites.find((s) => s.name === site)?.racks[0]?.name ?? null
}

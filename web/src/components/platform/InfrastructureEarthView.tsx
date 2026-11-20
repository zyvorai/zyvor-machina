// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { MapPin, Server } from 'lucide-react'
import InfrastructureEarthGlobe from './InfrastructureEarthGlobe'
import { hubLinkClasses } from '../../utils/semanticColors'
import type { FleetMissionOverview, MissionHost } from '../../api/platform'
import { statusBgClass, statusSurfaceClasses, utilizationTone } from '../../utils/semanticColors'

function LivingHostCard({ host }: { host: MissionHost }) {
  const memPct = host.memory_total_mib > 0
    ? Math.min(100, Math.round((host.memory_used_mib / host.memory_total_mib) * 100))
    : 0
  const cpuPct = Math.min(100, Math.round(host.cpu_percent))
  const online = host.state === 'online' && !host.maintenance_mode

  return (
    <Link
      to={`/platform/vms?lens=topology&host=${encodeURIComponent(host.id)}&site=${encodeURIComponent(host.site.trim() || '__unassigned__')}&rack=${encodeURIComponent(host.rack.trim() || (host.site.trim() ? 'Unassigned rack' : 'All hosts'))}`}
      className={`block rounded-xl border p-3 transition hover:scale-[1.02] hover:border-[color-mix(in_srgb,var(--machina-status-info)_40%,transparent)] ${
        online ? statusSurfaceClasses('ok') : 'border-white/[0.08] bg-[var(--apple-surface)]'
      }`}
      style={{ transform: 'translateZ(0)' }}
    >
      <div className="flex items-center justify-between gap-2 mb-2">
        <span className="font-medium text-sm text-[var(--text-primary)] truncate">{host.hostname}</span>
        {host.rack_u != null && (
          <span className="text-[10px] text-[var(--text-muted)] shrink-0">U{host.rack_u}</span>
        )}
      </div>
      <div className="space-y-1.5">
        <div>
          <div className="flex justify-between text-[10px] text-[var(--text-muted)] mb-0.5">
            <span>CPU</span><span>{cpuPct}%</span>
          </div>
          <div className="h-1 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden">
            <div
              role="progressbar"
              aria-label="CPU usage"
              aria-valuenow={Math.round(Math.min(100, Math.max(0, cpuPct)))}
              aria-valuemin={0}
              aria-valuemax={100}
              className={`h-full rounded-full transition-all duration-700 ${statusBgClass(utilizationTone(cpuPct))}`}
              style={{ width: `${cpuPct}%` }}
            />
          </div>
        </div>
        <div>
          <div className="flex justify-between text-[10px] text-[var(--text-muted)] mb-0.5">
            <span>Memory</span><span>{memPct}%</span>
          </div>
          <div className="h-1 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden">
            <div
              role="progressbar"
              aria-label="Memory usage"
              aria-valuenow={Math.round(Math.min(100, Math.max(0, memPct)))}
              aria-valuemin={0}
              aria-valuemax={100}
              className="h-full rounded-full bg-[var(--accent)] transition-all duration-700"
              style={{ width: `${memPct}%` }}
            />
          </div>
        </div>
      </div>
      <p className="text-[10px] text-[var(--text-muted)] mt-2 capitalize">{host.state} · {host.vm_count} VMs</p>
    </Link>
  )
}

function isMissionOverview(value: FleetMissionOverview | null): value is FleetMissionOverview {
  return Boolean(value && Array.isArray(value.sites) && Array.isArray(value.unassigned_hosts))
}

export default function InfrastructureEarthView({ mission }: { mission: FleetMissionOverview | null }) {
  if (!isMissionOverview(mission)) {
    return <p className="text-sm text-[var(--text-muted)] px-2">Loading infrastructure map…</p>
  }

  const hasSites = mission.sites.length > 0

  return (
    <section className="space-y-6">
      <div className="flex flex-wrap items-center gap-3 text-xs text-[var(--text-muted)]">
        <span className="inline-flex items-center gap-1.5 rounded-full border border-white/[0.08] px-3 py-1">
          <Server className="w-3.5 h-3.5" />
          {mission.summary.hosts} hosts · {mission.summary.vms} VMs · {mission.summary.health_pct}% healthy
        </span>
        <Link
          to="/platform/vms?lens=topology"
          className={`inline-flex items-center gap-1.5 rounded-full border border-white/[0.08] px-3 py-1 transition hover:border-white/[0.14] ${hubLinkClasses()}`}
        >
          <MapPin className="w-3.5 h-3.5" />
          Open Machine Finder
        </Link>
      </div>

      <InfrastructureEarthGlobe mission={mission} />

      {hasSites ? (
        <div
          className="grid gap-6 lg:grid-cols-2 xl:grid-cols-3"
          style={{ perspective: '1200px' }}
        >
          {mission.sites.map((site) => (
            <div
              key={site.name}
              className="rounded-2xl border border-white/[0.08] bg-[var(--apple-surface)]/50 p-4 space-y-4"
              style={{ transform: 'rotateX(2deg)' }}
            >
              <h3 className="text-sm font-semibold text-[var(--link)]">{site.name}</h3>
              {site.racks.map((rack) => (
                <div key={`${site.name}-${rack.name}`} className="space-y-2">
                  <p className="text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)]">{rack.name}</p>
                  <div className="grid gap-2 sm:grid-cols-2">
                    {rack.hosts.map((host) => (
                      <LivingHostCard key={host.id} host={host} />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
      ) : null}

      {mission.unassigned_hosts.length > 0 && (
        <div className="rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-3">
          <h3 className="text-sm font-semibold text-[var(--text-secondary)]">
            {hasSites ? 'Unassigned hosts' : 'All hosts'}
          </h3>
          <p className="text-xs text-[var(--text-muted)]">
            Set site and rack on host detail to organize your datacenter map.
          </p>
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {mission.unassigned_hosts.map((host) => (
              <LivingHostCard key={host.id} host={host} />
            ))}
          </div>
        </div>
      )}
    </section>
  )
}

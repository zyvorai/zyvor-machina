// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { HardDrive, Network, ArrowUpCircle, ArrowDownCircle } from 'lucide-react'
import { getFleetNetwork, getFleetStorage, listLivePlatformNetworks, type FleetNetworkOverview, type FleetStorageOverview } from '../../api/platform'
import { listLiveStoragePools } from '../../api/platformStorage'

function usagePct(used: number, total: number): number {
  if (total <= 0) return 0
  return Math.min(100, Math.round((used / total) * 100))
}

function UsageBar({ label, pct, tone }: { label: string; pct: number; tone: string }) {
  return (
    <div className="min-w-0">
      <div className="flex justify-between text-[10px] text-[var(--text-muted)] mb-0.5">
        <span>{label}</span>
        <span>{pct}%</span>
      </div>
      <div className="h-1.5 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden">
        <div className={`h-full ${tone}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  )
}

export default function MachineFinderResourceStrip() {
  const [storage, setStorage] = useState<FleetStorageOverview | null>(null)
  const [network, setNetwork] = useState<FleetNetworkOverview | null>(null)
  const [poolActive, setPoolActive] = useState(0)
  const [poolInactive, setPoolInactive] = useState(0)
  const [netActive, setNetActive] = useState(0)
  const [netInactive, setNetInactive] = useState(0)

  useEffect(() => {
    void Promise.all([
      getFleetStorage().then(setStorage).catch(() => setStorage(null)),
      getFleetNetwork().then(setNetwork).catch(() => setNetwork(null)),
      listLiveStoragePools()
        .then((r) => {
          const pools = r.pools ?? []
          setPoolActive(pools.filter((p) => p.state === 'active' || p.state === 'running').length)
          setPoolInactive(pools.filter((p) => p.state !== 'active' && p.state !== 'running').length)
        })
        .catch(() => {
          setPoolActive(0)
          setPoolInactive(0)
        }),
      listLivePlatformNetworks()
        .then((r) => {
          const nets = r.networks ?? []
          setNetActive(nets.filter((n) => n.active).length)
          setNetInactive(nets.filter((n) => !n.active).length)
        })
        .catch(() => {
          setNetActive(0)
          setNetInactive(0)
        }),
    ])
  }, [])

  const storagePct = usagePct(storage?.total_used_gib ?? 0, storage?.total_capacity_gib ?? 0)
  const poolCount = storage?.pools?.length ?? 0
  const netCount = network?.network_count ?? 0

  return (
    <section
      className="grid gap-3 sm:grid-cols-2 rounded-xl border border-white/[0.08] bg-[var(--apple-surface)] px-4 py-3"
      data-testid="machine-finder-resource-strip"
    >
      <div className="flex gap-3 min-w-0">
        <HardDrive className="w-4 h-4 text-[var(--link)] shrink-0 mt-1" />
        <div className="flex-1 min-w-0 space-y-2">
          <p className="text-xs font-medium text-[var(--text-secondary)]">Fleet storage</p>
          <UsageBar label={`${storage?.total_used_gib?.toFixed(0) ?? '—'} / ${storage?.total_capacity_gib?.toFixed(0) ?? '—'} GiB · ${poolCount} pools`} pct={storagePct} tone="bg-[var(--accent)]/70" />
          <div className="flex items-center gap-3 text-[10px] text-[var(--text-muted)]">
            <span className="inline-flex items-center gap-1"><ArrowUpCircle className="w-3 h-3 text-emerald-400" /> {poolActive} active</span>
            <span className="inline-flex items-center gap-1"><ArrowDownCircle className="w-3 h-3 text-[var(--text-muted)]" /> {poolInactive} inactive</span>
          </div>
        </div>
      </div>
      <div className="flex gap-3 min-w-0">
        <Network className="w-4 h-4 text-emerald-400 shrink-0 mt-1" />
        <div className="flex-1 min-w-0 space-y-1">
          <p className="text-xs font-medium text-[var(--text-secondary)]">Fleet networks</p>
          <p className="text-[11px] text-[var(--text-muted)]">
            {netCount} libvirt network(s)
            {network?.hosts_online != null ? ` · ${network.hosts_online} hosts online` : ''}
          </p>
          <div className="flex items-center gap-3 text-[10px] text-[var(--text-muted)]">
            <span className="inline-flex items-center gap-1"><ArrowUpCircle className="w-3 h-3 text-emerald-400" /> {netActive} active</span>
            <span className="inline-flex items-center gap-1"><ArrowDownCircle className="w-3 h-3 text-[var(--text-muted)]" /> {netInactive} inactive</span>
          </div>
        </div>
      </div>
    </section>
  )
}

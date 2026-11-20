// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Layers } from 'lucide-react'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import { formatUserError } from '../../utils/apiError'
import { getStorageTiersOverview, type StorageTierOverview, type StorageTiersOverview } from '../../api/platform'

export default function PlatformStorageTiers() {
  const [data, setData] = useState<StorageTiersOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      setData(await getStorageTiersOverview())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const tiers: StorageTierOverview[] = data?.tiers ?? []
  const totalCapacity = tiers.reduce((s, t) => s + t.capacity_gib, 0)
  const totalUsed = tiers.reduce((s, t) => s + t.used_gib, 0)

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/storage" label="Disk Utility" />}
      title="Storage Tiers"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">{data?.summary ?? 'Storage tier classification and capacity overview.'}</span>
          {platformStatSubtitle([
            { label: 'Tiers', value: tiers.length },
            { label: 'Capacity', value: `${totalCapacity} GiB` },
            { label: 'Used', value: `${totalUsed} GiB` },
          ])}
        </span>
      }
      icon={<Layers className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-6"
    >
      <div className="space-y-6">
        <MacGlassPanel title="Storage Tiers" >
          {tiers.length === 0 ? (
            <PlatformEmptyState
              icon={Layers}
              title="No storage tiers"
              subtitle="No storage tiers have been defined yet."
            />
          ) : (
            <div className="divide-y divide-border/40">
              {tiers.map((tier) => (
                <MacListRow
                  key={tier.id}
                  title={tier.name}
                  subtitle={`${tier.tier_class} · ${tier.iops_tier} IOPS · ${tier.replication} replication · ${tier.pool_count} pool${tier.pool_count !== 1 ? 's' : ''}`}
                  trailing={
                    <span className="text-xs text-muted-foreground shrink-0">
                      {tier.used_gib} / {tier.capacity_gib} GiB
                    </span>
                  }
                  badge={
                    tier.capacity_gib > 0 && tier.used_gib / tier.capacity_gib > 0.85 ? (
                      <span className="text-xs px-1.5 py-0.5 rounded bg-amber-500/10 text-amber-9000">High</span>
                    ) : undefined
                  }
                />
              ))}
            </div>
          )}
        </MacGlassPanel>
      </div>
    </PlatformPageChrome>
  )
}

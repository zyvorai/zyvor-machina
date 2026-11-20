// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import {
  CheckCircle2,
  Download,
  Package,
  Trash2,
} from 'lucide-react'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import { TahoeToolbar } from '../../components/platform/tahoe/TahoeListKit'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import {
  getMarketplacePlugins,
  installMarketplacePlugin,
  uninstallMarketplacePlugin,
  type MarketplacePlugin,
  type MarketplaceOverview,
} from '../../api/platform'

export default function PlatformMarketplace() {
  const toast = useToastContext()
  const [data, setData] = useState<MarketplaceOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [actionInProgress, setActionInProgress] = useState<string | null>(null)
  const [filter, setFilter] = useState('')

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      setData(await getMarketplacePlugins())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const handleInstall = async (plugin: MarketplacePlugin) => {
    setActionInProgress(plugin.slug)
    try {
      await installMarketplacePlugin(plugin.slug)
      toast.success(`${plugin.name} installed`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setActionInProgress(null)
    }
  }

  const handleUninstall = async (plugin: MarketplacePlugin) => {
    setActionInProgress(plugin.slug)
    try {
      await uninstallMarketplacePlugin(plugin.slug)
      toast.success(`${plugin.name} uninstalled`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setActionInProgress(null)
    }
  }

  const allPlugins: MarketplacePlugin[] = (data?.plugins ?? []).filter(
    (p) => !filter || p.name.toLowerCase().includes(filter.toLowerCase()) || p.category.toLowerCase().includes(filter.toLowerCase()),
  )
  const featured = allPlugins.filter((p) => p.featured)
  const others = allPlugins.filter((p) => !p.featured)
  const totalPlugins = data?.plugins?.length ?? 0

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/settings?section=integrations" label="Settings" />}
      title="Marketplace"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">{data?.summary ?? 'Browse and install platform plugins and extensions.'}</span>
          {platformStatSubtitle([
            { label: 'Available', value: totalPlugins },
            { label: 'Installed', value: data?.installed_count ?? 0 },
          ])}
        </span>
      }
      icon={<Package className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-6"
    >
      <div className="space-y-6 xl:space-y-0 xl:columns-2 xl:gap-6 [&>*]:break-inside-avoid xl:[&>*]:mb-6">
        <TahoeToolbar search={filter} onSearchChange={setFilter} placeholder="Filter plugins…" />

        {featured.length > 0 && (
          <MacGlassPanel title="Featured" >
            <div className="divide-y divide-border/40">
              {featured.map((p) => (
                <PluginRow
                  key={p.slug}
                  plugin={p}
                  busy={actionInProgress === p.slug}
                  onInstall={handleInstall}
                  onUninstall={handleUninstall}
                />
              ))}
            </div>
          </MacGlassPanel>
        )}

        <MacGlassPanel title="All Plugins" >
          {others.length === 0 && featured.length === 0 ? (
            <PlatformEmptyState icon={Package} title="No plugins available" subtitle="The marketplace is empty or unreachable." />
          ) : others.length === 0 ? (
            <PlatformEmptyState icon={CheckCircle2} title="All plugins are featured" subtitle="No additional plugins to show." />
          ) : (
            <div className="divide-y divide-border/40">
              {others.map((p) => (
                <PluginRow
                  key={p.slug}
                  plugin={p}
                  busy={actionInProgress === p.slug}
                  onInstall={handleInstall}
                  onUninstall={handleUninstall}
                />
              ))}
            </div>
          )}
        </MacGlassPanel>
      </div>
    </PlatformPageChrome>
  )
}

function PluginRow({
  plugin,
  busy,
  onInstall,
  onUninstall,
}: {
  plugin: MarketplacePlugin
  busy: boolean
  onInstall: (p: MarketplacePlugin) => void
  onUninstall: (p: MarketplacePlugin) => void
}) {
  return (
    <MacListRow
      title={plugin.name}
      subtitle={`${plugin.category} · v${plugin.version} · ${plugin.author} — ${plugin.description}`}
      badge={
        plugin.installed ? (
          <span className="text-xs px-1.5 py-0.5 rounded bg-emerald-500/10 text-emerald-9000">Installed</span>
        ) : undefined
      }
      trailing={
        plugin.installed ? (
          <button
            onClick={() => onUninstall(plugin)}
            disabled={busy}
            className="flex items-center gap-1 px-3 py-1 text-xs rounded-md bg-destructive/10 text-destructive hover:bg-destructive/20 disabled:opacity-50"
          >
            <Trash2 className="w-3 h-3" /> Uninstall
          </button>
        ) : (
          <button
            onClick={() => onInstall(plugin)}
            disabled={busy}
            className="flex items-center gap-1 px-3 py-1 text-xs rounded-md bg-primary/10 text-primary hover:bg-primary/20 disabled:opacity-50"
          >
            <Download className="w-3 h-3" /> Install
          </button>
        )
      }
    />
  )
}

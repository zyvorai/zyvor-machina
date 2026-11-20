// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import {
  AlertTriangle,
  CheckCircle2,
  RefreshCw,
  ShieldCheck,
} from 'lucide-react'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import ConfirmDialog from '../../components/ConfirmDialog'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import {
  getHaStatus,
  listFenceEvents,
  fenceHost,
  listPlatformHosts,
  type HaStatusResponse,
  type FenceEvent,
  type PlatformHost,
} from '../../api/platform'

export default function PlatformHa() {
  const toast = useToastContext()
  const [status, setStatus] = useState<HaStatusResponse | null>(null)
  const [fenceEvents, setFenceEvents] = useState<FenceEvent[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [fencing, setFencing] = useState<string | null>(null)
  const [pendingFence, setPendingFence] = useState<{ id: string; hostname: string } | null>(null)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [ha, events, hostList] = await Promise.all([
        getHaStatus(),
        listFenceEvents(),
        listPlatformHosts(),
      ])
      setStatus(ha)
      setFenceEvents(events)
      setHosts(hostList)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const handleFence = async (hostId: string, hostname: string) => {
    setFencing(hostId)
    try {
      await fenceHost(hostId)
      toast.success(`Fence initiated for ${hostname}`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setFencing(null)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="High Availability"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">HA cluster status, policy, and host fencing controls.</span>
          {status && platformStatSubtitle([
            { label: 'HA-protected VMs', value: status.status.enabled_vms },
            { label: 'Offline hosts', value: status.status.offline_hosts },
            { label: 'Recent events', value: status.status.recent_events },
          ])}
        </span>
      }
      icon={<ShieldCheck className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-6"
    >
      <div className="space-y-6 xl:space-y-0 xl:columns-2 xl:gap-6 [&>*]:break-inside-avoid xl:[&>*]:mb-6">
        <MacGlassPanel title="HA Events" >
          {!status || status.events.length === 0 ? (
            <PlatformEmptyState icon={CheckCircle2} title="No HA events" subtitle="All hosts are healthy." />
          ) : (
            <div className="divide-y divide-border/40">
              {status.events.map((ev) => (
                <MacListRow
                  key={ev.id}
                  title={ev.message}
                  subtitle={ev.action}
                  trailing={<span className="text-xs text-muted-foreground">{new Date(ev.created_at).toLocaleString()}</span>}
                />
              ))}
            </div>
          )}
        </MacGlassPanel>

        <MacGlassPanel title="Host Fencing" >
          {hosts.length === 0 ? (
            <PlatformEmptyState icon={AlertTriangle} title="No hosts" subtitle="Enroll hosts to enable fencing." />
          ) : (
            <div className="divide-y divide-border/40">
              {hosts.map((h) => (
                <MacListRow
                  key={h.id}
                  title={h.hostname}
                  subtitle={h.state}
                  trailing={
                    <button
                      onClick={() => setPendingFence({ id: h.id, hostname: h.hostname })}
                      disabled={fencing === h.id}
                      className="px-3 py-1 text-xs rounded-md bg-[var(--nl-status-danger-bg)] text-[var(--nl-accent-red-text)] hover:opacity-80 disabled:opacity-50"
                    >
                      {fencing === h.id ? (
                        <RefreshCw className="w-3 h-3 animate-spin inline" />
                      ) : (
                        'Fence'
                      )}
                    </button>
                  }
                />
              ))}
            </div>
          )}
        </MacGlassPanel>

        <MacGlassPanel title="Fence Events" >
          {fenceEvents.length === 0 ? (
            <PlatformEmptyState icon={CheckCircle2} title="No fence events" subtitle="No hosts have been fenced recently." />
          ) : (
            <div className="divide-y divide-border/40">
              {fenceEvents.map((ev) => (
                <MacListRow
                  key={ev.id}
                  title={ev.action}
                  subtitle={ev.message ?? undefined}
                  trailing={<span className="text-xs text-muted-foreground">{new Date(ev.created_at).toLocaleString()}</span>}
                  badge={
                    <span className={`text-xs px-1.5 py-0.5 rounded ${ev.success ? 'bg-emerald-500/10 text-emerald-9000' : 'bg-red-500/10 text-red-9000'}`}>
                      {ev.success ? 'OK' : 'Failed'}
                    </span>
                  }
                />
              ))}
            </div>
          )}
        </MacGlassPanel>
      </div>
      <ConfirmDialog
        open={pendingFence !== null}
        title="Fence Host"
        message={`Fence host "${pendingFence?.hostname}"? This will forcibly cut power or reset the machine, terminating all running VMs immediately. Only use in an emergency.`}
        confirmLabel="Fence"
        variant="danger"
        onCancel={() => setPendingFence(null)}
        onConfirm={async () => {
          const p = pendingFence
          setPendingFence(null)
          if (!p) return
          await handleFence(p.id, p.hostname)
        }}
      />
    </PlatformPageChrome>
  )
}

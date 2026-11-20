// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Gauge, Plus, Trash2 } from 'lucide-react'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import {
  channelTargetLabel,
  createAlertRule,
  createNotificationChannel,
  deleteAlertRule,
  listAlertRules,
  listNotificationChannelRows,
  testNotificationChannel,
  setAlertRuleEnabled,
  type AlertComparator,
  type AlertMetric,
  type AlertRule,
  type AlertSeverity,
  type NotificationChannelRow,
} from '../../api/day2'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformAlertRules() {
  const toast = useToastContext()
  const [rows, setRows] = useState<AlertRule[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('high-cpu')
  const [metric, setMetric] = useState<AlertMetric>('cpu_percent')
  const [comparator, setComparator] = useState<AlertComparator>('gt')
  const [threshold, setThreshold] = useState('85')
  const [severity, setSeverity] = useState<AlertSeverity>('warning')
  const [cooldown, setCooldown] = useState('30')
  const [scopeProject, setScopeProject] = useState('')
  const [scopeTag, setScopeTag] = useState('')
  const [saving, setSaving] = useState(false)
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null)
  const [channels, setChannels] = useState<NotificationChannelRow[]>([])
  const [chName, setChName] = useState('')
  const [chKind, setChKind] = useState('slack')
  const [chTarget, setChTarget] = useState('')
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    try {
      const r = await listAlertRules()
      if (alive()) setRows(r)
      const c = await listNotificationChannelRows().catch(() => [] as NotificationChannelRow[])
      if (alive()) setChannels(Array.isArray(c) ? c : [])
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const add = async () => {
    if (!name.trim() || saving) return
    setSaving(true)
    try {
      await createAlertRule({
        name: name.trim(),
        metric,
        comparator,
        threshold: Number(threshold) || 0,
        severity,
        scope_project: scopeProject.trim() || undefined,
        scope_tag: scopeTag.trim() || undefined,
        cooldown_minutes: Number(cooldown) || 30,
      })
      toast.success('Alert rule created')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const addChannel = async () => {
    if (!chName.trim() || !chTarget.trim() || saving) return
    setSaving(true)
    try {
      const ch = await createNotificationChannel({ name: chName.trim(), kind: chKind, target: chTarget.trim() })
      toast.success(`Channel '${ch.name}' added`)
      setChName('')
      setChTarget('')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const testChannel = async (id: string) => {
    try {
      await testNotificationChannel(id)
      toast.success('Test message sent')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const remove = async (id: string) => {
    try {
      await deleteAlertRule(id)
      toast.success('Alert rule deleted')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const toggleEnabled = async (r: AlertRule) => {
    try {
      await setAlertRuleEnabled(r.id, !r.enabled)
      toast.success(r.enabled ? `Disabled '${r.name}'` : `Enabled '${r.name}'`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Alert rules"
      subtitle="Alerts on hosts, storage, backups, failed tasks and VM CPU / memory — and where they are sent."
      icon={<Gauge className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      loading={loading && rows.length === 0}
      contentClassName="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4"
    >
      <OperatingSurfaceLayout testId="platform-alert-rules-page">
        <MacGlassPanel
          title="Where alerts go"
          subtitle={channels.length === 0 ? 'No channel yet: alerts only show in the app.' : `${channels.length} channel(s)`}
        >
          <div data-testid="alert-channels">
            {channels.length > 0 && (
              <ul className="divide-y divide-white/[0.04] -mx-1 mb-3">
                {channels.map((c) => (
                  <MacListRow
                    key={c.id}
                    title={c.name}
                    subtitle={`${c.kind} · ${channelTargetLabel(c.kind, c.target)}${c.enabled ? '' : ' · disabled'}`}
                    badge={<button type="button" className="btn-secondary text-xs px-2 py-1" onClick={() => void testChannel(c.id)}>Send test</button>}
                  />
                ))}
              </ul>
            )}
            <div className="grid gap-3 md:grid-cols-3 max-w-2xl">
              <input className="input text-sm" aria-label="Channel name" placeholder="Name (e.g. ops-slack)" value={chName} onChange={(e) => setChName(e.target.value)} />
              <select className="input text-sm" aria-label="Channel type" value={chKind} onChange={(e) => setChKind(e.target.value)}>
                <option value="slack">Slack</option>
                <option value="email">Email</option>
                <option value="webhook">Webhook</option>
              </select>
              <input className="input text-sm" aria-label="Channel target" placeholder={chKind === 'email' ? 'you@example.com' : 'https://hooks.example.com/…'} value={chTarget} onChange={(e) => setChTarget(e.target.value)} />
            </div>
            <button type="button" className="btn-primary text-sm mt-3 flex items-center gap-1.5" disabled={saving || !chName.trim() || !chTarget.trim()} onClick={() => void addChannel()}>
              <Plus className="w-4 h-4" /> Add channel
            </button>
          </div>
        </MacGlassPanel>

        <MacGlassPanel title="New alert rule">
          <div className="grid gap-3 md:grid-cols-2 max-w-2xl">
            <input className="input text-sm" aria-label="Rule name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
            <select className="input text-sm" aria-label="Metric" value={metric} onChange={(e) => setMetric(e.target.value as AlertMetric)}>
              <option value="cpu_percent">cpu_percent</option>
              <option value="mem_percent">mem_percent</option>
              <option value="host_offline">host_offline (fleet)</option>
              <option value="storage_pool_percent">storage_pool_percent (fleet)</option>
              <option value="backup_failed_24h">backup_failed_24h (fleet)</option>
              <option value="failed_task_burst">failed_task_burst (fleet)</option>
            </select>
            <select className="input text-sm" aria-label="Comparator" value={comparator} onChange={(e) => setComparator(e.target.value as AlertComparator)}>
              <option value="gt">greater than (gt)</option>
              <option value="lt">less than (lt)</option>
            </select>
            <input className="input text-sm" aria-label="Threshold" type="number" min={0} placeholder="Threshold (% or count)" value={threshold} onChange={(e) => setThreshold(e.target.value)} />
            <select className="input text-sm" aria-label="Severity" value={severity} onChange={(e) => setSeverity(e.target.value as AlertSeverity)}>
              <option value="info">info</option>
              <option value="warning">warning</option>
              <option value="critical">critical</option>
            </select>
            <input className="input text-sm" aria-label="Cooldown minutes" type="number" min={0} placeholder="Cooldown (minutes)" value={cooldown} onChange={(e) => setCooldown(e.target.value)} />
            <input className="input text-sm" aria-label="Scope project" placeholder="Scope project (optional)" value={scopeProject} onChange={(e) => setScopeProject(e.target.value)} />
            <input className="input text-sm" aria-label="Scope tag" placeholder="Scope tag (optional)" value={scopeTag} onChange={(e) => setScopeTag(e.target.value)} />
          </div>
          <button type="button" className="btn-primary text-sm mt-3 flex items-center gap-1.5" disabled={saving || !name.trim()} onClick={() => void add()}>
            <Plus className="w-4 h-4" /> {saving ? 'Saving…' : 'Add rule'}
          </button>
        </MacGlassPanel>

        <MacGlassPanel title="Active rules" subtitle={loading ? 'Loading…' : `${rows.length} rule(s)`}>
          {rows.length === 0 && !loading ? (
            <PlatformEmptyState
              icon={Gauge}
              title="No alert rules yet"
              subtitle="Add a threshold rule to be notified when VM CPU or memory crosses a bound."
            />
          ) : (
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {rows.map((r) => (
                <MacListRow
                  key={r.id}
                  title={r.name}
                  subtitle={`${r.metric} ${r.comparator} ${r.threshold} · ${r.severity} · cooldown ${r.cooldown_minutes}m · ${
                    r.scope_project || r.scope_tag ? `scope ${r.scope_project || '*'}/${r.scope_tag || '*'}` : 'all VMs'
                  }${r.enabled ? '' : ' · disabled'} · last fired ${r.last_fired_at ? new Date(r.last_fired_at).toLocaleString() : 'never'}`}
                  badge={
                    <div className="flex items-center gap-1.5">
                      <button type="button" className="btn-secondary text-xs px-2 py-1" onClick={() => void toggleEnabled(r)}>
                        {r.enabled ? 'Disable' : 'Enable'}
                      </button>
                      <button type="button" className="btn-secondary text-xs p-1.5" aria-label="Delete" onClick={() => setConfirmDeleteId(r.id)}>
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  }
                />
              ))}
            </ul>
          )}
        </MacGlassPanel>
      </OperatingSurfaceLayout>
      <ConfirmDialog
        open={confirmDeleteId !== null}
        title="Delete Alert Rule"
        message={`Delete alert rule "${rows.find((r) => r.id === confirmDeleteId)?.name}"? It will stop firing notifications.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDeleteId(null)}
        onConfirm={async () => {
          await remove(confirmDeleteId!)
          setConfirmDeleteId(null)
        }}
      />
    </PlatformPageChrome>
  )
}

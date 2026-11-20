// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback } from 'react'
import ConfirmDialog from '../components/ConfirmDialog'
import { Link } from 'react-router'
import {
  listRoles, setRole, listTokens, createToken, deleteToken,
  listAlertRules, saveAlertRules, listAlerts, acknowledgeAlert,
  listWebhooks, saveWebhooks, listSchedules, saveSchedules,
  listNotificationChannels, saveNotificationChannels, testNotification,
  listSnapshotSchedules, saveSnapshotSchedules,
  UserRole, ApiToken, AlertRule, Alert, WebhookConfig, ScheduledAction,
  NotificationChannel, SnapshotSchedule,
} from '../api/automation'
import { getOsUserCapability, createOsUser, deleteOsUser, OsUserCapability } from '../api/system'
import { getIntegrationsStatus, type IntegrationsStatus } from '../api/integrations'
import { usePlatformInfo } from '../contexts/PlatformInfoContext'
import { usePlatformTabState } from '../hooks/usePlatformTabState'
import { listVMs, VmInfo } from '../api/vm'
import { useToastContext } from '../contexts/ToastContext'
import {
  Settings, Users, Key, Bell, Webhook, Clock, Plus, Trash2, RefreshCw,
  Check, X, Shield, AlertCircle, Eye, Send, Camera, MessageSquare, Activity,
} from 'lucide-react'
import { ChoiceCard, ChoiceCardDenseGrid } from '../components/ChoiceCards'
import PageLayout from '../components/PageLayout'
import { formatUserError } from '../utils/apiError'
import { notificationChannelTone, statusActionLinkClasses, statusBadgeClasses, statusSurfaceClasses, statusToneClass, userRoleTone } from '../utils/semanticColors'
import {
  getObservabilitySettings,
  putObservabilitySettings,
  verifyAuditLog,
  type ObservabilitySettingsView,
} from '../api/observability'
import IdentitySsoPanel from '../components/IdentitySsoPanel'
import { getMetricsTraces, type HttpTraceSpan } from '../api/metrics'
const SETTINGS_TABS = ['roles', 'tokens', 'alerts', 'webhooks', 'schedules', 'notifications', 'snapshots'] as const
type Tab = (typeof SETTINGS_TABS)[number]

export default function SettingsPage() {
  const [tab, setTab] = usePlatformTabState(SETTINGS_TABS, { defaultTab: 'roles' })
  const [loading, setLoading] = useState(true)
  const toast = useToastContext()
  const { info } = usePlatformInfo()

  // Data
  const [roles, setRoles] = useState<UserRole[]>([])
  const [tokens, setTokens] = useState<ApiToken[]>([])
  const [alertRules, setAlertRules] = useState<AlertRule[]>([])
  const [alerts, setAlerts] = useState<Alert[]>([])
  const [webhooks, setWebhooks] = useState<WebhookConfig[]>([])
  const [schedules, setSchedules] = useState<ScheduledAction[]>([])
  const [notificationChannels, setNotificationChannels] = useState<NotificationChannel[]>([])
  const [snapshotSchedules, setSnapshotSchedules] = useState<SnapshotSchedule[]>([])
  const [vms, setVMs] = useState<VmInfo[]>([])

  // Forms
  const [newRoleUser, setNewRoleUser] = useState('')
  const [newRoleVal, setNewRoleVal] = useState('operator')
  const [newTokenName, setNewTokenName] = useState('')
  const [newTokenUser, setNewTokenUser] = useState('')
  const [newTokenRole, setNewTokenRole] = useState('readonly')
  const [creatingToken, setCreatingToken] = useState(false)
  const [createdToken, setCreatedToken] = useState('')
  const [newWebhookUrl, setNewWebhookUrl] = useState('')
  const [newSchedVm, setNewSchedVm] = useState('')
  const [newSchedAction, setNewSchedAction] = useState('shutdown')
  const [newSchedTime, setNewSchedTime] = useState('22:00')
  const [newNotifType, setNewNotifType] = useState('slack')
  const [newNotifConfig, setNewNotifConfig] = useState('')
  const [newSnapVm, setNewSnapVm] = useState('')
  const [newSnapInterval, setNewSnapInterval] = useState('24')
  const [newSnapRetain, setNewSnapRetain] = useState('5')

  const [osUserCap, setOsUserCap] = useState<OsUserCapability | null>(null)
  const [newOsUsername, setNewOsUsername] = useState('')
  const [newOsPassword, setNewOsPassword] = useState('')
  const [creatingOsUser, setCreatingOsUser] = useState(false)
  const [deleteOsUsername, setDeleteOsUsername] = useState('')
  const [confirmDeleteOsUser, setConfirmDeleteOsUser] = useState(false)
  const [addOsUserToLibvirt, setAddOsUserToLibvirt] = useState(true)
  const [integrations, setIntegrations] = useState<IntegrationsStatus | null>(null)
  const [obsSettings, setObsSettings] = useState<ObservabilitySettingsView | null>(null)
  const [obsSaving, setObsSaving] = useState(false)
  const [auditVerifyBusy, setAuditVerifyBusy] = useState(false)
  const [auditVerifyResult, setAuditVerifyResult] = useState<string | null>(null)
  const [otlpAuthInput, setOtlpAuthInput] = useState('')
  const [remoteWriteAuthInput, setRemoteWriteAuthInput] = useState('')
  const [httpTraces, setHttpTraces] = useState<HttpTraceSpan[] | null>(null)
  const [tracesLoading, setTracesLoading] = useState(false)

  const load = useCallback(async () => {
    const results = await Promise.allSettled([
      listRoles(), listTokens(), listAlertRules(), listAlerts(),
      listWebhooks(), listSchedules(), listVMs(),
      listNotificationChannels(), listSnapshotSchedules(),
      getOsUserCapability(),
      getObservabilitySettings(),
      getIntegrationsStatus(),
    ])
    if (results[0].status === 'fulfilled') setRoles(results[0].value)
    if (results[1].status === 'fulfilled') setTokens(results[1].value)
    if (results[2].status === 'fulfilled') setAlertRules(results[2].value)
    if (results[3].status === 'fulfilled') setAlerts(results[3].value)
    if (results[4].status === 'fulfilled') setWebhooks(results[4].value)
    if (results[5].status === 'fulfilled') setSchedules(results[5].value)
    if (results[6].status === 'fulfilled') setVMs(results[6].value)
    if (results[7].status === 'fulfilled') setNotificationChannels(results[7].value)
    if (results[8].status === 'fulfilled') setSnapshotSchedules(results[8].value)
    if (results[9].status === 'fulfilled') setOsUserCap(results[9].value)
    else setOsUserCap(null)
    if (results[10].status === 'fulfilled') setObsSettings(results[10].value)
    else setObsSettings(null)
    if (results[11].status === 'fulfilled') setIntegrations(results[11].value)
    else setIntegrations(null)
    setLoading(false)
  }, [])

  useEffect(() => { load() }, [load])

  useEffect(() => {
    if (osUserCap?.libvirtGroupAvailable === false) {
      setAddOsUserToLibvirt(false)
    }
  }, [osUserCap?.libvirtGroupAvailable])

  const tabs: { key: Tab; label: string; icon: React.ReactNode }[] = [
    { key: 'roles', label: 'Users & Roles', icon: <Users className="w-4 h-4" /> },
    { key: 'tokens', label: 'API Tokens', icon: <Key className="w-4 h-4" /> },
    { key: 'alerts', label: `Alerts (${alerts.filter(a => !a.acknowledged).length})`, icon: <Bell className="w-4 h-4" /> },
    { key: 'webhooks', label: 'Webhooks', icon: <Webhook className="w-4 h-4" /> },
    { key: 'schedules', label: 'Schedules', icon: <Clock className="w-4 h-4" /> },
    { key: 'notifications', label: 'Notifications', icon: <MessageSquare className="w-4 h-4" /> },
    { key: 'snapshots', label: 'Snapshot Schedules', icon: <Camera className="w-4 h-4" /> },
  ]

  return (
    <PageLayout
      eyebrow="System"
      className="w-full min-w-0 max-w-full"
      title="Settings"
      subtitle="RBAC, tokens, alerts, and schedules for the hypervisor control plane on this host."
      icon={<Settings className={`w-6 h-6 ${statusToneClass('info')}`} />}
      actions={
        <button type="button" onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" aria-label="Refresh"><RefreshCw className="w-4 h-4" aria-hidden="true" /></button>
      }
      contentLoading={loading}
    >
      <p className="text-xs text-[var(--text-muted)] break-words">
        Libvirt secrets (Ceph, iSCSI, TLS, …) are managed on the{' '}
        <Link to="/secrets" className={`underline ${statusActionLinkClasses('info')}`}>Secrets</Link> page (define XML + optional base64 value).
      </p>

      {integrations && (
        <section id="integrations-status" className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)] p-4 space-y-3 scroll-mt-24">
          <h2 className="text-sm font-semibold text-[var(--text-secondary)] flex items-center gap-2">
            <Activity className={`w-4 h-4 ${statusToneClass('ok')}`} />
            Integrations
          </h2>
          <p className="text-xs text-[var(--text-muted)]">
            Capability phases mirror the Platform Integrations hub — wire each backend before using operator UIs.
          </p>
          <div className="flex flex-wrap gap-2 text-xs">
            {info?.kubevirt?.exec_enabled && (
              <span className="px-2 py-1 rounded border border-[var(--accent)]/40 text-[var(--link)]">Kubernetes · exec enabled</span>
            )}
            {info?.guestkit?.enabled && (
              <span className="px-2 py-1 rounded border border-orange-500/40 text-orange-600">GuestKit · enabled</span>
            )}
            {info?.control_plane?.proxy_url && (
              <Link to="/platform/settings?section=integrations" className="px-2 py-1 rounded border border-[var(--accent)]/40 text-[var(--link)] hover:bg-[var(--accent-soft)]">
                Settings → Apps & Integrations →
              </Link>
            )}
          </div>
          <dl className="grid grid-cols-2 sm:grid-cols-3 gap-2 text-sm">
            <div>
              <dt className="text-[var(--text-muted)] text-xs">Automation worker</dt>
              <dd>
                {integrations.automation?.last_tick_unix
                  ? `tick ${new Date(integrations.automation?.last_tick_unix * 1000).toLocaleString()}`
                  : 'no tick yet'}
              </dd>
            </div>
            <div>
              <dt className="text-[var(--text-muted)] text-xs">Alerts (unacked)</dt>
              <dd>{integrations.automation?.alerts_unacknowledged ?? '—'}</dd>
            </div>
            <div>
              <dt className="text-[var(--text-muted)] text-xs">Alert rules</dt>
              <dd>
                {integrations.automation?.alert_rules_enabled ?? '—'}/{integrations.automation?.alert_rules_total ?? '—'} enabled
              </dd>
            </div>
            <div>
              <dt className="text-[var(--text-muted)] text-xs">KubeVirt exec</dt>
              <dd>{integrations.kubevirt.exec_enabled ? 'yes' : 'no'}</dd>
            </div>
            <div>
              <dt className="text-[var(--text-muted)] text-xs">K8s kubeconfig</dt>
              <dd className="font-mono text-xs break-all">
                {integrations.k8s.kubeconfig_auto_selected ?? 'default'}
              </dd>
            </div>
            <div>
              <dt className="text-[var(--text-muted)] text-xs">Run-as-user</dt>
              <dd>
                {integrations.run_as_user.impersonation_active
                  ? String(integrations.run_as_user.mode)
                  : 'off'}
              </dd>
            </div>
          </dl>
          <div className="flex flex-wrap gap-2 text-sm">
            <button
              type="button"
              className="px-3 py-2 rounded-lg border border-[var(--apple-hairline)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
              onClick={() => {
                void navigator.clipboard.writeText(JSON.stringify(integrations, null, 2))
                toast.success('Copied integrations JSON')
              }}
            >
              Copy JSON
            </button>
            <Link to="/k8s" className="px-3 py-2 rounded-lg border border-[var(--apple-hairline)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]">
              Kubernetes
            </Link>
            <Link to="/fleet-cloud/instances" className="px-3 py-2 rounded-lg border border-[var(--apple-hairline)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]">
              Fleet Cloud
            </Link>
          </div>
        </section>
      )}

      <IdentitySsoPanel />

      {obsSettings ? (
        <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)] p-4 space-y-4">
          <h2 className="text-sm font-semibold text-[var(--text-secondary)] flex items-center gap-2">
            <Activity className="w-4 h-4 text-[var(--accent)]" />
            Observability
          </h2>
          <p className="text-xs text-[var(--text-muted)]">
            Writes <code className="text-[var(--text-muted)]">{obsSettings.config_path}</code>. Workers reload
            automatically after save. Admin role required.
          </p>
          <div className="grid sm:grid-cols-2 gap-3 text-sm">
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={obsSettings.otlp.enabled}
                onChange={(e) =>
                  setObsSettings({
                    ...obsSettings,
                    otlp: { ...obsSettings.otlp, enabled: e.target.checked },
                  })
                }
              />
              OTLP export enabled
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={obsSettings.audit.sign_lines}
                onChange={(e) =>
                  setObsSettings({
                    ...obsSettings,
                    audit: { sign_lines: e.target.checked },
                  })
                }
              />
              Sign audit log lines (sha256)
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={obsSettings.otlp.export_metrics}
                onChange={(e) =>
                  setObsSettings({
                    ...obsSettings,
                    otlp: { ...obsSettings.otlp, export_metrics: e.target.checked },
                  })
                }
              />
              OTLP export metrics
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={obsSettings.otlp.export_logs}
                onChange={(e) =>
                  setObsSettings({
                    ...obsSettings,
                    otlp: { ...obsSettings.otlp, export_logs: e.target.checked },
                  })
                }
              />
              OTLP export logs
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={obsSettings.otlp.export_traces}
                onChange={(e) =>
                  setObsSettings({
                    ...obsSettings,
                    otlp: { ...obsSettings.otlp, export_traces: e.target.checked },
                  })
                }
              />
              OTLP export traces
            </label>
          </div>
          <div className="grid sm:grid-cols-2 gap-3">
            <div>
              <label className="text-xs text-[var(--text-muted)]">OTLP interval (seconds)</label>
              <input
                aria-label="OTLP interval in seconds"
                type="number"
                min={30}
                className="input-field w-full mt-1"
                value={obsSettings.otlp.interval_secs}
                onChange={(e) =>
                  setObsSettings({
                    ...obsSettings,
                    otlp: {
                      ...obsSettings.otlp,
                      interval_secs: Math.max(30, Number(e.target.value) || 30),
                    },
                  })
                }
              />
            </div>
            <div>
              <label className="text-xs text-[var(--text-muted)]">OTLP endpoint</label>
              <input
                aria-label="OTLP endpoint"
                className="input-field w-full mt-1"
                value={obsSettings.otlp.endpoint}
                onChange={(e) =>
                  setObsSettings({
                    ...obsSettings,
                    otlp: { ...obsSettings.otlp, endpoint: e.target.value },
                  })
                }
                placeholder="http://127.0.0.1:4318"
              />
            </div>
            <div>
              <label className="text-xs text-[var(--text-muted)]">
                OTLP authorization {obsSettings.otlp.authorization_set ? `(set: ${obsSettings.otlp.authorization})` : ''}
              </label>
              <input
                aria-label="OTLP authorization"
                className="input-field w-full mt-1"
                type="password"
                autoComplete="off"
                value={otlpAuthInput}
                placeholder={obsSettings.otlp.authorization_set ? 'Leave blank to keep' : 'Bearer …'}
                onChange={(e) => setOtlpAuthInput(e.target.value)}
              />
            </div>
            <div>
              <label className="text-xs text-[var(--text-muted)]">Metrics JSON remote_write URL</label>
              <input
                aria-label="Metrics remote_write URL"
                className="input-field w-full mt-1"
                value={obsSettings.metrics_history.remote_write_url}
                onChange={(e) =>
                  setObsSettings({
                    ...obsSettings,
                    metrics_history: {
                      ...obsSettings.metrics_history,
                      remote_write_url: e.target.value,
                    },
                  })
                }
              />
            </div>
            <div>
              <label className="text-xs text-[var(--text-muted)]">
                Remote_write auth{' '}
                {obsSettings.metrics_history.remote_write_authorization_set
                  ? `(set: ${obsSettings.metrics_history.remote_write_authorization})`
                  : ''}
              </label>
              <input
                aria-label="Remote write authorization"
                className="input-field w-full mt-1"
                type="password"
                autoComplete="off"
                value={remoteWriteAuthInput}
                placeholder="Bearer …"
                onChange={(e) => setRemoteWriteAuthInput(e.target.value)}
              />
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              disabled={obsSaving}
              className="btn-primary text-sm disabled:opacity-50"
              onClick={async () => {
                if (!obsSettings) return
                setObsSaving(true)
                try {
                  const patch: Record<string, unknown> = {
                    otlp_enabled: obsSettings.otlp.enabled,
                    otlp_endpoint: obsSettings.otlp.endpoint,
                    otlp_interval_secs: obsSettings.otlp.interval_secs,
                    otlp_export_metrics: obsSettings.otlp.export_metrics,
                    otlp_export_logs: obsSettings.otlp.export_logs,
                    otlp_export_traces: obsSettings.otlp.export_traces,
                    audit_sign_lines: obsSettings.audit.sign_lines,
                    metrics_history_remote_write_url: obsSettings.metrics_history.remote_write_url,
                  }
                  if (otlpAuthInput) {
                    patch.otlp_authorization = otlpAuthInput
                  }
                  if (remoteWriteAuthInput) {
                    patch.metrics_history_remote_write_authorization = remoteWriteAuthInput
                  }
                  const res = await putObservabilitySettings(patch)
                  setObsSettings(res.settings)
                  setOtlpAuthInput('')
                  setRemoteWriteAuthInput('')
                  toast.success(
                    res.workers_reloaded ? 'Saved — observability workers reloaded' : 'Saved',
                  )
                } catch (e: unknown) {
                  toast.error(formatUserError(e))
                } finally {
                  setObsSaving(false)
                }
              }}
            >
              {obsSaving ? 'Saving…' : 'Save observability settings'}
            </button>
            <button
              type="button"
              disabled={auditVerifyBusy}
              className="px-3 py-2 rounded-lg border border-[var(--apple-hairline)] text-sm text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] disabled:opacity-50"
              onClick={async () => {
                setAuditVerifyBusy(true)
                setAuditVerifyResult(null)
                try {
                  const r = await verifyAuditLog()
                  setAuditVerifyResult(
                    `${r.signed_valid} valid signed, ${r.signed_invalid} invalid, ${r.unsigned} unsigned (${r.total_lines} lines)`,
                  )
                } catch (e: unknown) {
                  toast.error(formatUserError(e))
                } finally {
                  setAuditVerifyBusy(false)
                }
              }}
            >
              {auditVerifyBusy ? 'Verifying…' : 'Verify audit log'}
            </button>
          </div>
          {auditVerifyResult ? (
            <p className="text-xs text-[var(--text-muted)] font-mono">{auditVerifyResult}</p>
          ) : null}
          <div className="border-t border-[var(--apple-hairline)] pt-3 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-xs text-[var(--text-muted)]">Recent HTTP traces</span>
              <button
                type="button"
                disabled={tracesLoading}
                className="px-2 py-1 rounded border border-[var(--apple-hairline)] text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] disabled:opacity-50"
                onClick={async () => {
                  setTracesLoading(true)
                  try {
                    const r = await getMetricsTraces(24)
                    setHttpTraces(r.traces)
                  } catch (e: unknown) {
                    toast.error(formatUserError(e))
                    setHttpTraces(null)
                  } finally {
                    setTracesLoading(false)
                  }
                }}
              >
                {tracesLoading ? 'Loading…' : 'Load traces'}
              </button>
            </div>
            {httpTraces && httpTraces.length > 0 ? (
              <ul className="text-xs font-mono text-[var(--text-muted)] space-y-1 max-h-40 overflow-y-auto">
                {httpTraces.map((t, i) => (
                  <li key={`${t.trace_id}-${i}`}>
                    {t.method} {t.route} → {t.status} ({t.duration_ms}ms){' '}
                    <span className="text-[var(--text-muted)]">{t.trace_id.slice(0, 16)}…</span>
                  </li>
                ))}
              </ul>
            ) : httpTraces ? (
              <p className="text-xs text-[var(--text-muted)]">No traces in buffer yet.</p>
            ) : null}
          </div>
        </section>
      ) : null}

      <div>
        <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">Section</h2>
        <ChoiceCardDenseGrid>
          {tabs.map((t) => (
            <ChoiceCard
              key={t.key}
              compact
              tone="blue"
              selected={tab === t.key}
              onClick={() => setTab(t.key)}
              icon={t.icon}
              title={t.label}
            />
          ))}
        </ChoiceCardDenseGrid>
      </div>

      {/* ── Roles ──────────────────────────────────────────── */}
      {tab === 'roles' && (
        <div className="space-y-4">
          <div className="flex flex-col gap-2 min-w-0 sm:flex-row sm:flex-wrap sm:items-center">
            <input value={newRoleUser} onChange={e => setNewRoleUser(e.target.value)} aria-label="Username" className="input-field flex-1 min-w-0 sm:min-w-[12rem]" placeholder="Username" />
            <select value={newRoleVal} onChange={e => setNewRoleVal(e.target.value)} aria-label="Role" className="input-field w-full sm:w-40 shrink-0">
              <option value="admin">Admin</option>
              <option value="operator">Operator</option>
              <option value="readonly">Read-only</option>
            </select>
            <button type="button" onClick={async () => { if (!newRoleUser) return; try { await setRole(newRoleUser, newRoleVal); toast.success('Role set'); setNewRoleUser(''); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) } }} aria-label="Set role" title="Set role" className="btn-primary text-sm shrink-0"><Plus className="w-4 h-4" aria-hidden="true" /></button>
          </div>
          <div className="card overflow-x-auto max-w-full">
            <table className="w-full min-w-[28rem]" aria-label="User roles">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">User</th><th scope="col" className="px-6 py-3">Role</th><th scope="col" className="px-6 py-3">Permissions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {roles.map(r => (
                  <tr key={r.username} className="table-row-hover">
                    <td className="px-6 py-3 font-medium">{r.username}</td>
                    <td className="px-6 py-3"><span className={`px-2 py-0.5 rounded text-xs font-medium ${statusBadgeClasses(userRoleTone(r.role))}`}>{r.role}</span></td>
                    <td className="px-6 py-3 text-xs text-[var(--text-muted)]">{r.role === 'admin' ? 'Full access' : r.role === 'operator' ? 'Create/modify VMs' : 'View only'}</td>
                  </tr>
                ))}
                {roles.length === 0 && <tr><td colSpan={3} className="px-6 py-8 text-center text-[var(--text-muted)]">No custom roles set. All users default to admin.</td></tr>}
              </tbody>
            </table>
          </div>

          {osUserCap && (
            <div className="card p-5 space-y-3">
              <h3 className="text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2"><Shield className={`w-4 h-4 ${statusToneClass('info')}`} /> System users (PAM / UNIX)</h3>
              <p className="text-xs text-[var(--text-muted)] break-words hyphens-auto">
                Adds or removes a UNIX account on the machina host. When <strong className="text-[var(--text-muted)]">systemd-homed</strong> is active and <code className="bg-[var(--apple-surface)] px-1 rounded break-all">homectl</code> is available, new users are created with <code className="bg-[var(--apple-surface)] px-1 rounded break-all">homectl create</code> (directory storage, <strong className="text-[var(--text-muted)]">wheel</strong>/<strong className="text-[var(--text-muted)]">sudo</strong> membership); otherwise <code className="bg-[var(--apple-surface)] px-1 rounded break-all">useradd</code> / <code className="bg-[var(--apple-surface)] px-1 rounded break-all">usermod</code>. Password is set with <code className="bg-[var(--apple-surface)] px-1 rounded break-all">chpasswd</code>. Optionally append the <strong className="text-[var(--text-muted)]">libvirt</strong> group so the account can use <code className="bg-[var(--apple-surface)] px-1 rounded break-all">qemu:///system</code> after next login (or <code className="bg-[var(--apple-surface)] px-1 rounded break-all">newgrp libvirt</code>). Removal uses <code className="bg-[var(--apple-surface)] px-1 rounded break-all">homectl remove</code> for homed-managed users, else <code className="bg-[var(--apple-surface)] px-1 rounded break-all">userdel -r</code>. The signed-in user must be in <strong className="text-[var(--text-muted)]">wheel</strong>, <strong className="text-[var(--text-muted)]">sudo</strong>, or <strong className="text-[var(--text-muted)]">admin</strong>. Not available when using an API token.
              </p>
              {osUserCap.userAccountBackend && (
                <p className="text-xs text-[var(--text-muted)]">
                  Host account backend: <span className="font-mono text-[var(--text-secondary)]">{osUserCap.userAccountBackend}</span>
                  {osUserCap.sudoSupplementaryGroup != null && osUserCap.sudoSupplementaryGroup !== '' && (
                    <span className="text-[var(--text-muted)]"> — sudo group: <span className="font-mono text-[var(--text-secondary)]">{osUserCap.sudoSupplementaryGroup}</span></span>
                  )}
                </p>
              )}
              {osUserCap.libvirtGroupAvailable === false && (
                <p className={`text-xs opacity-90 ${statusToneClass('warn')}`}>Host has no <code className="bg-[var(--apple-surface)] px-1 rounded">libvirt</code> UNIX group — install libvirt or create the group before enabling libvirt access for new users.</p>
              )}
              {(osUserCap.canDeleteOsUsers ?? osUserCap.canCreateOsUsers) ? (
                <div className="space-y-3">
                  {osUserCap.canCreateOsUsers && (
                    <>
                      <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)] cursor-pointer select-none">
                        <input
                          type="checkbox"
                          className="rounded border-[var(--apple-hairline)]"
                          checked={addOsUserToLibvirt}
                          disabled={osUserCap.libvirtGroupAvailable === false}
                          onChange={e => setAddOsUserToLibvirt(e.target.checked)}
                        />
                        Add to <code className="text-xs bg-[var(--apple-surface)] px-1 rounded">{osUserCap.libvirtGroupName ?? 'libvirt'}</code> group (libvirt / qemu system URI)
                      </label>
                      <div className="flex flex-col sm:flex-row gap-2">
                        <input value={newOsUsername} onChange={e => setNewOsUsername(e.target.value)} aria-label="New username" className="input-field flex-1" placeholder="New username" autoComplete="off" />
                        <input value={newOsPassword} onChange={e => setNewOsPassword(e.target.value)} type="password" aria-label="Initial password" className="input-field flex-1" placeholder="Initial password" autoComplete="new-password" />
                        <button
                          type="button"
                          disabled={creatingOsUser || !newOsUsername.trim() || !newOsPassword}
                          onClick={async () => {
                            if (!newOsUsername.trim() || !newOsPassword) { toast.error('Username and password required'); return }
                            if (creatingOsUser) return
                            setCreatingOsUser(true)
                            try {
                              const r = await createOsUser(newOsUsername.trim(), newOsPassword, addOsUserToLibvirt)
                              const extra = r.libvirt_group_attached ? ' (added to libvirt group)' : ''
                              const be = r.account_backend ? ` [${r.account_backend}]` : ''
                              toast.success(`System user '${newOsUsername.trim()}' created${extra}${be}`)
                              setNewOsUsername('')
                              setNewOsPassword('')
                            } catch (e: unknown) {
                              toast.error(formatUserError(e))
                            } finally {
                              setCreatingOsUser(false)
                            }
                          }}
                          className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed whitespace-nowrap"
                        >
                          {creatingOsUser ? 'Creating…' : 'Create UNIX user'}
                        </button>
                      </div>
                    </>
                  )}
                  {(osUserCap.canDeleteOsUsers ?? osUserCap.canCreateOsUsers) && (
                    <div className="pt-3 border-t border-[var(--apple-hairline)]/40 space-y-2">
                      <p className="text-xs text-[var(--text-muted)]">Delete a UNIX account and remove its home directory. You cannot remove the account you are signed in as.</p>
                      <div className="flex flex-col sm:flex-row gap-2">
                        <input value={deleteOsUsername} onChange={e => setDeleteOsUsername(e.target.value)} aria-label="Username to remove" className="input-field flex-1" placeholder="Username to remove" autoComplete="off" />
                        <button
                          type="button"
                          onClick={() => {
                            const u = deleteOsUsername.trim()
                            if (!u) { toast.error('Username required'); return }
                            setConfirmDeleteOsUser(true)
                          }}
                          className="btn-destructive text-sm whitespace-nowrap"
                        >
                          Remove UNIX user
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              ) : (
                <p className={`text-xs opacity-90 ${statusToneClass('warn')}`}>{typeof osUserCap.reason === 'string' ? osUserCap.reason : 'You cannot create system users with the current sign-in method.'}</p>
              )}
            </div>
          )}
        </div>
      )}

      {/* ── Tokens ─────────────────────────────────────────── */}
      {tab === 'tokens' && (
        <div className="space-y-4">
          <div className="flex flex-col gap-2 min-w-0 sm:flex-row sm:flex-wrap sm:items-center">
            <input value={newTokenName} onChange={e => setNewTokenName(e.target.value)} aria-label="Token name" className="input-field flex-1 min-w-0" placeholder="Token name" />
            <input value={newTokenUser} onChange={e => setNewTokenUser(e.target.value)} aria-label="Token user" className="input-field w-full sm:w-32 shrink-0" placeholder="User" />
            <select value={newTokenRole} onChange={e => setNewTokenRole(e.target.value)} aria-label="Token role" className="input-field w-full sm:w-32 shrink-0">
              <option value="admin">Admin</option>
              <option value="operator">Operator</option>
              <option value="readonly">Read-only</option>
            </select>
            <button type="button" disabled={creatingToken || !newTokenName.trim() || !newTokenUser.trim()} onClick={async () => { if (!newTokenName.trim() || !newTokenUser.trim()) { toast.error('Token name and user are required'); return } if (creatingToken) return; setCreatingToken(true); try { const t = await createToken(newTokenName, newTokenUser, newTokenRole); setCreatedToken(t.token); toast.success('Token created'); setNewTokenName(''); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) } finally { setCreatingToken(false) } }} aria-label="Create token" title="Create token" className="btn-primary text-sm shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"><Plus className="w-4 h-4" aria-hidden="true" /></button>
          </div>
          {createdToken && (
            <div className={`p-3 rounded-lg border ${statusSurfaceClasses('ok')}`}>
              <span className={`text-xs ${statusToneClass('ok')}`}>New token (copy now, won't be shown again):</span>
              <div className={`font-mono text-sm mt-1 break-all ${statusToneClass('ok')} opacity-90`}>{createdToken}</div>
            </div>
          )}
          <div className="card overflow-x-auto max-w-full">
            <table className="w-full min-w-[36rem]" aria-label="API tokens">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">Name</th><th scope="col" className="px-6 py-3">Token</th><th scope="col" className="px-6 py-3">User</th><th scope="col" className="px-6 py-3">Role</th><th scope="col" className="px-6 py-3">Created</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {tokens.map(t => (
                  <tr key={t.name} className="table-row-hover">
                    <td className="px-6 py-3 font-medium">{t.name}</td>
                    <td className="px-6 py-3 font-mono text-xs text-[var(--text-muted)]">{t.token}</td>
                    <td className="px-6 py-3 text-sm">{t.username}</td>
                    <td className="px-6 py-3 text-xs"><span className="px-2 py-0.5 bg-[var(--surface-hover)] rounded">{t.role}</span></td>
                    <td className="px-6 py-3 text-xs text-[var(--text-muted)]">{t.created}</td>
                    <td className="px-6 py-3 text-right"><button onClick={async () => { try { await deleteToken(t.token); toast.success('Deleted'); load() } catch (e: unknown) { toast.error(formatUserError(e)) } }} aria-label="Delete token" title="Delete token" className="p-1 hover:bg-red-600/20 rounded"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} aria-hidden="true" /></button></td>
                  </tr>
                ))}
                {tokens.length === 0 && <tr><td colSpan={6} className="px-6 py-8 text-center text-[var(--text-muted)]">No API tokens. Create one to authenticate scripts and automation.</td></tr>}
              </tbody>
            </table>
          </div>
          <p className="text-xs text-[var(--text-muted)] break-words">Use tokens with: <code className="bg-[var(--apple-fill-tertiary)] px-1 rounded break-all">curl -k -H &quot;Authorization: Bearer mach_xxx...&quot; https://host:5092/api/v1/vms</code> (older installs may still have <code className="bg-[var(--apple-fill-tertiary)] px-1 rounded">vs_</code> tokens until rotated)</p>
        </div>
      )}

      {/* ── Alerts ─────────────────────────────────────────── */}
      {tab === 'alerts' && (
        <div className="space-y-4">
          <h3 className="text-sm font-semibold text-[var(--text-secondary)]">Alert Rules</h3>
          <div className="card overflow-x-auto max-w-full">
            <table className="w-full min-w-[32rem]" aria-label="Alert rules">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">Rule</th><th scope="col" className="px-6 py-3">Condition</th><th scope="col" className="px-6 py-3">Threshold</th><th scope="col" className="px-6 py-3">Enabled</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {alertRules.map((r, i) => (
                  <tr key={r.id} className="table-row-hover">
                    <td className="px-6 py-3 font-medium">{r.name}</td>
                    <td className="px-6 py-3 text-sm font-mono text-[var(--text-muted)]">{r.condition}</td>
                    <td className="px-6 py-3"><input aria-label="Alert rule threshold" type="number" value={r.threshold} onChange={e => { const next = [...alertRules]; next[i].threshold = parseFloat(e.target.value) || 0; setAlertRules(next) }} className="input-field w-20" /></td>
                    <td className="px-6 py-3"><input type="checkbox" checked={r.enabled} onChange={e => { const next = [...alertRules]; next[i].enabled = e.target.checked; setAlertRules(next) }} /></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <button onClick={async () => { try { await saveAlertRules(alertRules); toast.success('Rules saved') } catch (e: unknown) { toast.error(`${formatUserError(e)}`) } }} className="btn-primary text-sm">Save Rules</button>

          <h3 className="text-sm font-semibold text-[var(--text-secondary)] mt-6">Active Alerts</h3>
          <div className="space-y-2">
            {alerts.filter(a => !a.acknowledged).map(a => (
              <div key={a.id} className={`flex items-center gap-3 p-3 rounded-lg border ${statusSurfaceClasses(a.severity === 'critical' ? 'error' : 'warn')}`}>
                <AlertCircle className={`w-4 h-4 ${statusToneClass(a.severity === 'critical' ? 'error' : 'warn')}`} />
                <div className="flex-1">
                  <div className="text-sm font-medium">{a.rule_name}</div>
                  <div className="text-xs text-[var(--text-muted)]">{a.message} — {a.timestamp}</div>
                </div>
                <button onClick={async () => { try { await acknowledgeAlert(a.id); load() } catch (e: unknown) { toast.error(formatUserError(e)) } }} className="px-2 py-1 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] rounded text-xs"><Check className="w-3 h-3 inline" /> Ack</button>
              </div>
            ))}
            {alerts.filter(a => !a.acknowledged).length === 0 && <p className="text-sm text-[var(--text-muted)]">No active alerts</p>}
          </div>
        </div>
      )}

      {/* ── Webhooks ───────────────────────────────────────── */}
      {tab === 'webhooks' && (
        <div className="space-y-4">
          <div className="flex flex-col gap-2 min-w-0 sm:flex-row sm:items-center">
            <input aria-label="Webhook URL" value={newWebhookUrl} onChange={e => setNewWebhookUrl(e.target.value)} className="input-field flex-1 min-w-0" placeholder="https://example.com/webhook" />
            <button type="button" onClick={() => { if (!newWebhookUrl) return; const next = [...webhooks, { id: `wh-${Date.now()}`, url: newWebhookUrl, events: ['*'], enabled: true }]; setWebhooks(next); setNewWebhookUrl(''); saveWebhooks(next).then(() => toast.success('Webhook added')).catch((e: unknown) => toast.error(formatUserError(e))) }} aria-label="Add webhook" title="Add webhook" className="btn-primary text-sm shrink-0"><Plus className="w-4 h-4" aria-hidden="true" /></button>
          </div>
          <div className="card overflow-x-auto max-w-full">
            <table className="w-full min-w-[28rem]" aria-label="Webhook endpoints">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">URL</th><th scope="col" className="px-6 py-3">Events</th><th scope="col" className="px-6 py-3">Enabled</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {webhooks.map((h, i) => (
                  <tr key={h.id} className="table-row-hover">
                    <td className={`px-6 py-3 text-sm font-mono truncate max-w-xs ${statusToneClass('info')}`}>{h.url}</td>
                    <td className="px-6 py-3 text-xs text-[var(--text-muted)]">{h.events.join(', ')}</td>
                    <td className="px-6 py-3"><input type="checkbox" checked={h.enabled} onChange={e => { const next = [...webhooks]; next[i].enabled = e.target.checked; setWebhooks(next); saveWebhooks(next).catch((e: unknown) => toast.error(formatUserError(e))) }} /></td>
                    <td className="px-6 py-3 text-right"><button onClick={() => { const next = webhooks.filter((_, j) => j !== i); setWebhooks(next); saveWebhooks(next).catch((e: unknown) => toast.error(formatUserError(e))) }} aria-label="Remove webhook" title="Remove webhook" className="p-1 hover:bg-red-600/20 rounded"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} aria-hidden="true" /></button></td>
                  </tr>
                ))}
                {webhooks.length === 0 && <tr><td colSpan={4} className="px-6 py-8 text-center text-[var(--text-muted)]">No webhooks configured. Add one to receive VM event notifications.</td></tr>}
              </tbody>
            </table>
          </div>
          <p className="text-xs text-[var(--text-muted)]">Events: vm_started, vm_stopped, alert_fired, backup_completed, * (all)</p>
        </div>
      )}

      {/* ── Schedules ──────────────────────────────────────── */}
      {tab === 'schedules' && (
        <div className="space-y-4">
          <div className="flex flex-col gap-2 min-w-0 sm:flex-row sm:flex-wrap sm:items-center">
            <select value={newSchedVm} onChange={e => setNewSchedVm(e.target.value)} aria-label="VM" className="input-field flex-1 min-w-0">
              <option value="">Select VM...</option>
              {vms.map(v => <option key={v.name} value={v.name}>{v.name}</option>)}
            </select>
            <select value={newSchedAction} onChange={e => setNewSchedAction(e.target.value)} aria-label="Action" className="input-field w-full shrink-0 sm:w-32">
              <option value="start">Start</option>
              <option value="shutdown">Shutdown</option>
              <option value="stop">Force Stop</option>
              <option value="reboot">Reboot</option>
              <option value="snapshot">Snapshot</option>
            </select>
            <input type="time" aria-label="Schedule time" value={newSchedTime} onChange={e => setNewSchedTime(e.target.value)} className="input-field w-full shrink-0 sm:w-28" />
            <button type="button" onClick={() => { if (!newSchedVm) return; const next = [...schedules, { id: `sched-${Date.now()}`, vm_name: newSchedVm, action: newSchedAction, schedule: `daily ${newSchedTime}`, enabled: true, last_run: '' }]; setSchedules(next); saveSchedules(next).then(() => toast.success('Schedule added')).catch((e: unknown) => toast.error(formatUserError(e))) }} aria-label="Add schedule" title="Add schedule" className="btn-primary text-sm shrink-0"><Plus className="w-4 h-4" aria-hidden="true" /></button>
          </div>
          <div className="card overflow-x-auto max-w-full">
            <table className="w-full min-w-[40rem]" aria-label="VM schedules">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">VM</th><th scope="col" className="px-6 py-3">Action</th><th scope="col" className="px-6 py-3">Schedule</th><th scope="col" className="px-6 py-3">Enabled</th><th scope="col" className="px-6 py-3">Last Run</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {schedules.map((s, i) => (
                  <tr key={s.id} className="table-row-hover">
                    <td className="px-6 py-3 font-medium">{s.vm_name}</td>
                    <td className="px-6 py-3 text-sm"><span className="px-2 py-0.5 bg-[var(--surface-hover)] rounded text-xs">{s.action}</span></td>
                    <td className="px-6 py-3 text-sm font-mono text-[var(--text-muted)]">{s.schedule}</td>
                    <td className="px-6 py-3"><input type="checkbox" checked={s.enabled} onChange={e => { const next = [...schedules]; next[i].enabled = e.target.checked; setSchedules(next); saveSchedules(next).catch((e: unknown) => toast.error(formatUserError(e))) }} /></td>
                    <td className="px-6 py-3 text-xs text-[var(--text-muted)]">{s.last_run || 'never'}</td>
                    <td className="px-6 py-3 text-right"><button onClick={() => { const next = schedules.filter((_, j) => j !== i); setSchedules(next); saveSchedules(next).catch((e: unknown) => toast.error(formatUserError(e))) }} aria-label="Remove schedule" title="Remove schedule" className="p-1 hover:bg-red-600/20 rounded"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} aria-hidden="true" /></button></td>
                  </tr>
                ))}
                {schedules.length === 0 && <tr><td colSpan={6} className="px-6 py-8 text-center text-[var(--text-muted)]">No scheduled actions. Add one to auto start/stop VMs at specific times.</td></tr>}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* ── Notifications ─────────────────────────────────── */}
      {tab === 'notifications' && (
        <div className="space-y-4">
          <div className="flex items-center gap-2 flex-wrap">
            <select value={newNotifType} onChange={e => setNewNotifType(e.target.value)} aria-label="Channel type" className="input-field w-36">
              <option value="slack">Slack</option>
              <option value="email">Email</option>
              <option value="telegram">Telegram</option>
              <option value="webhook">Webhook</option>
            </select>
            <input aria-label="Channel configuration" value={newNotifConfig} onChange={e => setNewNotifConfig(e.target.value)} className="input-field flex-1" placeholder={newNotifType === 'slack' ? 'Slack webhook URL' : newNotifType === 'email' ? 'recipient@example.com' : newNotifType === 'telegram' ? 'bot_token:chat_id' : 'https://example.com/hook'} />
            <button onClick={() => { if (!newNotifConfig) return; const next = [...notificationChannels, { id: `notif-${Date.now()}`, channel_type: newNotifType, config: newNotifConfig, enabled: true }]; setNotificationChannels(next); setNewNotifConfig(''); saveNotificationChannels(next).then(() => toast.success('Channel added')).catch((e: unknown) => toast.error(formatUserError(e))) }} aria-label="Add channel" title="Add channel" className="btn-primary text-sm"><Plus className="w-4 h-4" aria-hidden="true" /></button>
          </div>
          <div className="card overflow-x-auto max-w-full">
            <table className="w-full min-w-[28rem]" aria-label="Notification channels">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">Type</th><th scope="col" className="px-6 py-3">Config</th><th scope="col" className="px-6 py-3">Enabled</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {notificationChannels.map((ch, i) => (
                  <tr key={ch.id} className="table-row-hover">
                    <td className="px-6 py-3"><span className={`px-2 py-0.5 rounded text-xs font-medium ${statusBadgeClasses(notificationChannelTone(ch.channel_type))}`}>{ch.channel_type}</span></td>
                    <td className="px-6 py-3 text-sm font-mono text-[var(--text-muted)] truncate max-w-xs">{ch.config}</td>
                    <td className="px-6 py-3"><input type="checkbox" checked={ch.enabled} onChange={e => { const next = [...notificationChannels]; next[i].enabled = e.target.checked; setNotificationChannels(next); saveNotificationChannels(next).catch((e: unknown) => toast.error(formatUserError(e))) }} /></td>
                    <td className="px-6 py-3 text-right flex items-center justify-end gap-1">
                      <button onClick={async () => { try { await testNotification(ch); toast.success('Test sent') } catch (e: unknown) { toast.error(`${formatUserError(e)}`) } }} className="p-1 hover:bg-white/10 rounded" title="Send test" aria-label="Send test"><Send className={`w-4 h-4 ${statusToneClass('info')}`} aria-hidden="true" /></button>
                      <button onClick={() => { const next = notificationChannels.filter((_, j) => j !== i); setNotificationChannels(next); saveNotificationChannels(next).catch((e: unknown) => toast.error(formatUserError(e))) }} aria-label="Remove channel" title="Remove channel" className="p-1 hover:bg-red-600/20 rounded"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} aria-hidden="true" /></button>
                    </td>
                  </tr>
                ))}
                {notificationChannels.length === 0 && <tr><td colSpan={4} className="px-6 py-8 text-center text-[var(--text-muted)]">No notification channels. Add Slack, Email, Telegram, or Webhook to receive alerts.</td></tr>}
              </tbody>
            </table>
          </div>
          <p className="text-xs text-[var(--text-muted)]">Slack: paste incoming webhook URL. Email: recipient address (requires sendmail). Telegram: bot_token:chat_id format.</p>
        </div>
      )}

      {/* ── Snapshot Schedules ─────────────────────────────── */}
      {tab === 'snapshots' && (
        <div className="space-y-4">
          <div className="flex items-center gap-2 flex-wrap">
            <select value={newSnapVm} onChange={e => setNewSnapVm(e.target.value)} aria-label="VM" className="input-field flex-1">
              <option value="">Select VM...</option>
              {vms.map(v => <option key={v.name} value={v.name}>{v.name}</option>)}
            </select>
            <select value={newSnapInterval} onChange={e => setNewSnapInterval(e.target.value)} aria-label="Interval" className="input-field w-28">
              <option value="1">Every 1h</option>
              <option value="4">Every 4h</option>
              <option value="12">Every 12h</option>
              <option value="24">Every 24h</option>
            </select>
            <input type="number" value={newSnapRetain} onChange={e => setNewSnapRetain(e.target.value)} aria-label="Retain count" className="input-field w-24" placeholder="Retain" min="1" max="100" />
            <button onClick={() => { if (!newSnapVm) return; const next = [...snapshotSchedules, { id: `snap-${Date.now()}`, vm_name: newSnapVm, interval_hours: parseInt(newSnapInterval) || 24, retain_count: parseInt(newSnapRetain) || 5, enabled: true, last_run: '' }]; setSnapshotSchedules(next); setNewSnapVm(''); saveSnapshotSchedules(next).then(() => toast.success('Snapshot schedule added')).catch((e: unknown) => toast.error(formatUserError(e))) }} aria-label="Add snapshot schedule" title="Add snapshot schedule" className="btn-primary text-sm"><Plus className="w-4 h-4" aria-hidden="true" /></button>
          </div>
          <div className="card overflow-x-auto max-w-full">
            <table className="w-full min-w-[36rem]" aria-label="Snapshot schedules">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">VM</th><th scope="col" className="px-6 py-3">Interval</th><th scope="col" className="px-6 py-3">Retain</th><th scope="col" className="px-6 py-3">Last Run</th><th scope="col" className="px-6 py-3">Enabled</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {snapshotSchedules.map((s, i) => (
                  <tr key={s.id} className="table-row-hover">
                    <td className="px-6 py-3 font-medium">{s.vm_name}</td>
                    <td className="px-6 py-3 text-sm">{s.interval_hours}h</td>
                    <td className="px-6 py-3 text-sm">{s.retain_count}</td>
                    <td className="px-6 py-3 text-xs text-[var(--text-muted)]">{s.last_run || 'never'}</td>
                    <td className="px-6 py-3"><input type="checkbox" checked={s.enabled} onChange={e => { const next = [...snapshotSchedules]; next[i].enabled = e.target.checked; setSnapshotSchedules(next); saveSnapshotSchedules(next).catch((e: unknown) => toast.error(formatUserError(e))) }} /></td>
                    <td className="px-6 py-3 text-right"><button onClick={() => { const next = snapshotSchedules.filter((_, j) => j !== i); setSnapshotSchedules(next); saveSnapshotSchedules(next).catch((e: unknown) => toast.error(formatUserError(e))) }} aria-label="Delete snapshot schedule" title="Delete snapshot schedule" className="p-1 hover:bg-red-600/20 rounded"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} aria-hidden="true" /></button></td>
                  </tr>
                ))}
                {snapshotSchedules.length === 0 && <tr><td colSpan={6} className="px-6 py-8 text-center text-[var(--text-muted)]">No snapshot schedules. Add one to automatically snapshot VMs at regular intervals.</td></tr>}
              </tbody>
            </table>
          </div>
          <p className="text-xs text-[var(--text-muted)]">Snapshots are taken automatically at the configured interval. Old snapshots beyond the retain count are pruned.</p>
        </div>
      )}
      <ConfirmDialog
        open={confirmDeleteOsUser}
        title="Delete UNIX user"
        message={`Permanently delete UNIX user "${deleteOsUsername.trim()}" and remove their home directory? This cannot be undone.`}
        confirmLabel="Delete user"
        variant="danger"
        onCancel={() => setConfirmDeleteOsUser(false)}
        onConfirm={async () => {
          setConfirmDeleteOsUser(false)
          const u = deleteOsUsername.trim()
          try {
            await deleteOsUser(u)
            toast.success(`System user '${u}' removed`)
            setDeleteOsUsername('')
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          }
        }}
      />
    </PageLayout>
  )
}

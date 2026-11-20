// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Settings } from 'lucide-react'
import { MacGlassPanel, MacSettingsGroup, MacSettingsGroupBody } from '../../components/platform/mac/PlatformMacUi'

function SettingsBlock({
  title,
  subtitle,
  embedded,
  children,
}: {
  title: string
  subtitle?: string
  embedded?: boolean
  children: React.ReactNode
}) {
  if (embedded) {
    return (
      <MacSettingsGroup title={title}>
        <MacSettingsGroupBody>
          {subtitle ? <p className="text-xs text-[var(--text-muted)] leading-relaxed">{subtitle}</p> : null}
          {children}
        </MacSettingsGroupBody>
      </MacSettingsGroup>
    )
  }
  return (
    <MacGlassPanel title={title} subtitle={subtitle}>
      {children}
    </MacGlassPanel>
  )
}
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import DatabaseStatus from '../../components/platform/DatabaseStatus'
import {
  getControllerBase,
  getDirectControllerBase,
  PLATFORM_CONTROLLER_PROXY,
  setControllerConfig,
  getClusterSummary,
  getClusterLeadership,
  getClusterSettings,
  patchCluster,
  patchClusterSettings,
  getOidcSettings,
  patchOidcSettings,
  getCpuCompatMatrix,
  patchCpuCompatMatrix,
  getOidcLoginUrl,
  listProjectQuotas,
  upsertProjectQuota,
  type CpuCompatRule,
  type OidcSettings,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { safeHref } from '../../utils/safeHref'
import { statusToneClass } from '../../utils/semanticColors'
import { getAiSettings, patchAiSettings, type AiSettings } from '../../api/ai'
import { useAi } from '../../contexts/AiContext'
import AdIntegrationPanel from '../../components/AdIntegrationPanel'

export default function PlatformSettings({ embedded }: { embedded?: boolean }) {
  const toast = useToastContext()
  const { setMode } = useAi()
  const [error, setError] = useState<string | null>(null)
  const [clusterName, setClusterName] = useState('')
  const [syncInterval, setSyncInterval] = useState(30)
  const [leadership, setLeadership] = useState<{ controller_id: string; is_leader: boolean; holder_id: string; lease_until: string } | null>(null)
  const [oidc, setOidc] = useState<OidcSettings>({ enabled: false, issuer: '', client_id: '', client_secret: '', redirect_uri: '' })
  const [cpuRules, setCpuRules] = useState<CpuCompatRule[]>([])
  const [cpuJson, setCpuJson] = useState('[]')
  const [quotaProject, setQuotaProject] = useState('default')
  const [quotaVms, setQuotaVms] = useState(0)
  const [quotaVcpu, setQuotaVcpu] = useState(0)
  const [quotaMem, setQuotaMem] = useState(0)
  const [quotaStorage, setQuotaStorage] = useState(0)
  const [quotas, setQuotas] = useState<Array<{ project: string; max_vms: number; max_vcpu: number; max_memory_mib: number; max_storage_gib: number }>>([])
  const [ai, setAi] = useState<AiSettings>({ enabled: false, mode: 'advisor', provider: 'openai', model: 'gpt-4o-mini', api_key_configured: false, autopilot_interval_secs: 0, autopilot_max_actions: 5, fleet_peer_urls: [] })
  const [fleetPeers, setFleetPeers] = useState('')
  const [aiKey, setAiKey] = useState('')
  const [controllerUrl, setControllerUrl] = useState('')
  const [controllerUser, setControllerUser] = useState('')
  const [controllerPass, setControllerPass] = useState('')
  const [directControllerUrl, setDirectControllerUrl] = useState('')

  const load = useCallback(async () => {
    setError(null)
    try {
      const [cluster, o, cpu, lead, settings, q, aiSettings] = await Promise.all([
        getClusterSummary(),
        getOidcSettings(),
        getCpuCompatMatrix(),
        getClusterLeadership(),
        getClusterSettings(),
        listProjectQuotas().catch(() => []),
        getAiSettings().catch(() => null),
      ])
      setClusterName(cluster.name)
      setSyncInterval(settings.inventory_sync_interval_secs)
      setLeadership(lead)
      setOidc(o)
      setCpuRules(cpu)
      setCpuJson(JSON.stringify(cpu, null, 2))
      setQuotas(q)
      if (aiSettings) {
        setAi(aiSettings)
        setFleetPeers((aiSettings.fleet_peer_urls ?? []).join('\n'))
        setMode(aiSettings.enabled ? (aiSettings.mode === 'autopilot' ? 'autopilot' : aiSettings.mode === 'autopilot_preview' ? 'autopilot_preview' : 'advisor') : 'off')
      }
    } catch (e: unknown) { setError(formatUserError(e)) }
  }, [setMode])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    setControllerUrl(getControllerBase())
    setDirectControllerUrl(getDirectControllerBase())
  }, [])

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      error={error}
      onErrorRetry={() => void load()}
      title={embedded ? undefined : 'Platform settings'}
      subtitle={embedded ? undefined : 'Cluster name, OIDC, CPU compatibility, HA'}
      icon={embedded ? undefined : <Settings className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <SettingsBlock embedded={embedded} title="Database" subtitle="Where the controller keeps its state.">
        <DatabaseStatus />
      </SettingsBlock>

      <SettingsBlock embedded={embedded} title="Controller connection" subtitle="HTTP API base and direct WebSocket console URL.">
        <p className="text-xs text-[var(--text-muted)]">
          Daemon proxy: <code className="text-[var(--text-primary)]">{PLATFORM_CONTROLLER_PROXY}</code>
        </p>
        <label className="text-sm block mt-2 text-[var(--text-primary)]">
          Controller base URL
          <input className="input mt-1 block w-full font-mono text-xs" value={controllerUrl} onChange={(e) => setControllerUrl(e.target.value)} placeholder="http://127.0.0.1:5093" />
        </label>
        <p className="text-xs text-[var(--text-muted)] mt-2">
          Direct console base: <code className="text-[var(--text-primary)] break-all">{directControllerUrl}</code>
        </p>
        <div className="grid gap-2 md:grid-cols-2 mt-2">
          <input className="input" aria-label="Basic auth user" placeholder="Basic auth user" value={controllerUser} onChange={(e) => setControllerUser(e.target.value)} />
          <input className="input" type="password" autoComplete="off" aria-label="Basic auth password" placeholder="Basic auth password" value={controllerPass} onChange={(e) => setControllerPass(e.target.value)} />
        </div>
        <button
          type="button"
          data-testid="controller-config-save"
          className="btn-secondary text-sm mt-2"
          onClick={() => {
            if (!controllerUrl.trim()) {
              toast.error('Controller URL is required')
              return
            }
            void setControllerConfig(controllerUrl.trim(), controllerUser, controllerPass)
              .then(() => {
                setControllerPass('')
                setDirectControllerUrl(getDirectControllerBase())
                toast.success('Signed in — controller connection saved')
              })
              .catch((e: unknown) => toast.error(formatUserError(e)))
          }}
        >
          Save controller config
        </button>
      </SettingsBlock>
      {leadership && (
        <SettingsBlock embedded={embedded} title="Controller leadership">
          <div className="space-y-2 text-sm">
          <p className="text-[var(--text-secondary)]">
            This instance: <span className="text-[var(--text-primary)]">{leadership.controller_id}</span>
            {' · '}
            {leadership.is_leader ? <span className={statusToneClass('ok')}>leader</span> : <span className={statusToneClass('warn')}>follower</span>}
          </p>
          <p className="text-[var(--text-muted)]">Holder: {leadership.holder_id || 'none'} · lease until {new Date(leadership.lease_until).toLocaleString()}</p>
          </div>
        </SettingsBlock>
      )}
      <SettingsBlock embedded={embedded} title="Cluster">
        <input className="input" aria-label="Cluster name" value={clusterName} onChange={(e) => setClusterName(e.target.value)} />
        <button type="button" className="btn-secondary text-sm" onClick={async () => {
          try { await patchCluster({ name: clusterName }); toast.success('Cluster updated'); await load() } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}>Save cluster name</button>
      </SettingsBlock>
      <SettingsBlock embedded={embedded} title="Inventory sync" subtitle="Leader-only periodic host inventory sync. Set 0 to disable.">
        <label className="text-sm block text-[var(--text-primary)]">
          Interval (seconds)
          <input type="number" min={0} max={86400} className="input mt-1 block w-40" value={syncInterval}
            onChange={(e) => setSyncInterval(Number(e.target.value))} />
        </label>
        <button type="button" className="btn-secondary text-sm" onClick={async () => {
          try {
            await patchClusterSettings({ inventory_sync_interval_secs: syncInterval })
            toast.success('Sync interval saved')
            await load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}>Save sync interval</button>
      </SettingsBlock>
      <SettingsBlock embedded={embedded} title="OIDC login">
        <label className="flex items-center gap-2 text-sm text-[var(--text-primary)]"><input type="checkbox" checked={oidc.enabled} onChange={(e) => setOidc({ ...oidc, enabled: e.target.checked })} /> Enable OIDC</label>
        <input className="input" aria-label="Issuer URL" placeholder="issuer URL" value={oidc.issuer} onChange={(e) => setOidc({ ...oidc, issuer: e.target.value })} />
        <input className="input" aria-label="Client ID" placeholder="client id" value={oidc.client_id} onChange={(e) => setOidc({ ...oidc, client_id: e.target.value })} />
        <input className="input" type="password" autoComplete="off" aria-label="Client secret" placeholder="client secret" value={oidc.client_secret} onChange={(e) => setOidc({ ...oidc, client_secret: e.target.value })} />
        <input className="input" aria-label="Redirect URI" placeholder="redirect URI (optional)" value={oidc.redirect_uri} onChange={(e) => setOidc({ ...oidc, redirect_uri: e.target.value })} />
        <div className="flex gap-2 flex-wrap">
          <button type="button" className="btn-secondary text-sm" onClick={async () => {
            try { await patchOidcSettings(oidc); toast.success('OIDC settings saved') } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}>Save OIDC</button>
          {oidc.enabled && (
            <a href={safeHref(getOidcLoginUrl())} className="btn-primary text-sm inline-flex items-center">Login with OIDC</a>
          )}
        </div>
      </SettingsBlock>
      <SettingsBlock embedded={embedded} title="Active Directory / LDAP" subtitle="Domain login for machina-daemon (Settings → AD panel on hypervisor). UPN: user@zyvorai.local">
        <AdIntegrationPanel compact />
      </SettingsBlock>
      <SettingsBlock embedded={embedded} title="Project quotas" subtitle="0 = unlimited. Enforced on VM create.">
        {quotas.length > 0 && (
          <ul className="text-xs text-[var(--text-muted)] space-y-1">
            {quotas.map((q) => (
              <li key={q.project}>{q.project}: {q.max_vms || '∞'} VMs, {q.max_vcpu || '∞'} vCPU, {q.max_memory_mib || '∞'} MiB, {q.max_storage_gib || '∞'} GiB</li>
            ))}
          </ul>
        )}
        <div className="grid gap-2 md:grid-cols-2">
          <input className="input" aria-label="Project" placeholder="project" value={quotaProject} onChange={(e) => setQuotaProject(e.target.value)} />
          <input className="input" type="number" aria-label="Max VMs" placeholder="max VMs" value={quotaVms || ''} onChange={(e) => setQuotaVms(Number(e.target.value))} />
          <input className="input" type="number" aria-label="Max vCPU" placeholder="max vCPU" value={quotaVcpu || ''} onChange={(e) => setQuotaVcpu(Number(e.target.value))} />
          <input className="input" type="number" aria-label="Max memory MiB" placeholder="max memory MiB" value={quotaMem || ''} onChange={(e) => setQuotaMem(Number(e.target.value))} />
          <input className="input" type="number" aria-label="Max storage GiB" placeholder="max storage GiB" value={quotaStorage || ''} onChange={(e) => setQuotaStorage(Number(e.target.value))} />
        </div>
        <button type="button" className="btn-secondary text-sm" onClick={async () => {
          try {
            await upsertProjectQuota({ project: quotaProject, max_vms: quotaVms, max_vcpu: quotaVcpu, max_memory_mib: quotaMem, max_storage_gib: quotaStorage })
            toast.success('Quota saved')
            await load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}>Save quota</button>
      </SettingsBlock>
      <SettingsBlock embedded={embedded} title="Zyra AI (BYOK)" subtitle="Deterministic engines work with AI disabled. Optional LLM improves NL parsing and explanations. Configure providers under Settings → AI Providers.">
        <label className="flex items-center gap-2 text-sm text-[var(--text-primary)]">
          <input type="checkbox" checked={ai.enabled} onChange={(e) => setAi({ ...ai, enabled: e.target.checked })} />
          Enable Zyra AI
        </label>
        <label className="text-sm block text-[var(--text-primary)]">
          Mode
          <select className="input mt-1 block w-full max-w-xs" value={ai.mode} onChange={(e) => setAi({ ...ai, mode: e.target.value })}>
            <option value="advisor">Advisor — recommend only</option>
            <option value="autopilot_preview">Autopilot preview — proposed fixes in Zyra</option>
            <option value="autopilot">Autopilot — auto-run low-risk fixes (configurable batch)</option>
          </select>
        </label>
        <label className="text-sm block text-[var(--text-primary)]">
          Autopilot max actions per batch
          <input
            type="number"
            min={1}
            max={10}
            className="input mt-1 block w-40"
            value={ai.autopilot_max_actions ?? 5}
            onChange={(e) => setAi({ ...ai, autopilot_max_actions: Number(e.target.value) })}
          />
        </label>
        {ai.mode === 'autopilot' && (
          <label className="text-sm block text-[var(--text-primary)]">
            Scheduled Autopilot interval (seconds)
            <input
              type="number"
              min={0}
              max={86400}
              className="input mt-1 block w-40"
              value={ai.autopilot_interval_secs}
              onChange={(e) => setAi({ ...ai, autopilot_interval_secs: Number(e.target.value) })}
            />
            <span className="text-xs text-[var(--text-muted)]">0 = manual only. Leader runs safe batch on interval.</span>
            {ai.autopilot_last_run && (
              <span className="text-xs text-[var(--text-muted)] block mt-1">
                Last run: {new Date(ai.autopilot_last_run).toLocaleString()}
              </span>
            )}
          </label>
        )}
        <label className="text-sm block text-[var(--text-primary)]">
          Fleet peer controller URLs (one per line, for multi-cluster Zyra summary)
          <textarea
            className="input mt-1 block w-full max-w-lg min-h-20 font-mono text-xs"
            placeholder="https://controller-site-b.example.com"
            value={fleetPeers}
            onChange={(e) => setFleetPeers(e.target.value)}
          />
        </label>
        <select className="input w-full max-w-xs" aria-label="AI provider" value={ai.provider} onChange={(e) => setAi({ ...ai, provider: e.target.value })}>
          <option value="openai">OpenAI-compatible</option>
          <option value="anthropic">Anthropic</option>
        </select>
        <input className="input" aria-label="AI model" placeholder="Model (e.g. gpt-4o-mini)" value={ai.model} onChange={(e) => setAi({ ...ai, model: e.target.value })} />
        <input className="input" type="password" autoComplete="off" aria-label="API key" placeholder={ai.api_key_configured ? 'API key configured — enter to replace' : 'API key'} value={aiKey} onChange={(e) => setAiKey(e.target.value)} />
        <button type="button" className="btn-secondary text-sm" onClick={async () => {
          try {
            const body: Partial<AiSettings & { api_key?: string }> = {
              enabled: ai.enabled,
              provider: ai.provider,
              model: ai.model,
              mode: ai.mode,
              autopilot_interval_secs: ai.autopilot_interval_secs,
              autopilot_max_actions: ai.autopilot_max_actions ?? 5,
              fleet_peer_urls: fleetPeers.split('\n').map((s) => s.trim()).filter(Boolean),
            }
            if (aiKey.trim()) body.api_key = aiKey.trim()
            const saved = await patchAiSettings(body)
            setAi(saved)
            setAiKey('')
            setMode(saved.enabled ? (saved.mode === 'autopilot' ? 'autopilot' : saved.mode === 'autopilot_preview' ? 'autopilot_preview' : 'advisor') : 'off')
            toast.success('AI settings saved')
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}>Save AI settings</button>
      </SettingsBlock>
      <SettingsBlock embedded={embedded} title="CPU compatibility matrix">
        <textarea className="input font-mono text-xs min-h-32" aria-label="CPU compatibility matrix" value={cpuJson} onChange={(e) => setCpuJson(e.target.value)} />
        <button type="button" className="btn-secondary text-sm" onClick={async () => {
          try {
            const rules = JSON.parse(cpuJson) as CpuCompatRule[]
            await patchCpuCompatMatrix(rules)
            setCpuRules(rules)
            toast.success('CPU matrix updated')
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}>Save matrix ({cpuRules.length} rules)</button>
      </SettingsBlock>
    </PlatformPageChrome>
  )
}

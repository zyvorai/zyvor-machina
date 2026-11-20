// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import { Link } from 'react-router'
import { Plus, Shield } from 'lucide-react'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import UnderlineTabs from '../../components/kit/UnderlineTabs'
import SocAlertDetailPanel from '../../components/platform/soc/SocAlertDetailPanel'
import SocPlaybookEditor from '../../components/platform/soc/SocPlaybookEditor'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import {
  getAsmSummary,
  getSocAlerts,
  getSocEvents,
  getSocIntegrations,
  getSocOverview,
  getSocPlaybookRuns,
  getSocPlaybook,
  getSocPlaybooks,
  getSocSettings,
  getSocRules,
  getSplunkIntegration,
  patchSocIntegration,
  patchSocRule,
  putSplunkIntegration,
  replaySocForward,
  runSocIngestCycle,
  testSocIntegration,
  testSocRule,
  testSplunkIntegration,
  type AsmSummary,
  type SocAlert,
  type SocEvent,
  type SocIntegration,
  type SocOverview,
  type SocPlaybook,
  type SocPlaybookRun,
  type SocRule,
} from '../../api/soc'
import { getFleetThreatSummary } from '../../api/zeusSecurity'
import EbpfActionMenu from '../../components/platform/EbpfActionMenu'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusBadgeClasses, statusToneClass } from '../../utils/semanticColors'
import { useExpandable } from '../../hooks/useExpandable'
import { ExpandableToggle } from '../../components/ui/ExpandableToggle'
import { bySeverityDesc } from '../../utils/topN'

const SOC_TABS = ['overview', 'alerts', 'detections', 'asm', 'integrations', 'playbooks'] as const
type Tab = (typeof SOC_TABS)[number]

function integrationStatusLabel(i: SocIntegration): { tone: 'ok' | 'error' | 'neutral'; text: string } {
  if (i.last_error) return { tone: 'error', text: i.last_error }
  if (i.last_success_at) {
    return { tone: 'ok', text: `Last forward ${new Date(i.last_success_at).toLocaleString()}` }
  }
  return { tone: 'neutral', text: i.enabled ? 'Enabled — awaiting first forward' : 'Disabled' }
}

export default function PlatformSoc() {
  const toast = useToastContext()
  const [tab, setTab] = usePlatformTabState(SOC_TABS, { defaultTab: 'overview' })
  const [overview, setOverview] = useState<SocOverview | null>(null)
  const [threatScore, setThreatScore] = useState<number | null>(null)
  const [alerts, setAlerts] = useState<SocAlert[]>([])
  const [events, setEvents] = useState<SocEvent[]>([])
  const [rules, setRules] = useState<SocRule[]>([])
  const [asm, setAsm] = useState<AsmSummary | null>(null)
  const [splunkUrl, setSplunkUrl] = useState('')
  const [splunkToken, setSplunkToken] = useState('')
  const [splunkIndex, setSplunkIndex] = useState('machina')
  const [splunkEnabled, setSplunkEnabled] = useState(false)
  const [integrations, setIntegrations] = useState<SocIntegration[]>([])
  const [elasticUrl, setElasticUrl] = useState('')
  const [elasticKey, setElasticKey] = useState('')
  const [elasticIndex, setElasticIndex] = useState('logs-machina.soc')
  const [elasticEnabled, setElasticEnabled] = useState(false)
  const [webhookUrl, setWebhookUrl] = useState('')
  const [webhookToken, setWebhookToken] = useState('')
  const [webhookEnabled, setWebhookEnabled] = useState(false)
  const [qradarUrl, setQradarUrl] = useState('')
  const [qradarToken, setQradarToken] = useState('')
  const [qradarLogSource, setQradarLogSource] = useState('')
  const [qradarEnabled, setQradarEnabled] = useState(false)
  const [playbooks, setPlaybooks] = useState<SocPlaybook[]>([])
  const [playbookRuns, setPlaybookRuns] = useState<SocPlaybookRun[]>([])
  // Each panel showed everything at once (or, for events, a silent slice(0, 12) with no way to see
  // the rest); ranked-if-applicable top N with an honest expand instead.
  const eventsList = useExpandable(events, 12)
  const alertsList = useExpandable(alerts, 20, bySeverityDesc)
  const rulesList = useExpandable(rules, 20)
  const findingsList = useExpandable(asm?.open_port_findings ?? [], 20)
  const playbooksList = useExpandable(playbooks, 20)
  const playbookRunsList = useExpandable(playbookRuns, 20)
  const [selectedAlertId, setSelectedAlertId] = useState<string | null>(null)
  const [selectedPlaybookId, setSelectedPlaybookId] = useState<string | null>(null)
  const [newPlaybook, setNewPlaybook] = useState(false)
  const [playbookDetail, setPlaybookDetail] = useState<SocPlaybook | null>(null)
  const [playbookReloadBusy, setPlaybookReloadBusy] = useState(false)
  const [socWebhookUrl, setSocWebhookUrl] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [ov, al, ev, ru, asmRes, threat, splunk, ints, pbs, pbr, socSet] = await Promise.all([
        getSocOverview().catch(() => null),
        getSocAlerts({ limit: 50 }).catch(() => []),
        getSocEvents(30).catch(() => []),
        getSocRules().catch(() => []),
        getAsmSummary().catch(() => null),
        getFleetThreatSummary().catch(() => null),
        getSplunkIntegration().catch(() => null),
        getSocIntegrations().catch(() => []),
        getSocPlaybooks().catch(() => []),
        getSocPlaybookRuns(30).catch(() => []),
        getSocSettings().catch(() => ({ webhook_url: '' })),
      ])
      setOverview(ov)
      setAlerts(al)
      setEvents(ev)
      setRules(ru)
      setAsm(asmRes)
      setThreatScore(threat?.fleet_threat_score ?? null)
      setIntegrations(ints)
      setPlaybooks(pbs)
      setPlaybookRuns(pbr)
      setSocWebhookUrl(socSet.webhook_url ?? '')
      if (splunk?.config) {
        setSplunkUrl(String(splunk.config.url ?? ''))
        setSplunkIndex(String(splunk.config.index ?? 'machina'))
        setSplunkEnabled(splunk.enabled)
      }
      const elastic = ints.find((i) => i.integration_type === 'elastic_bulk')
      if (elastic?.config) {
        setElasticUrl(String(elastic.config.url ?? ''))
        setElasticIndex(String(elastic.config.index ?? 'logs-machina.soc'))
        setElasticEnabled(elastic.enabled)
      }
      const webhook = ints.find((i) => i.integration_type === 'webhook_json')
      if (webhook?.config) {
        setWebhookUrl(String(webhook.config.url ?? ''))
        setWebhookEnabled(webhook.enabled)
      }
      const qradar = ints.find((i) => i.integration_type === 'qradar_rest')
      if (qradar?.config) {
        setQradarUrl(String(qradar.config.url ?? ''))
        setQradarLogSource(String(qradar.config.log_source_id ?? ''))
        setQradarEnabled(qradar.enabled)
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    if (!selectedAlertId && alerts.length > 0) setSelectedAlertId(alerts[0].id)
  }, [alerts, selectedAlertId])

  useEffect(() => {
    if (!newPlaybook && !selectedPlaybookId && playbooks.length > 0) {
      setSelectedPlaybookId(playbooks[0].id)
    }
  }, [playbooks, selectedPlaybookId, newPlaybook])

  const selectedAlert = alerts.find((a) => a.id === selectedAlertId) ?? null
  const selectedPlaybook = playbooks.find((p) => p.id === selectedPlaybookId) ?? null
  const editorPlaybook = playbookDetail ?? selectedPlaybook

  useEffect(() => {
    if (!selectedPlaybookId || newPlaybook) {
      setPlaybookDetail(null)
      return
    }
    void getSocPlaybook(selectedPlaybookId)
      .then(setPlaybookDetail)
      .catch(() => setPlaybookDetail(null))
  }, [selectedPlaybookId, newPlaybook])

  const toggleRule = async (r: SocRule) => {
    try {
      await patchSocRule(r.id, { enabled: !r.enabled })
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const runRuleTest = async (id: string) => {
    try {
      const r = await testSocRule(id)
      toast.success(`Test: ${r.match_count} matches${r.would_fire ? ' (would fire)' : ''}`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const runIngest = async () => {
    try {
      const r = await runSocIngestCycle()
      const ing = r?.ingest
      const total = (ing?.firewall ?? 0) + (ing?.audit ?? 0) + (ing?.platform ?? 0) + (ing?.native_bpf ?? 0)
      toast.success(`Ingest: ${total} new events, ${r?.alerts_fired ?? 0} alerts, ${r?.forwarded ?? 0} forwarded`)
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const saveElastic = async () => {
    try {
      await patchSocIntegration('elastic_bulk', {
        enabled: elasticEnabled,
        config_json: {
          url: elasticUrl,
          api_key: elasticKey,
          index: elasticIndex,
          pipeline: '',
        },
      })
      toast.success('Elastic integration saved')
      setElasticKey('')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const saveWebhook = async () => {
    try {
      await patchSocIntegration('webhook_json', {
        enabled: webhookEnabled,
        config_json: {
          url: webhookUrl,
          bearer_token: webhookToken,
        },
      })
      toast.success('Webhook integration saved')
      setWebhookToken('')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const saveQradar = async () => {
    try {
      await patchSocIntegration('qradar_rest', {
        enabled: qradarEnabled,
        config_json: {
          url: qradarUrl,
          api_token: qradarToken,
          log_source_id: qradarLogSource,
        },
      })
      toast.success('QRadar integration saved')
      setQradarToken('')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const saveSplunk = async () => {
    try {
      await putSplunkIntegration({
        url: splunkUrl,
        token: splunkToken,
        index: splunkIndex,
        enabled: splunkEnabled,
      })
      toast.success('Splunk HEC saved')
      setSplunkToken('')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const tabs: { id: Tab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'alerts', label: 'Alerts' },
    { id: 'detections', label: 'Detections' },
    { id: 'asm', label: 'Attack surface' },
    { id: 'integrations', label: 'Integrations' },
    { id: 'playbooks', label: 'Playbooks' },
  ]

  const splunkStatus = integrations.find((i) => i.integration_type === 'splunk_hec')
  const elasticStatus = integrations.find((i) => i.integration_type === 'elastic_bulk')
  const webhookStatus = integrations.find((i) => i.integration_type === 'webhook_json')
  const qradarStatus = integrations.find((i) => i.integration_type === 'qradar_rest')

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Security Operations Center"
      subtitle="Unified detection, hunting, ASM, and SIEM export"
      icon={<Shield className="w-5 h-5" />}
      loading={loading}
      error={error}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
    >
      <UnderlineTabs
        className="mb-5"
        label="SOC sections"
        tabs={tabs.map((t) => ({ id: t.id, label: t.label }))}
        value={tab}
        onChange={(id) => setTab(id as typeof tab)}
        trailing={
          <>
            <Link to="/platform/zyra/security/hunt" className={`text-[13px] ${hubLinkClasses()}`}>Threat hunting →</Link>
            <Link to="/platform/zeus/security" className={`text-[13px] ${hubLinkClasses()}`}>Security Center →</Link>
          </>
        }
      />

      {tab === 'overview' && (
        <div className="space-y-4">
          <div className="apple-metric-band">
            {[
              { label: 'Open alerts', value: String(overview?.open_alerts ?? '—') },
              { label: 'Critical / high', value: String(overview?.critical_alerts ?? '—') },
              { label: 'Events (24h)', value: String(overview?.events_24h ?? '—') },
              { label: 'Fleet threat', value: threatScore != null ? `${threatScore.toFixed(0)}` : '—' },
            ].map((s) => (
              <div key={s.label} className="min-w-0">
                <div className="apple-metric-value">{s.value}</div>
                <div className="apple-metric-label">{s.label}</div>
              </div>
            ))}
          </div>
          <div className="flex flex-wrap gap-2">
            <button type="button" className="btn-secondary text-sm" onClick={() => void runIngest()}>
              Run ingest now
            </button>
          </div>
          <MacGlassPanel title="Recent events">
            {events.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)] p-3">No SOC events yet — ingestion runs every 2 minutes.</p>
            ) : (
              <>
                <ul id={eventsList.listId} className="divide-y divide-white/5">
                  {eventsList.shown.map((e) => (
                    <li key={e.id} className="px-3 py-2 text-sm flex justify-between gap-2">
                      <span className="text-[var(--text-primary)] truncate">{e.summary}</span>
                      <span className={`text-xs shrink-0 ${statusToneClass(e.severity === 'high' ? 'error' : 'neutral')}`}>
                        {e.source} · {e.severity}
                      </span>
                    </li>
                  ))}
                </ul>
                {eventsList.showToggle && (
                  <div className="p-3">
                    <ExpandableToggle expanded={eventsList.expanded} hidden={eventsList.hidden} listId={eventsList.listId} onToggle={eventsList.toggle} noun="events" />
                  </div>
                )}
              </>
            )}
          </MacGlassPanel>
        </div>
      )}

      {tab === 'alerts' && (
        <div className="grid w-full gap-4 xl:grid-cols-[minmax(0,5fr)_minmax(0,6fr)] xl:items-start">
          <MacGlassPanel title="Alert queue">
            {alerts.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)] p-3">No open alerts.</p>
            ) : (
              <>
                <ul id={alertsList.listId} className="divide-y divide-white/5">
                  {alertsList.shown.map((a) => (
                    <li key={a.id}>
                      <button
                        type="button"
                        className={`w-full text-left px-3 py-3 flex flex-wrap items-center justify-between gap-2 ${
                          selectedAlertId === a.id ? 'bg-[var(--accent)]/10' : 'hover:bg-[var(--surface-hover)]'
                        }`}
                        onClick={() => setSelectedAlertId(a.id)}
                      >
                        <div>
                          <p className="font-medium text-sm text-[var(--text-primary)]">{a.title}</p>
                          <p className="text-xs text-[var(--text-muted)]">
                            {a.severity} · {a.status} · {a.event_count} events
                            {a.assigned_to ? ` · ${a.assigned_to}` : ''}
                          </p>
                        </div>
                        <EbpfActionMenu
                          suggestedKind="deny_process"
                          suggestedMatch="/usr/bin/nc"
                          huntQueryId={a.title.toLowerCase().includes('dns') ? 'dns-anomalies' : 'shell-spawn'}
                          policyName={a.title}
                          compact
                        />
                      </button>
                    </li>
                  ))}
                </ul>
                {alertsList.showToggle && (
                  <div className="p-3">
                    <ExpandableToggle expanded={alertsList.expanded} hidden={alertsList.hidden} listId={alertsList.listId} onToggle={alertsList.toggle} noun="alerts" />
                  </div>
                )}
              </>
            )}
          </MacGlassPanel>
          {selectedAlert ? (
            <SocAlertDetailPanel alert={selectedAlert} onUpdated={() => void load()} />
          ) : (
            <MacGlassPanel title="Alert detail">
              <p className="text-sm text-[var(--text-muted)] p-3">Select an alert to view details.</p>
            </MacGlassPanel>
          )}
        </div>
      )}

      {tab === 'detections' && (
        <MacGlassPanel title="Detection rules">
          <ul id={rulesList.listId} className="divide-y divide-white/5">
            {rules.length === 0 && (
              <li className="px-3 py-6 text-sm text-[var(--text-muted)] text-center">No detection rules configured yet.</li>
            )}
            {rulesList.shown.map((r) => (
              <li key={r.id} className="px-3 py-3 flex flex-wrap items-center justify-between gap-2">
                <div>
                  <p className="font-medium text-sm flex items-center gap-2">
                    {r.name}
                    {r.builtin && <span className={statusBadgeClasses('neutral')}>built-in</span>}
                  </p>
                  <p className="text-xs text-[var(--text-muted)]">{r.description}</p>
                </div>
                <div className="flex gap-2">
                  <button type="button" className="btn-secondary text-xs" onClick={() => void runRuleTest(r.id)}>
                    Test
                  </button>
                  {!r.builtin && (
                    <button type="button" className="btn-secondary text-xs" onClick={() => void toggleRule(r)}>
                      {r.enabled ? 'Disable' : 'Enable'}
                    </button>
                  )}
                  {r.builtin && (
                    <button type="button" className="btn-secondary text-xs" onClick={() => void toggleRule(r)}>
                      {r.enabled ? 'On' : 'Off'}
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
          {rulesList.showToggle && (
            <div className="p-3">
              <ExpandableToggle expanded={rulesList.expanded} hidden={rulesList.hidden} listId={rulesList.listId} onToggle={rulesList.toggle} noun="rules" />
            </div>
          )}
        </MacGlassPanel>
      )}

      {tab === 'asm' && (
        <div className="space-y-4">
          <div className="apple-metric-band">
            {[
              { label: 'Exposure score', value: asm ? asm.exposure_score.toFixed(0) : '—' },
              { label: 'Firewall targets', value: String(asm?.firewall_targets ?? '—') },
              { label: 'High-risk nodes', value: String(asm?.high_risk_nodes ?? '—') },
            ].map((s) => (
              <div key={s.label} className="min-w-0">
                <div className="apple-metric-value">{s.value}</div>
                <div className="apple-metric-label">{s.label}</div>
              </div>
            ))}
          </div>
          <MacGlassPanel title="Findings">
            {(asm?.open_port_findings ?? []).length === 0 ? (
              <p className="text-sm text-[var(--text-muted)] p-3">No high-risk exposure findings.</p>
            ) : (
              <>
                <ul id={findingsList.listId}>
                  {findingsList.shown.map((f, i) => (
                    <MacListRow
                      key={`${f.resource}-${i}`}
                      title={f.resource}
                      subtitle={`${f.kind} · ${f.detail}`}
                    />
                  ))}
                </ul>
                {findingsList.showToggle && (
                  <div className="p-3">
                    <ExpandableToggle expanded={findingsList.expanded} hidden={findingsList.hidden} listId={findingsList.listId} onToggle={findingsList.toggle} noun="findings" />
                  </div>
                )}
              </>
            )}
          </MacGlassPanel>
          {asm?.recommendations?.length ? (
            <MacGlassPanel title="Recommendations">
              <ul className="p-3 text-sm text-[var(--text-secondary)] space-y-1 list-disc list-inside">
                {asm.recommendations.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            </MacGlassPanel>
          ) : null}
        </div>
      )}

      {tab === 'playbooks' && (
        <div className="space-y-4">
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className="btn-primary text-sm flex items-center gap-1"
              onClick={() => {
                setNewPlaybook(true)
                setSelectedPlaybookId(null)
              }}
            >
              <Plus className="w-4 h-4" /> New playbook
            </button>
          </div>
          <div className="flex flex-col gap-4 w-full">
            <div className="space-y-4">
              <MacGlassPanel title="SOAR playbooks">
                {playbooks.length === 0 ? (
                  <p className="text-sm text-[var(--text-muted)] p-3">No playbooks configured.</p>
                ) : (
                  <>
                    <ul id={playbooksList.listId} className="divide-y divide-white/5">
                      {playbooksList.shown.map((p) => (
                        <li key={p.id}>
                          <button
                            type="button"
                            className={`w-full text-left px-3 py-3 ${
                              selectedPlaybookId === p.id && !newPlaybook ? 'bg-[var(--accent)]/10' : 'hover:bg-[var(--surface-hover)]'
                            }`}
                            onClick={() => {
                              setSelectedPlaybookId(p.id)
                              setNewPlaybook(false)
                              setPlaybookDetail(null)
                            }}
                          >
                            <p className="font-medium text-sm text-[var(--text-primary)] flex items-center gap-2">
                              {p.name}
                              <span className={statusBadgeClasses(p.enabled ? 'ok' : 'neutral')}>
                                {p.enabled ? 'enabled' : 'disabled'}
                              </span>
                            </p>
                            <p className="text-xs text-[var(--text-muted)] mt-1">{p.description}</p>
                            <p className="text-[10px] text-[var(--text-faint)] mt-1">
                              {(p.steps_json?.length ?? 0)} step(s) · min {(p.trigger_json?.min_severity as string) ?? 'high'}
                            </p>
                          </button>
                        </li>
                      ))}
                    </ul>
                    {playbooksList.showToggle && (
                      <div className="p-3">
                        <ExpandableToggle expanded={playbooksList.expanded} hidden={playbooksList.hidden} listId={playbooksList.listId} onToggle={playbooksList.toggle} noun="playbooks" />
                      </div>
                    )}
                  </>
                )}
              </MacGlassPanel>
              <MacGlassPanel title="Recent runs">
                {playbookRuns.length === 0 ? (
                  <p className="text-sm text-[var(--text-muted)] p-3">No playbook runs yet.</p>
                ) : (
                  <>
                    <ul id={playbookRunsList.listId} className="divide-y divide-white/5">
                      {playbookRunsList.shown.map((r) => (
                        <li key={r.id} className="px-3 py-2 text-sm flex justify-between gap-2">
                          <span className="text-[var(--text-secondary)] truncate">{r.playbook_id.slice(0, 8)}…</span>
                          <span className={`text-xs shrink-0 ${statusToneClass(r.status === 'completed' ? 'ok' : r.status === 'failed' ? 'error' : 'neutral')}`}>
                            {r.status}
                          </span>
                        </li>
                      ))}
                    </ul>
                    {playbookRunsList.showToggle && (
                      <div className="p-3">
                        <ExpandableToggle expanded={playbookRunsList.expanded} hidden={playbookRunsList.hidden} listId={playbookRunsList.listId} onToggle={playbookRunsList.toggle} noun="runs" />
                      </div>
                    )}
                  </>
                )}
              </MacGlassPanel>
            </div>
            {!newPlaybook && selectedPlaybookId && (
              <button
                type="button"
                data-testid="soc-playbook-reload"
                disabled={playbookReloadBusy}
                className="btn-secondary text-xs"
                onClick={() => {
                  if (!selectedPlaybookId) return
                  setPlaybookReloadBusy(true)
                  void getSocPlaybook(selectedPlaybookId)
                    .then((pb) => {
                      setPlaybookDetail(pb)
                      toast.success(`Reloaded ${pb.name}`)
                    })
                    .catch((e: unknown) => toast.error(formatUserError(e)))
                    .finally(() => setPlaybookReloadBusy(false))
                }}
              >
                {playbookReloadBusy ? 'Reloading…' : 'Reload from API'}
              </button>
            )}
            <SocPlaybookEditor
              playbook={newPlaybook ? null : editorPlaybook}
              globalWebhookUrl={socWebhookUrl}
              isNew={newPlaybook}
              onSaved={() => {
                setNewPlaybook(false)
                void load()
              }}
              onDeleted={() => {
                setSelectedPlaybookId(null)
                void load()
              }}
              onCancelNew={() => setNewPlaybook(false)}
            />
          </div>
        </div>
      )}

      {tab === 'integrations' && (
        <div className="space-y-4">
          <MacGlassPanel title="Splunk HTTP Event Collector (HEC)">
            {splunkStatus && (
              <p className={`px-3 pt-3 text-xs ${statusToneClass(integrationStatusLabel(splunkStatus).tone)}`}>
                {integrationStatusLabel(splunkStatus).text}
              </p>
            )}
            <div className="p-3 space-y-3">
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">HEC URL</span>
                <input
                  className="input w-full mt-1 text-sm"
                  value={splunkUrl}
                  onChange={(e) => setSplunkUrl(e.target.value)}
                  placeholder="https://splunk:8088"
                />
              </label>
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">HEC token (leave blank to keep)</span>
                <input
                  className="input w-full mt-1 text-sm"
                  type="password"
                  autoComplete="off"
                  value={splunkToken}
                  onChange={(e) => setSplunkToken(e.target.value)}
                  placeholder="••••••••"
                />
              </label>
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">Index</span>
                <input
                  className="input w-full mt-1 text-sm"
                  value={splunkIndex}
                  onChange={(e) => setSplunkIndex(e.target.value)}
                />
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={splunkEnabled}
                  onChange={(e) => setSplunkEnabled(e.target.checked)}
                />
                Enable Splunk forwarder
              </label>
              <div className="flex flex-wrap gap-2">
                <button type="button" className="btn-primary text-sm" onClick={() => void saveSplunk()}>
                  Save
                </button>
                <button
                  type="button"
                  className="btn-secondary text-sm"
                  onClick={() => void testSplunkIntegration().then((r) => toast.success(r.message)).catch((e: unknown) => toast.error(formatUserError(e)))}
                >
                  Test connection
                </button>
                <button
                  type="button"
                  className="btn-secondary text-sm"
                  onClick={() => void replaySocForward(24).then((r) => toast.success(`Replay forwarded ${r.forwarded}`)).catch((e: unknown) => toast.error(formatUserError(e)))}
                >
                  Replay 24h
                </button>
              </div>
            </div>
          </MacGlassPanel>

          <MacGlassPanel title="Elastic bulk API">
            {elasticStatus && (
              <p className={`px-3 pt-3 text-xs ${statusToneClass(integrationStatusLabel(elasticStatus).tone)}`}>
                {integrationStatusLabel(elasticStatus).text}
              </p>
            )}
            <div className="p-3 space-y-3">
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">Elasticsearch URL</span>
                <input className="input w-full mt-1 text-sm" value={elasticUrl} onChange={(e) => setElasticUrl(e.target.value)} placeholder="https://elastic:9200" />
              </label>
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">API key (leave blank to keep)</span>
                <input className="input w-full mt-1 text-sm" type="password" autoComplete="off" value={elasticKey} onChange={(e) => setElasticKey(e.target.value)} placeholder="••••••••" />
              </label>
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">Index</span>
                <input className="input w-full mt-1 text-sm" value={elasticIndex} onChange={(e) => setElasticIndex(e.target.value)} />
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={elasticEnabled} onChange={(e) => setElasticEnabled(e.target.checked)} />
                Enable Elastic forwarder
              </label>
              <div className="flex flex-wrap gap-2">
                <button type="button" className="btn-primary text-sm" onClick={() => void saveElastic()}>Save</button>
                <button type="button" className="btn-secondary text-sm" onClick={() => void testSocIntegration('elastic_bulk').then((r) => toast.success(r.message)).catch((e: unknown) => toast.error(formatUserError(e)))}>Test</button>
              </div>
            </div>
          </MacGlassPanel>

          <MacGlassPanel title="JSON webhook (local collector)">
            {webhookStatus && (
              <p className={`px-3 pt-3 text-xs ${statusToneClass(integrationStatusLabel(webhookStatus).tone)}`}>
                {integrationStatusLabel(webhookStatus).text}
              </p>
            )}
            <div className="p-3 space-y-3">
              <p className="text-xs text-[var(--text-muted)]">
                POSTs events as NDJSON to your own collector (Vector, Fluent Bit, Loki proxy, Wazuh, Graylog). Nothing leaves the site unless you point it elsewhere.
              </p>
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">Collector URL</span>
                <input className="input w-full mt-1 text-sm" value={webhookUrl} onChange={(e) => setWebhookUrl(e.target.value)} placeholder="http://10.0.0.5:8686/events" />
              </label>
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">Bearer token (leave blank to keep)</span>
                <input className="input w-full mt-1 text-sm" type="password" autoComplete="off" value={webhookToken} onChange={(e) => setWebhookToken(e.target.value)} placeholder="••••••••" />
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={webhookEnabled} onChange={(e) => setWebhookEnabled(e.target.checked)} />
                Enable webhook forwarder
              </label>
              <div className="flex flex-wrap gap-2">
                <button type="button" className="btn-primary text-sm" onClick={() => void saveWebhook()}>Save</button>
                <button type="button" className="btn-secondary text-sm" onClick={() => void testSocIntegration('webhook_json').then((r) => toast.success(r.message)).catch((e: unknown) => toast.error(formatUserError(e)))}>Test</button>
              </div>
            </div>
          </MacGlassPanel>

          <MacGlassPanel title="IBM QRadar REST">
            {qradarStatus && (
              <p className={`px-3 pt-3 text-xs ${statusToneClass(integrationStatusLabel(qradarStatus).tone)}`}>
                {integrationStatusLabel(qradarStatus).text}
              </p>
            )}
            <div className="p-3 space-y-3">
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">QRadar URL</span>
                <input className="input w-full mt-1 text-sm" value={qradarUrl} onChange={(e) => setQradarUrl(e.target.value)} />
              </label>
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">API token (leave blank to keep)</span>
                <input className="input w-full mt-1 text-sm" type="password" autoComplete="off" value={qradarToken} onChange={(e) => setQradarToken(e.target.value)} placeholder="••••••••" />
              </label>
              <label className="block text-sm">
                <span className="text-[var(--text-muted)] text-xs">Log source ID</span>
                <input className="input w-full mt-1 text-sm" value={qradarLogSource} onChange={(e) => setQradarLogSource(e.target.value)} />
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={qradarEnabled} onChange={(e) => setQradarEnabled(e.target.checked)} />
                Enable QRadar forwarder
              </label>
              <div className="flex flex-wrap gap-2">
                <button type="button" className="btn-primary text-sm" onClick={() => void saveQradar()}>Save</button>
                <button type="button" className="btn-secondary text-sm" onClick={() => void testSocIntegration('qradar_rest').then((r) => toast.success(r.message)).catch((e: unknown) => toast.error(formatUserError(e)))}>Test</button>
              </div>
            </div>
          </MacGlassPanel>
        </div>
      )}
    </PlatformPageChrome>
  )
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VM-to-VM network policy (CiliumNetworkPolicy schema): policies, YAML
// editor with dry-run preview, connection tester, endpoints / selectors and
// live packet flows. Scope: this host (daemon) or the fleet (controller).

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { ShieldHalf } from 'lucide-react'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacSegmentedControl, MacToggle } from '../../components/platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../../components/platform/tahoe/TahoeListKit'
import { Empty, Field, Metrics, headRowCls, rowCls, thCls } from '../../components/bpf/shared'
import FlowTerminal from '../../components/flow/FlowTerminal'
import FlowAlerts from '../../components/flow/FlowAlerts'
import LearnPanel from '../../components/flow/LearnPanel'
import ReplaySummary from '../../components/flow/ReplaySummary'
import ServiceMap from '../../components/flow/ServiceMap'
import QuarantinePanel from '../../components/flow/QuarantinePanel'
import JitPanel from '../../components/flow/JitPanel'
import ThreatFeedsPanel from '../../components/flow/ThreatFeedsPanel'
import DraftPanel from '../../components/flow/DraftPanel'
import ProjectsPanel from '../../components/flow/ProjectsPanel'
import {
  NETPOL_TEMPLATES,
  applyVmNetpol,
  deleteVmNetpol,
  formatRemaining,
  getNetpolStatus,
  listAuthTable,
  listFqdnCache,
  listNetpolEndpoints,
  listNetpolSelectors,
  listQuarantines,
  evidenceFilename,
  getEvidence,
  listVmNetpols,
  replayVmNetpol,
  setVmNetpolEnabled,
  syncFleetNetpol,
  traceVmNetpol,
  validateVmNetpol,
  type AuthEntry,
  type EvidenceFormat,
  type FqdnEntry,
  type NetpolEndpoint,
  type NetpolPreview,
  type NetpolScope,
  type NetpolSelector,
  type NetpolStatus,
  type ReplayResult,
  type TraceQuery,
  type TraceResult,
  type TraceSide,
  type VmNetworkPolicy,
  type VmQuarantine,
} from '../../api/vmNetpol'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { downloadText } from '../../utils/export'
import { statusPillClasses, statusToneClass } from '../../utils/semanticColors'
import { summarizeSpec } from '../../utils/netpolSummary'

type Tab = 'policies' | 'editor' | 'tester' | 'endpoints' | 'flows' | 'map' | 'learn' | 'alerts' | 'projects'

type L7Kind = 'none' | 'http' | 'tls' | 'dns' | 'kafka'

const L7_KINDS: Array<{ value: L7Kind; label: string }> = [
  { value: 'none', label: 'L4 only' },
  { value: 'http', label: 'HTTP' },
  { value: 'tls', label: 'TLS SNI' },
  { value: 'dns', label: 'DNS' },
  { value: 'kafka', label: 'Kafka' },
]

const TABS: Array<{ value: Tab; label: string }> = [
  { value: 'policies', label: 'Policies' },
  { value: 'editor', label: 'YAML editor' },
  { value: 'tester', label: 'Policy tester' },
  { value: 'endpoints', label: 'Endpoints' },
  { value: 'flows', label: 'Flows' },
  { value: 'map', label: 'Service map' },
  { value: 'learn', label: 'Learn' },
  { value: 'alerts', label: 'Alerts' },
  { value: 'projects', label: 'Projects' },
]

function PolicyView({ p }: { p: VmNetworkPolicy }) {
  return (
    <div className="space-y-3 text-xs">
      {p.specs.map((s, i) => {
        const v = summarizeSpec(s)
        return (
          <div key={i} className="rounded-lg border border-white/[0.06] p-3 space-y-2">
            <div>
              <span className="text-[var(--text-muted)]">Applies to </span>
              <span className="font-mono">{v.subject}</span>
              {v.description ? <span className="text-[var(--text-muted)]"> — {v.description}</span> : null}
            </div>
            {v.groupCidrs ? (
              <div className="font-mono">{v.groupCidrs.join(', ')}</div>
            ) : v.rules.length === 0 ? (
              <div className="text-[var(--text-muted)]">No rules.</div>
            ) : (
              <ul className="space-y-1">
                {v.rules.map((r, j) => (
                  <li key={j} className="flex gap-2 items-baseline">
                    <span className={statusPillClasses(r.deny ? 'error' : r.allowNothing ? 'warn' : 'ok')}>
                      {r.direction} {r.deny ? 'deny' : 'allow'}
                    </span>
                    <span className="font-mono">{r.text}</span>
                  </li>
                ))}
              </ul>
            )}
            {v.defaultDeny.length > 0 && (
              <div className="text-[var(--text-muted)]">Default deny: {v.defaultDeny.join(', ')} (traffic not allowed above is dropped)</div>
            )}
          </div>
        )
      })}
    </div>
  )
}

function SideCard({ title, s }: { title: string; s: TraceSide }) {
  const tone = s.verdict === 'allowed' ? 'ok' : ['denied', 'default-deny', 'l7-denied', 'auth-failed'].includes(s.verdict) ? 'error' : 'neutral'
  return (
    <div className="rounded-lg border border-white/[0.06] p-3 space-y-1 text-sm">
      <div className="text-xs text-[var(--text-muted)]">{title}</div>
      <div className="font-medium">{s.vm ?? '—'}</div>
      <span className={statusPillClasses(tone)}>{s.verdict}</span>
      {s.enforced && <span className="ml-2 text-xs text-[var(--text-muted)]">default-deny in this direction</span>}
      {s.rule && <div className="text-xs font-mono text-[var(--text-muted)] pt-1">↳ {s.rule}</div>}
      {s.auth && <div className="text-xs text-[#64d2ff]">Mutual authentication: {s.auth}</div>}
      {s.l7 && <div className="text-xs font-mono text-[#bf5af2]">L7: {s.l7}</div>}
    </div>
  )
}

export default function PlatformVmNetworkPolicies() {
  const toast = useToastContext()
  const [params, setParams] = useSearchParams()
  const tab = (TABS.some((t) => t.value === params.get('tab')) ? params.get('tab') : 'policies') as Tab
  const scope = (params.get('scope') === 'fleet' ? 'fleet' : 'host') as NetpolScope
  const setTab = (t: Tab) => setParams((p) => { p.set('tab', t); return p }, { replace: true })
  const setScope = (s: NetpolScope) => setParams((p) => { p.set('scope', s); return p }, { replace: true })

  const [policies, setPolicies] = useState<VmNetworkPolicy[]>([])
  const [warnings, setWarnings] = useState<string[]>([])
  const [status, setStatus] = useState<NetpolStatus | null>(null)
  const [endpoints, setEndpoints] = useState<NetpolEndpoint[]>([])
  const [selectors, setSelectors] = useState<NetpolSelector[]>([])
  const [fqdn, setFqdn] = useState<FqdnEntry[]>([])
  const [auth, setAuth] = useState<AuthEntry[]>([])
  const [quarantines, setQuarantines] = useState<VmQuarantine[]>([])
  const [quarantineVm, setQuarantineVm] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [expanded, setExpanded] = useState<string | null>(null)

  const [yaml, setYaml] = useState(NETPOL_TEMPLATES[0].yaml)
  const [preview, setPreview] = useState<NetpolPreview | null>(null)
  const [replay, setReplay] = useState<ReplayResult | null>(null)
  const [busy, setBusy] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)

  const [tq, setTq] = useState({ from: '', to: '', protocol: 'tcp', port: '', icmp: '' })
  const [l7, setL7] = useState({ kind: 'none' as L7Kind, method: 'GET', path: '/', host: '', header: '', name: '', apiKey: 'produce', topic: '' })
  const [trace, setTrace] = useState<TraceResult | null>(null)

  const load = useCallback(async () => {
    setError(null)
    try {
      const [l, st, ep, se, fq, au, qu] = await Promise.all([
        listVmNetpols(scope),
        getNetpolStatus(scope).catch(() => null),
        listNetpolEndpoints(scope).catch(() => []),
        listNetpolSelectors(scope).catch(() => []),
        listFqdnCache(scope).catch(() => []),
        listAuthTable(scope).catch(() => []),
        listQuarantines(scope).catch(() => []),
      ])
      setPolicies(l.items)
      setWarnings(l.warnings)
      setStatus(st)
      setEndpoints(ep)
      setSelectors(se)
      setFqdn(fq)
      setAuth(au)
      setQuarantines(qu)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [scope])

  useEffect(() => { void load() }, [load])

  const validate = async () => {
    setBusy(true)
    try {
      setPreview(await validateVmNetpol(scope, yaml))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const runReplay = async () => {
    setBusy(true)
    try {
      setReplay(await replayVmNetpol(scope, yaml))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const editYaml = (y: string) => {
    setYaml(y)
    setPreview(null)
    setReplay(null)
  }

  const apply = async () => {
    setBusy(true)
    try {
      const r = await applyVmNetpol(scope, yaml)
      toast.success(`Applied ${r.applied.join(', ')}`)
      setPreview(null)
      await load()
      setTab('policies')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const remove = async (name: string) => {
    if (!window.confirm(`Delete VM network policy ${name}? VMs it isolates fall back to the remaining policies.`)) return
    try {
      await deleteVmNetpol(scope, name)
      toast.success(`Deleted ${name}`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const toggle = async (p: VmNetworkPolicy, enabled: boolean) => {
    try {
      await setVmNetpolEnabled(p.name, enabled)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const runTrace = async () => {
    const q: TraceQuery = {
      from: tq.from.trim(),
      to: tq.to.trim(),
      protocol: tq.protocol,
      port: tq.port ? Number(tq.port) : undefined,
      icmp_type: tq.protocol === 'icmp' && tq.icmp ? Number(tq.icmp) : undefined,
    }
    if (l7.kind === 'http') {
      Object.assign(q, { http_method: l7.method, http_path: l7.path || '/', http_host: l7.host || undefined, http_headers: l7.header ? [l7.header] : undefined })
    } else if (l7.kind === 'tls') {
      q.server_name = l7.name
    } else if (l7.kind === 'dns') {
      q.dns_name = l7.name
    } else if (l7.kind === 'kafka') {
      Object.assign(q, { kafka_api_key: l7.apiKey, kafka_topic: l7.topic || undefined })
    }
    try {
      setTrace(await traceVmNetpol(scope, q))
    } catch (e: unknown) {
      setTrace(null)
      toast.error(formatUserError(e))
    }
  }

  const importFile = async (f: File | undefined) => {
    if (!f) return
    setYaml(await f.text())
    setPreview(null)
    setTab('editor')
  }

  const vmNames = useMemo(() => endpoints.map((e) => e.name).sort(), [endpoints])
  const quarantineLeft = useMemo(() => {
    const m = new Map<string, number>()
    for (const q of quarantines) m.set(q.vm, Math.max(m.get(q.vm) ?? 0, q.remaining_secs))
    return m
  }, [quarantines])
  const startQuarantine = (vm: string) => {
    setQuarantineVm(vm)
    setTab('endpoints')
    window.setTimeout(() => document.getElementById('q-vm')?.focus(), 50)
  }
  const exportEvidence = async (format: EvidenceFormat) => {
    try {
      const text = await getEvidence(scope, format)
      downloadText(text, evidenceFilename(format), format === 'md' ? 'text/markdown' : 'application/json')
      toast.success('Segmentation evidence downloaded')
    } catch (e) {
      toast.error(formatUserError(e))
    }
  }
  const enforcement = status?.enforcement?.mode ?? (status?.hosts?.some((h) => h.enforcing) ? 'enforce' : 'observe')
  const cilium = status?.cilium ?? status?.hosts?.find((h) => h.cilium)?.cilium ?? null
  const lastSync = status?.last_sync

  return (
    <PlatformPageChrome
      eyebrow="Security"
      title="VM Network Policies"
      subtitle="Which VM may talk to which — ingress and egress, Cilium policy schema, enforced natively by Machina's eBPF datapath on each libvirt host."
      icon={<ShieldHalf className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && policies.length === 0 && !error}
      error={error}
      onErrorRetry={() => void load()}
      actions={
        <div className="flex items-center gap-2">
          {scope === 'fleet' && (
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={() => void syncFleetNetpol().then(() => { toast.success('Pushed to every host'); void load() }, (e: unknown) => toast.error(formatUserError(e)))}
            >
              Sync hosts
            </button>
          )}
          <span className="text-xs text-[var(--text-muted)]">Export evidence</span>
          <button type="button" className="btn-secondary text-xs" aria-label="Export evidence as JSON" onClick={() => void exportEvidence('json')}>
            JSON
          </button>
          <button type="button" className="btn-secondary text-xs" aria-label="Export evidence as Markdown" onClick={() => void exportEvidence('md')}>
            Markdown
          </button>
          <PlatformRefreshButton onClick={() => void load()} />
        </div>
      }
      contentClassName="space-y-4"
    >
      <div className="flex flex-wrap items-end gap-4">
        <MacSegmentedControl
          label="Scope"
          options={[{ value: 'host', label: 'This host' }, { value: 'fleet', label: 'Fleet (controller)' }]}
          value={scope}
          onChange={setScope}
        />
        <MacSegmentedControl label="View" options={TABS} value={tab} onChange={setTab} />
      </div>

      <Metrics
        items={[
          { label: 'Policies', value: policies.length },
          { label: 'VMs selected', value: endpoints.filter((e) => e.policies.length > 0).length },
          { label: 'Managed by', value: status?.managed_by === 'controller' ? 'Controller' : 'This host' },
          {
            label: 'Datapath',
            value: <span className={statusToneClass(enforcement === 'enforce' ? 'warn' : 'neutral')}>{enforcement === 'enforce' ? 'Enforcing' : 'Observe (audit)'}</span>,
          },
          { label: 'Cilium', value: cilium ? 'Present' : 'Absent · native' },
        ]}
      />

      {scope === 'host' && status?.managed_by === 'controller' && (
        <p className={`text-xs ${statusToneClass('warn')}`}>
          The controller manages this host's VM edge — local policies are stored but inactive. Switch scope to Fleet to edit the active set.
        </p>
      )}
      {enforcement !== 'enforce' && policies.length > 0 && (
        <p className="text-xs text-[var(--text-muted)]">
          Observe mode: traffic a policy would drop is logged as AUDIT in Flows and still forwarded. Drops need the enforcement lease (Native eBPF → Enforcement).
        </p>
      )}

      {tab === 'policies' && (
        <JitPanel key={scope} scope={scope} vmNames={vmNames} onChanged={() => void load()} />
      )}

      {tab === 'policies' && (
        <MacGlassPanel
          title="Policies"
          subtitle="CiliumNetworkPolicy and CiliumClusterwideNetworkPolicy documents, applied to VMs by label."
          action={
            <div className="flex gap-2">
              <button type="button" className="btn-secondary text-xs" onClick={() => fileRef.current?.click()}>Import YAML</button>
              <button type="button" className="btn-primary text-xs" onClick={() => { editYaml(NETPOL_TEMPLATES[0].yaml); setTab('editor') }}>
                New policy
              </button>
            </div>
          }
        >
          {policies.length === 0 ? (
            <Empty>No VM network policies. Every VM can reach every other VM until a policy selects it.</Empty>
          ) : (
            <TahoeTableWrap>
              <table className="w-full text-xs" aria-label="VM network policies">
                <thead>
                  <tr className={headRowCls}>
                    <th scope="col" className={thCls}>Name</th>
                    <th scope="col" className={thCls}>Kind</th>
                    <th scope="col" className={thCls}>Selected VMs</th>
                    <th scope="col" className={thCls}>Description</th>
                    {scope === 'fleet' && <th scope="col" className={thCls}>Enabled</th>}
                    <th scope="col" className="py-2 text-right">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {policies.map((p) => (
                    <PolicyRow
                      key={p.name}
                      p={p}
                      fleet={scope === 'fleet'}
                      open={expanded === p.name}
                      onToggleOpen={() => setExpanded(expanded === p.name ? null : p.name)}
                      onEdit={() => { editYaml(p.yaml); setTab('editor') }}
                      onDelete={() => void remove(p.name)}
                      onEnabled={(v) => void toggle(p, v)}
                    />
                  ))}
                </tbody>
              </table>
            </TahoeTableWrap>
          )}
          {warnings.length > 0 && (
            <ul className={`mt-3 text-xs space-y-1 ${statusToneClass('warn')}`}>{warnings.map((w) => <li key={w}>{w}</li>)}</ul>
          )}
          {lastSync && (
            <p className="mt-3 text-xs text-[var(--text-muted)]">
              Last sync {lastSync.at ?? '—'}: {lastSync.skipped ? `skipped (${lastSync.skipped})` : lastSync.ok ? `${lastSync.vms} VMs, ${lastSync.rules} rules, ${lastSync.peers} peers` : `failed — ${lastSync.error}`}
            </p>
          )}
          {status?.hosts && status.hosts.length > 0 && (
            <TahoeTableWrap>
              <table className="w-full text-xs mt-3" aria-label="Host sync status">
                <thead>
                  <tr className={headRowCls}>
                    <th scope="col" className={thCls}>Host</th>
                    <th scope="col" className={thCls}>Sync</th>
                    <th scope="col" className={thCls}>VMs / rules / peers</th>
                    <th scope="col" className={thCls}>Taps</th>
                    <th scope="col" className={thCls}>Owner</th>
                    <th scope="col" className="py-2">Synced</th>
                  </tr>
                </thead>
                <tbody>
                  {status.hosts.map((h) => (
                    <tr key={h.host_id} className={rowCls}>
                      <td className="py-2 pr-2">{h.hostname}</td>
                      <td className="py-2 pr-2">
                        <span className={statusPillClasses(h.reachable === false ? 'neutral' : h.ok === false ? 'error' : h.ok ? 'ok' : 'neutral')}>
                          {h.reachable === false ? 'unreachable' : h.ok === false ? h.error ?? 'error' : h.ok ? 'ok' : 'not synced'}
                        </span>
                      </td>
                      <td className="py-2 pr-2 font-mono">{h.vms ?? 0} / {h.rules ?? 0} / {h.peers ?? 0}</td>
                      <td className="py-2 pr-2">{h.taps ?? 0}</td>
                      <td className="py-2 pr-2">{h.owner || '—'}</td>
                      <td className="py-2 text-[var(--text-muted)]">{h.synced_at ?? '—'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </TahoeTableWrap>
          )}
        </MacGlassPanel>
      )}

      {tab === 'editor' && (
        <div className="space-y-4">
        <DraftPanel
          key={`draft-${scope}`}
          scope={scope}
          editorYaml={yaml}
          onDraft={(d) => {
            setYaml(d.yaml)
            setPreview(d.preview)
            setReplay(d.replay)
          }}
          onChanged={() => void load()}
        />
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
          <MacGlassPanel
            title="YAML"
            subtitle="apiVersion cilium.io/v2 (CiliumCIDRGroup: v2alpha1) — multiple documents with ---. L7 (HTTP, HTTP/2, gRPC, Kafka, DNS, TLS SNI), toFQDNs, toGroups, toServices and authentication are enforced natively."
            action={
              <div className="flex gap-2">
                <select
                  className="input text-xs py-1"
                  aria-label="Template"
                  value=""
                  onChange={(e) => {
                    const t = NETPOL_TEMPLATES.find((x) => x.id === e.target.value)
                    if (t) editYaml(t.yaml)
                  }}
                >
                  <option value="">Templates…</option>
                  {NETPOL_TEMPLATES.map((t) => <option key={t.id} value={t.id}>{t.label}</option>)}
                </select>
                <button type="button" className="btn-secondary text-xs" onClick={() => fileRef.current?.click()}>Import</button>
              </div>
            }
          >
            <textarea
              aria-label="Policy YAML"
              className="input w-full h-[460px] font-mono text-xs leading-relaxed bg-[#0b0b0d] text-[#e5e5ea]"
              spellCheck={false}
              value={yaml}
              onChange={(e) => editYaml(e.target.value)}
            />
            <div className="flex gap-2 mt-3">
              <button type="button" className="btn-secondary text-sm" disabled={busy} onClick={() => void validate()}>Validate &amp; preview</button>
              <button
                type="button"
                className="btn-secondary text-sm"
                disabled={busy}
                title="Evaluate every connection from the last 7 days against this YAML"
                onClick={() => void runReplay()}
              >
                Replay history
              </button>
              <button type="button" className="btn-primary text-sm" disabled={busy || (preview !== null && !preview.valid)} onClick={() => void apply()}>Apply</button>
            </div>
          </MacGlassPanel>
          <MacGlassPanel title="Preview" subtitle="Dry run against the current VM inventory — nothing is changed.">
            {replay && (
              <div className="mb-4 pb-4 border-b border-white/[0.06]">
                <ReplaySummary r={replay} />
              </div>
            )}
            {!preview ? (
              <Empty>Validate to see which VMs each policy selects and how many datapath rules it compiles to.</Empty>
            ) : (
              <div className="space-y-3 text-xs">
                <span className={statusPillClasses(preview.valid ? 'ok' : 'error')}>{preview.valid ? 'Valid' : 'Invalid'}</span>
                <span className="ml-2 text-[var(--text-muted)]">{preview.rules} compiled rule(s) in total</span>
                {preview.errors.map((e) => (
                  <div key={e.path + e.message} className={statusToneClass('error')}><span className="font-mono">{e.path}</span>: {e.message}</div>
                ))}
                {preview.warnings.map((e) => (
                  <div key={e.path + e.message} className={statusToneClass('warn')}><span className="font-mono">{e.path}</span>: {e.message}</div>
                ))}
                {preview.compile_warnings.map((w) => <div key={w} className={statusToneClass('warn')}>{w}</div>)}
                {preview.policies.map((p) => (
                  <div key={p.name} className="space-y-2">
                    <div className="font-medium text-sm">{p.name} <span className="text-[var(--text-muted)] font-normal">· {p.kind}</span></div>
                    <div>Selects: {p.selected_vms.length === 0 ? <span className="text-[var(--text-muted)]">no VMs yet</span> : p.selected_vms.join(', ')}</div>
                    <PolicyView p={p} />
                  </div>
                ))}
                {preview.selectors.length > 0 && (
                  <div>
                    <div className="text-[var(--text-muted)] mb-1">Selectors</div>
                    <ul className="space-y-1 font-mono">
                      {preview.selectors.map((s) => (
                        <li key={s.policy + s.path}>{s.path}: {s.selector} → {s.vms.length ? s.vms.join(', ') : '(none)'}</li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            )}
          </MacGlassPanel>
        </div>
        </div>
      )}

      {tab === 'tester' && (
        <MacGlassPanel title="Policy tester" subtitle="Would this connection be allowed? Evaluates egress at the source and ingress at the destination, like cilium policy trace.">
          <div className="flex flex-wrap items-end gap-3">
            <Field label="From (VM or IP)" htmlFor="tr-from">
              <input id="tr-from" list="netpol-vms" className="input text-sm w-48" value={tq.from} onChange={(e) => setTq({ ...tq, from: e.target.value })} />
            </Field>
            <Field label="To (VM, IP or DNS name)" htmlFor="tr-to">
              <input id="tr-to" list="netpol-vms" className="input text-sm w-48" value={tq.to} onChange={(e) => setTq({ ...tq, to: e.target.value })} />
            </Field>
            <Field label="Protocol" htmlFor="tr-proto">
              <select id="tr-proto" className="input text-sm" value={tq.protocol} onChange={(e) => setTq({ ...tq, protocol: e.target.value })}>
                {['tcp', 'udp', 'sctp', 'icmp', 'any'].map((p) => <option key={p} value={p}>{p.toUpperCase()}</option>)}
              </select>
            </Field>
            {tq.protocol === 'icmp' ? (
              <Field label="ICMP type" htmlFor="tr-icmp">
                <input id="tr-icmp" className="input text-sm w-24" placeholder="8" value={tq.icmp} onChange={(e) => setTq({ ...tq, icmp: e.target.value })} />
              </Field>
            ) : (
              <Field label="Port" htmlFor="tr-port">
                <input id="tr-port" className="input text-sm w-24" placeholder="443" value={tq.port} onChange={(e) => setTq({ ...tq, port: e.target.value })} />
              </Field>
            )}
            <Field label="L7 request" htmlFor="tr-l7">
              <select id="tr-l7" className="input text-sm" value={l7.kind} onChange={(e) => setL7({ ...l7, kind: e.target.value as L7Kind })}>
                {L7_KINDS.map((k) => <option key={k.value} value={k.value}>{k.label}</option>)}
              </select>
            </Field>
            {l7.kind === 'http' && (
              <>
                <Field label="Method" htmlFor="tr-method">
                  <select id="tr-method" className="input text-sm" value={l7.method} onChange={(e) => setL7({ ...l7, method: e.target.value })}>
                    {['GET', 'HEAD', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS'].map((m) => <option key={m}>{m}</option>)}
                  </select>
                </Field>
                <Field label="Path" htmlFor="tr-path">
                  <input id="tr-path" className="input text-sm w-40 font-mono" value={l7.path} onChange={(e) => setL7({ ...l7, path: e.target.value })} />
                </Field>
                <Field label="Host" htmlFor="tr-host">
                  <input id="tr-host" className="input text-sm w-36" value={l7.host} onChange={(e) => setL7({ ...l7, host: e.target.value })} />
                </Field>
                <Field label="Header" htmlFor="tr-header">
                  <input id="tr-header" className="input text-sm w-40 font-mono" placeholder="X-Token: abc" value={l7.header} onChange={(e) => setL7({ ...l7, header: e.target.value })} />
                </Field>
              </>
            )}
            {(l7.kind === 'tls' || l7.kind === 'dns') && (
              <Field label={l7.kind === 'tls' ? 'Server name (SNI)' : 'Query name'} htmlFor="tr-name">
                <input id="tr-name" className="input text-sm w-48 font-mono" value={l7.name} onChange={(e) => setL7({ ...l7, name: e.target.value })} />
              </Field>
            )}
            {l7.kind === 'kafka' && (
              <>
                <Field label="API key / role" htmlFor="tr-kkey">
                  <input id="tr-kkey" className="input text-sm w-32 font-mono" value={l7.apiKey} onChange={(e) => setL7({ ...l7, apiKey: e.target.value })} />
                </Field>
                <Field label="Topic" htmlFor="tr-ktopic">
                  <input id="tr-ktopic" className="input text-sm w-32 font-mono" value={l7.topic} onChange={(e) => setL7({ ...l7, topic: e.target.value })} />
                </Field>
              </>
            )}
            <button type="button" className="btn-primary text-sm" disabled={!tq.from || !tq.to} onClick={() => void runTrace()}>Trace</button>
            <datalist id="netpol-vms">{vmNames.map((n) => <option key={n} value={n} />)}</datalist>
          </div>
          {trace && (
            <div className="mt-4 space-y-3">
              <div className="grid gap-3 sm:grid-cols-2">
                <SideCard title={`Egress at source · ${trace.from.kind} ${trace.from.vm ?? trace.from.input}`} s={trace.egress} />
                <SideCard title={`Ingress at destination · ${trace.to.kind} ${trace.to.vm ?? trace.to.input}`} s={trace.ingress} />
              </div>
              <div className={`text-sm font-semibold ${statusToneClass(trace.allowed ? 'ok' : 'error')}`}>{trace.summary}</div>
            </div>
          )}
        </MacGlassPanel>
      )}

      {tab === 'endpoints' && (
        <>
          <QuarantinePanel
            scope={scope}
            vmNames={vmNames}
            vm={quarantineVm}
            setVm={setQuarantineVm}
            items={quarantines}
            onChanged={() => void load()}
          />
          <MacGlassPanel title="Endpoints" subtitle="VMs with their policy identity, labels and per-direction enforcement (like cilium endpoint list).">
            {endpoints.length === 0 ? (
              <Empty>No VMs found.</Empty>
            ) : (
              <TahoeTableWrap>
                <table className="w-full text-xs" aria-label="Policy endpoints">
                  <thead>
                    <tr className={headRowCls}>
                      <th scope="col" className={thCls}>VM</th>
                      <th scope="col" className={thCls}>Identity</th>
                      {scope === 'fleet' && <th scope="col" className={thCls}>Host</th>}
                      <th scope="col" className={thCls}>Ingress</th>
                      <th scope="col" className={thCls}>Egress</th>
                      <th scope="col" className={thCls}>Addresses</th>
                      <th scope="col" className={thCls}>Labels</th>
                      <th scope="col" className={thCls}>Policies</th>
                      <th scope="col" className="py-2"><span className="sr-only">Quarantine</span></th>
                    </tr>
                  </thead>
                  <tbody>
                    {endpoints.map((e) => (
                      <tr key={e.name} className={rowCls}>
                        <td className="py-2 pr-2 font-medium">{e.name}</td>
                        <td className="py-2 pr-2 font-mono">{e.identity}</td>
                        {scope === 'fleet' && <td className="py-2 pr-2">{e.host ?? '—'}</td>}
                        <td className="py-2 pr-2"><span className={statusPillClasses(e.ingress_enforced ? 'warn' : 'neutral')}>{e.ingress_enforced ? 'enforced' : 'allow all'}</span></td>
                        <td className="py-2 pr-2"><span className={statusPillClasses(e.egress_enforced ? 'warn' : 'neutral')}>{e.egress_enforced ? 'enforced' : 'allow all'}</span></td>
                        <td className="py-2 pr-2 font-mono">{e.addresses.join(', ') || '—'}</td>
                        <td className="py-2 pr-2 font-mono">{Object.entries(e.labels).map(([k, v]) => `${k}=${v}`).join(' ') || '—'}</td>
                        <td className="py-2 pr-2">{e.policies.join(', ') || '—'}</td>
                        <td className="py-2 text-right whitespace-nowrap">
                          {quarantineLeft.has(e.name) ? (
                            <span className={statusPillClasses('error')}>quarantined · {formatRemaining(quarantineLeft.get(e.name) ?? 0)}</span>
                          ) : (
                            <button type="button" className="btn-secondary text-xs" aria-label={`Quarantine ${e.name}`} onClick={() => startQuarantine(e.name)}>
                              Quarantine
                            </button>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </TahoeTableWrap>
            )}
          </MacGlassPanel>
          <MacGlassPanel title="Selectors" subtitle="Every selector in every policy and the VMs it matches right now (like cilium policy selectors).">
            {selectors.length === 0 ? (
              <Empty>No selectors.</Empty>
            ) : (
              <ul className="text-xs font-mono space-y-1">
                {selectors.map((s) => (
                  <li key={s.policy + s.path}>
                    <span className="text-[var(--text-muted)]">{s.policy} {s.path}</span> {s.selector} → {s.vms.length ? s.vms.join(', ') : '(none)'}
                  </li>
                ))}
              </ul>
            )}
          </MacGlassPanel>
          <MacGlassPanel
            title="DNS names (toFQDNs)"
            subtitle="Addresses learned from DNS replies to VMs for names a toFQDNs rule selects (like cilium fqdn cache list). Allow DNS (UDP 53) in policy so lookups reach the resolver."
          >
            {fqdn.length === 0 ? (
              <Empty>No names learned yet. They appear after a selected VM resolves a name a toFQDNs rule matches.</Empty>
            ) : (
              <TahoeTableWrap>
                <table className="w-full text-xs" aria-label="Learned DNS names">
                  <thead>
                    <tr className={headRowCls}>
                      <th scope="col" className={thCls}>Name</th>
                      <th scope="col" className={thCls}>Address</th>
                      <th scope="col" className={thCls}>Identity</th>
                      <th scope="col" className={thCls}>VM</th>
                      {scope === 'fleet' && <th scope="col" className={thCls}>Host</th>}
                      <th scope="col" className={thCls}>Expires</th>
                      <th scope="col" className="py-2">Patterns</th>
                    </tr>
                  </thead>
                  <tbody>
                    {fqdn.map((f) => (
                      <tr key={`${f.hostname ?? ''}|${f.name}|${f.address}`} className={rowCls}>
                        <td className="py-2 pr-2 font-medium font-mono">{f.name}</td>
                        <td className="py-2 pr-2 font-mono">{f.address}</td>
                        <td className="py-2 pr-2 font-mono">{f.identity || '—'}</td>
                        <td className="py-2 pr-2">{f.vm || '—'}</td>
                        {scope === 'fleet' && <td className="py-2 pr-2">{f.hostname ?? '—'}</td>}
                        <td className="py-2 pr-2">{f.expires_in_secs}s</td>
                        <td className="py-2 font-mono">{f.patterns.join(', ') || '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </TahoeTableWrap>
            )}
          </MacGlassPanel>
          <MacGlassPanel
            title="Mutual authentication"
            subtitle="Identity pairs authenticated for authentication.mode rules (like cilium-dbg auth list). New flows on such rules wait for an entry; the source guard keeps VM identities unforgeable."
          >
            {auth.length === 0 ? (
              <Empty>No authentication entries. They appear when a flow hits a rule with authentication.mode: required.</Empty>
            ) : (
              <TahoeTableWrap>
                <table className="w-full text-xs" aria-label="Authentication table">
                  <thead>
                    <tr className={headRowCls}>
                      <th scope="col" className={thCls}>Subject</th>
                      <th scope="col" className={thCls}>Peer</th>
                      <th scope="col" className={thCls}>Mode</th>
                      {scope === 'fleet' && <th scope="col" className={thCls}>Host</th>}
                      <th scope="col" className={thCls}>Expires</th>
                      <th scope="col" className="py-2">State</th>
                    </tr>
                  </thead>
                  <tbody>
                    {auth.map((a) => (
                      <tr key={`${a.hostname ?? ''}|${a.subject_identity}|${a.peer_identity}`} className={rowCls}>
                        <td className="py-2 pr-2 font-medium">{a.subject} <span className="text-[var(--text-muted)] font-mono">[{a.subject_identity}]</span></td>
                        <td className="py-2 pr-2">{a.peer} <span className="text-[var(--text-muted)] font-mono">[{a.peer_identity}]</span></td>
                        <td className="py-2 pr-2 font-mono">{a.mode}</td>
                        {scope === 'fleet' && <td className="py-2 pr-2">{a.hostname ?? '—'}</td>}
                        <td className="py-2 pr-2">{a.expires_in_secs > 0 ? `${a.expires_in_secs}s` : '—'}</td>
                        <td className="py-2">
                          <span className={statusPillClasses(a.state.startsWith('authenticated') ? 'ok' : 'error')}>{a.state}</span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </TahoeTableWrap>
            )}
          </MacGlassPanel>
        </>
      )}

      {tab === 'flows' && <FlowTerminal key={scope} scope={scope} />}
      {tab === 'map' && <ServiceMap key={scope} scope={scope} />}
      {tab === 'learn' && (
        <LearnPanel key={scope} scope={scope} vmNames={vmNames} onOpenInEditor={(y) => { editYaml(y); setTab('editor') }} />
      )}
      {tab === 'alerts' && (
        <div className="space-y-4">
          <FlowAlerts key={scope} scope={scope} onQuarantine={startQuarantine} />
          <ThreatFeedsPanel key={`threat-${scope}`} scope={scope} />
        </div>
      )}

      {tab === 'projects' && (
        scope === 'fleet' ? (
          <ProjectsPanel onChanged={() => void load()} />
        ) : (
          <MacGlassPanel title="Projects">
            <Empty>Project isolation, egress allowlists and egress IPs are fleet features. Switch the scope to Fleet (controller).</Empty>
          </MacGlassPanel>
        )
      )}

      <input ref={fileRef} type="file" accept=".yaml,.yml,.json" className="hidden" onChange={(e) => void importFile(e.target.files?.[0])} />
    </PlatformPageChrome>
  )
}

function PolicyRow({
  p,
  fleet,
  open,
  onToggleOpen,
  onEdit,
  onDelete,
  onEnabled,
}: {
  p: VmNetworkPolicy
  fleet: boolean
  open: boolean
  onToggleOpen: () => void
  onEdit: () => void
  onDelete: () => void
  onEnabled: (v: boolean) => void
}) {
  return (
    <>
      <tr className={rowCls}>
        <td className="py-2 pr-2">
          <button type="button" className="font-medium hover:underline" onClick={onToggleOpen} aria-expanded={open}>
            {open ? '▾' : '▸'} {p.name}
          </button>
        </td>
        <td className="py-2 pr-2">{p.kind}</td>
        <td className="py-2 pr-2" title={p.selected_vms.join(', ')}>
          {p.selected_vms.length === 0 ? <span className="text-[var(--text-muted)]">none</span> : p.selected_vms.slice(0, 4).join(', ') + (p.selected_vms.length > 4 ? ` +${p.selected_vms.length - 4}` : '')}
        </td>
        <td className="py-2 pr-2 text-[var(--text-muted)]">{p.description ?? '—'}</td>
        {fleet && (
          <td className="py-2 pr-2 w-24">
            <div className="-my-3 -mx-4">
              <MacToggle label={p.enabled === false ? 'Off' : 'On'} id={`np-en-${p.name}`} checked={p.enabled !== false} onChange={onEnabled} />
            </div>
          </td>
        )}
        <td className="py-2 text-right whitespace-nowrap">
          <button type="button" className="btn-secondary text-xs mr-2" onClick={onEdit}>Edit</button>
          <button type="button" className="btn-secondary text-xs" onClick={onDelete}>Delete</button>
        </td>
      </tr>
      {open && (
        <tr className={rowCls}>
          <td colSpan={fleet ? 6 : 5} className="py-3">
            <PolicyView p={p} />
          </td>
        </tr>
      )}
    </>
  )
}

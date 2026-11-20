// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native eBPF on this host: machina-bpfd status, enforcement mode, interfaces,
// flows, live event stream, DNS / process telemetry, packet captures, QoS and
// the native data plane (service LB, VM edge, shield, TCP health, TLS, node isolation).

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { Cpu, Download, Pause, Play, Plug, Unplug } from 'lucide-react'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacSegmentedControl, MacToggle } from '../../components/platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../../components/platform/tahoe/TahoeListKit'
import {
  attachBpfInterface,
  bpfStreamUrl,
  detachBpfInterface,
  downloadBpfCapture,
  getBpfAnomalies,
  getBpfDns,
  getBpfFlows,
  getBpfAccounting,
  getBpfHealth,
  getBpfL7,
  getBpfProcesses,
  getBpfStatus,
  listBpfCaptures,
  resetBpfAccounting,
  setBpfMode,
  setBpfQos,
  setBpfTelemetry,
  startBpfCapture,
  type BpfAccountingRecord,
  type BpfAnomaly,
  type BpfCapture,
  type BpfL7Record,
  type BpfDnsRecord,
  type BpfFlowRecord,
  type BpfNetHealth,
  type BpfProcRecord,
  type BpfStatus,
  type BpfTelemetryConfig,
  workloadLabel,
} from '../../api/bpf'
import ServiceLbTab from '../../components/bpf/ServiceLbTab'
import VmEdgeTab from '../../components/bpf/VmEdgeTab'
import ShieldTab from '../../components/bpf/ShieldTab'
import TcpHealthTab from '../../components/bpf/TcpHealthTab'
import TlsTab from '../../components/bpf/TlsTab'
import NodeIsoTab from '../../components/bpf/NodeIsoTab'
import NetChangesTab from '../../components/bpf/NetChangesTab'
import L7SampleTab from '../../components/bpf/L7SampleTab'
import VmRuntimeTab from '../../components/bpf/VmRuntimeTab'
import VmmGuardTab from '../../components/bpf/VmmGuardTab'
import DirectTab from '../../components/bpf/DirectTab'
import QuicLbTab from '../../components/bpf/QuicLbTab'
import AfxdpTab from '../../components/bpf/AfxdpTab'
import SchedulerTab from '../../components/bpf/SchedulerTab'
import GroupedTabs from '../../components/kit/GroupedTabs'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusPillClasses, statusToneClass } from '../../utils/semanticColors'

const TABS = [
  'Overview', 'Flows', 'L7', 'Accounting', 'Live', 'DNS & processes', 'Captures', 'QoS & telemetry',
  'Service LB', 'VM Edge', 'Shield', 'TCP Health', 'TLS / JA4', 'Node Isolation', 'Net changes',
  'L7 sampling', 'VM runtime', 'VMM guard', 'Direct redirect', 'QUIC LB', 'AF_XDP', 'Scheduler',
] as const
type Tab = (typeof TABS)[number]
const TAB_GROUPS: Array<{ name: string; tabs: Tab[] }> = [
  { name: 'Observe', tabs: ['Overview', 'Flows', 'L7', 'Accounting', 'Live', 'DNS & processes', 'Captures', 'TCP Health', 'TLS / JA4', 'Net changes', 'L7 sampling'] },
  { name: 'Enforce', tabs: ['Shield', 'Node Isolation', 'QoS & telemetry'] },
  { name: 'VM', tabs: ['VM Edge', 'VM runtime', 'VMM guard', 'Direct redirect', 'Scheduler'] },
  { name: 'Network', tabs: ['Service LB', 'QUIC LB', 'AF_XDP'] },
]

const TOPICS = ['net', 'dns', 'l7', 'proc', 'anomaly'] as const
const L7_PROTOCOLS = ['', 'tls', 'http', 'ssh'] as const
const LIVE_MAX = 300

type LiveEvent = { id: number; topic: string; data: Record<string, unknown> }

function fmtBytes(n: number): string {
  if (n >= 1 << 30) return `${(n / (1 << 30)).toFixed(1)} GiB`
  if (n >= 1 << 20) return `${(n / (1 << 20)).toFixed(1)} MiB`
  if (n >= 1 << 10) return `${(n / (1 << 10)).toFixed(1)} KiB`
  return `${n} B`
}

function fmtBps(bps: number): string {
  if (!bps) return 'unlimited'
  if (bps >= 1e9) return `${(bps / 1e9).toFixed(1)} Gbit/s`
  if (bps >= 1e6) return `${(bps / 1e6).toFixed(0)} Mbit/s`
  return `${(bps / 1e3).toFixed(0)} kbit/s`
}

function liveSummary(ev: LiveEvent): string {
  const d = ev.data
  const s = (k: string) => (d[k] == null ? '' : String(d[k]))
  switch (ev.topic) {
    case 'net':
      return `${s('kind')} ${s('verdict')} ${s('proto')} ${s('local')}:${s('local_port')} ↔ ${s('remote')}:${s('remote_port')}${d.vm ? ` (${s('vm')})` : ''}`
    case 'dns':
      return `${d.is_response ? 'answer' : 'query'} ${s('qname')} ${s('qtype')}${d.is_response ? ` → ${s('rcode')}` : ''} from ${s('client')}`
    case 'proc':
      return `${s('kind')} ${s('comm')} pid ${s('pid')}${d.path ? ` ${s('path')}` : ''}${d.daddr ? ` → ${s('daddr')}:${s('dport')}` : ''}${d.denied ? ' · denied' : ''}`
    case 'l7':
      return `${s('protocol')} ${s('direction')} ${s('client')} → ${s('server')}:${s('server_port')}${d.host ? ` ${s('host')}` : ''}${d.method ? ` ${s('method')} ${s('path')}` : ''}${d.banner ? ` ${s('banner')}` : ''}`
    case 'anomaly':
      return `${s('severity')} ${s('kind')}: ${s('summary')}`
    default:
      return JSON.stringify(d)
  }
}

export default function PlatformNativeBpf() {
  const toast = useToastContext()
  const [tab, setTab] = useState<Tab>('Overview')
  const [status, setStatus] = useState<BpfStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [leaseMins, setLeaseMins] = useState(15)
  const [attachName, setAttachName] = useState('')

  const [flows, setFlows] = useState<BpfFlowRecord[]>([])
  const [flowVm, setFlowVm] = useState('')
  const [health, setHealth] = useState<BpfNetHealth | null>(null)
  const [dns, setDns] = useState<BpfDnsRecord[]>([])
  const [procs, setProcs] = useState<BpfProcRecord[]>([])
  const [anomalies, setAnomalies] = useState<BpfAnomaly[]>([])
  const [captures, setCaptures] = useState<BpfCapture[]>([])
  const [capIface, setCapIface] = useState('')
  const [capSecs, setCapSecs] = useState(30)
  const [qosDraft, setQosDraft] = useState<Record<string, { egress: string; ingress: string }>>({})
  const [telemetry, setTelemetry] = useState<BpfTelemetryConfig | null>(null)
  const [l7, setL7] = useState<BpfL7Record[]>([])
  const [l7Proto, setL7Proto] = useState<string>('')
  const [accounting, setAccounting] = useState<BpfAccountingRecord[]>([])

  const [live, setLive] = useState<LiveEvent[]>([])
  const [liveTopics, setLiveTopics] = useState<string[]>([...TOPICS])
  const [paused, setPaused] = useState(false)
  const pausedRef = useRef(false)
  const seq = useRef(0)

  const available = status?.available === true
  const ifaces = status?.interfaces ?? []

  const loadStatus = useCallback(async () => {
    setError(null)
    try {
      const st = await getBpfStatus()
      setStatus(st)
      if (st.telemetry) setTelemetry(st.telemetry)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  const loadTab = useCallback(async () => {
    if (!available) return
    try {
      if (tab === 'Overview') {
        const [h, a] = await Promise.all([getBpfHealth(), getBpfAnomalies(20)])
        setHealth(h)
        setAnomalies(a)
      } else if (tab === 'Flows') {
        setFlows(await getBpfFlows(300, flowVm.trim() || undefined))
      } else if (tab === 'L7') {
        setL7(await getBpfL7(300, l7Proto || undefined))
      } else if (tab === 'Accounting') {
        setAccounting(await getBpfAccounting())
      } else if (tab === 'DNS & processes') {
        const [d, p] = await Promise.all([getBpfDns(100), getBpfProcesses(100)])
        setDns(d)
        setProcs(p)
      } else if (tab === 'Captures') {
        setCaptures(await listBpfCaptures())
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }, [available, tab, flowVm, l7Proto, toast])

  useEffect(() => { void loadStatus() }, [loadStatus])
  useEffect(() => { void loadTab() }, [loadTab])

  useEffect(() => { pausedRef.current = paused }, [paused])

  useEffect(() => {
    if (tab !== 'Live' || !available) return
    const es = new EventSource(bpfStreamUrl(liveTopics), { withCredentials: true })
    const onEvent = (topic: string) => (msg: MessageEvent<string>) => {
      if (pausedRef.current) return
      let data: Record<string, unknown>
      try {
        data = JSON.parse(msg.data) as Record<string, unknown>
      } catch {
        return
      }
      seq.current += 1
      const ev = { id: seq.current, topic, data }
      setLive((prev) => [ev, ...prev].slice(0, LIVE_MAX))
    }
    const handlers = TOPICS.map((t) => [t, onEvent(t)] as const)
    for (const [t, h] of handlers) es.addEventListener(t, h as EventListener)
    return () => {
      for (const [t, h] of handlers) es.removeEventListener(t, h as EventListener)
      es.close()
    }
  }, [tab, available, liveTopics])

  const run = (p: Promise<unknown>, ok: string, after?: () => void) =>
    void p
      .then(() => {
        toast.success(ok)
        after?.()
        void loadStatus()
      })
      .catch((e: unknown) => toast.error(formatUserError(e)))

  const mode = status?.mode
  const enforcing = mode?.mode === 'enforce' && !mode.lease_expired

  return (
    <PlatformPageChrome
      eyebrow="Security"
      error={error}
      onErrorRetry={() => void loadStatus()}
      contentLoading={loading && !status}
      prepend={<Link to="/platform/zeus/security" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>← Security Center</Link>}
      title="Native eBPF"
      subtitle={
        <span className="flex flex-wrap items-center gap-2 text-sm">
          <span className={statusPillClasses(available ? (status?.programs_compiled ? 'ok' : 'warn') : 'error')}>
            {available ? (status?.programs_compiled ? 'Datapath loaded' : 'Datapath not compiled') : 'machina-bpfd unreachable'}
          </span>
          {available && (
            <span className={statusPillClasses(enforcing ? 'warn' : 'neutral')}>
              {enforcing ? `Enforcing · ${mode?.lease_remaining_secs ?? 0}s lease left` : 'Observing'}
            </span>
          )}
          {status?.version && <span className="text-[var(--text-muted)]">machina-bpfd {status.version} · this host</span>}
        </span>
      }
      icon={<Cpu className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => { void loadStatus(); void loadTab() }} />}
      contentClassName="space-y-4"
    >
      {status && !available && (
        <MacGlassPanel title="machina-bpfd is not reachable" subtitle={status.socket}>
          <p className="text-sm text-[var(--text-muted)]">
            {status.error ?? 'No response from the bpfd socket.'} Start it with{' '}
            <code>sudo systemctl enable --now machina-bpfd</code>.
          </p>
        </MacGlassPanel>
      )}

      {available && (
        <GroupedTabs label="eBPF sections" groups={TAB_GROUPS} value={tab} onChange={setTab} />
      )}

      {available && tab === 'Overview' && status && (
        <>
          <div className="apple-metric-band">
            {[
              { label: 'Policies', value: `${status.policies_enabled ?? 0}/${status.policies_total ?? 0}` },
              { label: 'Interfaces', value: String(ifaces.length) },
              { label: 'Flows opened', value: String(status.counters?.flows_opened ?? 0) },
              { label: 'Drops', value: String(status.counters?.drops ?? 0) },
              { label: 'Anomalies', value: String(status.counters?.anomalies ?? 0) },
            ].map((m) => (
              <div key={m.label} className="min-w-0">
                <div className="apple-metric-value">{m.value}</div>
                <div className="apple-metric-label">{m.label}</div>
              </div>
            ))}
          </div>

          <MacGlassPanel
            title="Enforcement mode"
            subtitle="Enforce needs a lease; the datapath fails open to observe when it lapses or bpfd restarts."
          >
            <div className="flex flex-wrap items-center gap-2">
              <label className="text-xs text-[var(--text-muted)]" htmlFor="bpf-lease">Lease (minutes)</label>
              <input
                id="bpf-lease"
                type="number"
                min={1}
                max={1440}
                className="input text-sm w-24"
                value={leaseMins}
                onChange={(e) => setLeaseMins(Math.max(1, Math.min(1440, Number(e.target.value) || 1)))}
              />
              <button type="button" className="btn-secondary text-sm" onClick={() => run(setBpfMode('enforce', leaseMins * 60), `Enforcing for ${leaseMins} min`)}>
                {enforcing ? 'Renew lease' : 'Enforce'}
              </button>
              <button type="button" className="btn-secondary text-sm" onClick={() => run(setBpfMode('observe'), 'Back to observe')}>
                Observe
              </button>
              {mode?.lease_expires_at && enforcing && (
                <span className="text-xs text-[var(--text-muted)]">expires {new Date(mode.lease_expires_at).toLocaleTimeString()}</span>
              )}
            </div>
          </MacGlassPanel>

          <MacGlassPanel title="Interfaces" subtitle="VM taps (vnet*/tap*) attach automatically; anything else is manual.">
            {ifaces.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No interfaces attached.</p>
            ) : (
              <ul className="divide-y divide-white/[0.04] text-sm">
                {ifaces.map((i) => (
                  <li key={i.name} className="flex flex-wrap items-center justify-between gap-2 py-2">
                    <span className="text-[var(--text-primary)]">
                      {i.name}
                      <span className="text-xs text-[var(--text-muted)] ml-2">
                        {i.vm ? `VM ${i.vm}` : 'host'} · {i.flags.join(', ') || 'tc'}{i.xdp ? ' · xdp' : ''}
                      </span>
                    </span>
                    <button
                      type="button"
                      className="btn-secondary text-xs inline-flex items-center gap-1"
                      onClick={() => run(detachBpfInterface(i.name), `Detached ${i.name}`)}
                    >
                      <Unplug className="w-3 h-3" /> Detach
                    </button>
                  </li>
                ))}
              </ul>
            )}
            <div className="flex flex-wrap gap-2 mt-3">
              <input
                aria-label="Interface name"
                className="input text-sm w-48 font-mono"
                placeholder="vnet3"
                value={attachName}
                onChange={(e) => setAttachName(e.target.value)}
              />
              <button
                type="button"
                className="btn-secondary text-sm inline-flex items-center gap-1"
                disabled={!attachName.trim()}
                onClick={() => run(attachBpfInterface(attachName.trim()), `Attached ${attachName.trim()}`, () => setAttachName(''))}
              >
                <Plug className="w-4 h-4" /> Attach
              </button>
            </div>
          </MacGlassPanel>

          <MacGlassPanel title="Kernel" subtitle={status.features?.kernel}>
            <ul className="flex flex-wrap gap-2 text-xs">
              {(
                [
                  ['BTF', status.features?.btf],
                  ['TCX', status.features?.tcx],
                  ['BPF LSM', status.features?.lsm_bpf],
                  ['cgroup v2', status.features?.cgroup2],
                  ['fentry', status.features?.fentry],
                  ['sched_ext', status.features?.sched_ext],
                  ['AF_XDP', status.features?.xsk],
                ] as const
              ).map(([k, v]) => (
                <li key={k} className={statusPillClasses(v ? 'ok' : 'warn')}>{k} {v ? 'yes' : 'no'}</li>
              ))}
            </ul>
            {(status.notes?.length ?? 0) > 0 && (
              <ul className="mt-3 text-xs text-[var(--text-muted)] space-y-1">
                {status.notes?.map((n) => <li key={n}>{n}</li>)}
              </ul>
            )}
          </MacGlassPanel>

          {health && (health.drop_reasons.length > 0 || health.tcp.length > 0) && (
            <MacGlassPanel title="Network health" subtitle="Kernel drop reasons and TCP retransmits / resets">
              <div className="grid gap-4 sm:grid-cols-2 text-sm">
                <ul className="space-y-1">
                  {health.drop_reasons.slice(0, 10).map((d) => (
                    <li key={d.reason} className="flex justify-between gap-2">
                      <span className="text-[var(--text-secondary)]">{d.name}</span>
                      <span className="text-[var(--text-muted)]">{d.count}</span>
                    </li>
                  ))}
                </ul>
                <ul className="space-y-1">
                  {health.tcp.slice(0, 10).map((t) => (
                    <li key={`${t.kind}-${t.addr}`} className="flex justify-between gap-2">
                      <span className="text-[var(--text-secondary)]">{t.kind} {t.addr}</span>
                      <span className="text-[var(--text-muted)]">{t.count}</span>
                    </li>
                  ))}
                </ul>
              </div>
            </MacGlassPanel>
          )}

          <MacGlassPanel title="Recent anomalies">
            {anomalies.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No anomalies detected.</p>
            ) : (
              <ul className="text-sm space-y-2">
                {anomalies.map((a) => (
                  <li key={a.id}>
                    <span className={statusToneClass(a.severity === 'high' || a.severity === 'critical' ? 'error' : 'warn')}>
                      {a.kind}
                    </span>
                    <span className="text-[var(--text-muted)]"> · {a.summary} · {new Date(a.ts).toLocaleTimeString()}</span>
                  </li>
                ))}
              </ul>
            )}
          </MacGlassPanel>
        </>
      )}

      {available && tab === 'Flows' && (
        <MacGlassPanel title="Flow table" subtitle="Connections tracked on attached interfaces (newest first)">
          <div className="flex flex-wrap gap-2 mb-3">
            <input
              aria-label="Filter by VM"
              className="input text-sm w-56"
              placeholder="Filter by VM name"
              value={flowVm}
              onChange={(e) => setFlowVm(e.target.value)}
            />
          </div>
          {flows.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No flows recorded.</p>
          ) : (
            <TahoeTableWrap>
              <table className="w-full text-xs" aria-label="eBPF flows">
                <thead>
                  <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                    <th scope="col" className="py-2 pr-2">Workload</th>
                    <th scope="col" className="py-2 pr-2">Peer</th>
                    <th scope="col" className="py-2 pr-2">Proto</th>
                    <th scope="col" className="py-2 pr-2">Sent</th>
                    <th scope="col" className="py-2 pr-2">Received</th>
                    <th scope="col" className="py-2 pr-2">Verdict</th>
                    <th scope="col" className="py-2">Last seen</th>
                  </tr>
                </thead>
                <tbody>
                  {flows.map((f) => (
                    <tr key={`${f.iface}-${f.proto}-${f.local}:${f.local_port}-${f.remote}:${f.remote_port}`} className="border-b border-white/[0.04]">
                      <td className="py-2 pr-2 font-mono">{workloadLabel(f.workload) || f.vm || f.iface} {f.local}:{f.local_port}</td>
                      <td className="py-2 pr-2 font-mono">{f.origin === 'remote' ? '← ' : '→ '}{f.remote}:{f.remote_port}</td>
                      <td className="py-2 pr-2">{f.proto}</td>
                      <td className="py-2 pr-2">{fmtBytes(f.tx_bytes)}</td>
                      <td className="py-2 pr-2">{fmtBytes(f.rx_bytes)}</td>
                      <td className={`py-2 pr-2 ${statusToneClass(f.verdict === 'drop' ? 'error' : 'ok')}`}>{f.verdict}</td>
                      <td className="py-2 text-[var(--text-muted)]">{new Date(f.last_seen).toLocaleTimeString()}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </TahoeTableWrap>
          )}
        </MacGlassPanel>
      )}

      {available && tab === 'L7' && (
        <MacGlassPanel title="Application layer" subtitle="TLS server names, HTTP requests and SSH banners from each TCP flow's first client payload">
          <div className="mb-3">
            <MacSegmentedControl
              options={L7_PROTOCOLS.map((p) => ({ value: p, label: p ? p.toUpperCase() : 'All' }))}
              value={l7Proto}
              onChange={setL7Proto}
            />
          </div>
          {l7.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No application-layer records yet.</p>
          ) : (
            <TahoeTableWrap>
              <table className="w-full text-xs" aria-label="eBPF L7 records">
                <thead>
                  <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                    <th scope="col" className="py-2 pr-2">Time</th>
                    <th scope="col" className="py-2 pr-2">Protocol</th>
                    <th scope="col" className="py-2 pr-2">Client</th>
                    <th scope="col" className="py-2 pr-2">Server</th>
                    <th scope="col" className="py-2">Detail</th>
                  </tr>
                </thead>
                <tbody>
                  {l7.map((r, i) => (
                    <tr key={`${r.ts}-${r.client}:${r.client_port}-${i}`} className="border-b border-white/[0.04]">
                      <td className="py-2 pr-2 text-[var(--text-muted)]">{new Date(r.ts).toLocaleTimeString()}</td>
                      <td className="py-2 pr-2">
                        {r.protocol}
                        {r.tls_version ? <span className="text-[var(--text-muted)]"> {r.tls_version}</span> : null}
                      </td>
                      <td className="py-2 pr-2 font-mono">
                        {r.direction === 'outbound' && r.vm ? `${r.vm} ` : ''}{r.client}:{r.client_port}
                      </td>
                      <td className="py-2 pr-2 font-mono">
                        {r.direction === 'inbound' && r.vm ? `${r.vm} ` : ''}{r.server}:{r.server_port}
                      </td>
                      <td className="py-2 font-mono break-all">
                        {r.host ?? ''}
                        {r.method ? ` ${r.method} ${r.path ?? ''}` : ''}
                        {r.alpn && r.alpn.length > 0 ? <span className="text-[var(--text-muted)]"> [{r.alpn.join(', ')}]</span> : null}
                        {r.banner ?? ''}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </TahoeTableWrap>
          )}
        </MacGlassPanel>
      )}

      {available && tab === 'Accounting' && (
        <MacGlassPanel
          title="Traffic accounting"
          subtitle="Bytes and packets per VM since each window started; survives bpfd restarts (folded every minute)"
          action={
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={accounting.length === 0}
              onClick={() => run(resetBpfAccounting(), 'Accounting reset', () => void loadTab())}
            >
              Reset all
            </button>
          }
        >
          {accounting.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No traffic accounted yet.</p>
          ) : (
            <TahoeTableWrap>
              <table className="w-full text-xs" aria-label="eBPF traffic accounting">
                <thead>
                  <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                    <th scope="col" className="py-2 pr-2">Workload</th>
                    <th scope="col" className="py-2 pr-2">Sent</th>
                    <th scope="col" className="py-2 pr-2">Received</th>
                    <th scope="col" className="py-2 pr-2">Packets</th>
                    <th scope="col" className="py-2 pr-2">Drops</th>
                    <th scope="col" className="py-2 pr-2">Since</th>
                    <th scope="col" className="py-2"><span className="sr-only">Actions</span></th>
                  </tr>
                </thead>
                <tbody>
                  {accounting.map((r) => {
                    const name = r.vm ?? r.interfaces.join(', ')
                    return (
                      <tr key={name} className="border-b border-white/[0.04]">
                        <td className="py-2 pr-2">
                          {name}
                          {r.vm && r.interfaces.length > 0 ? <span className="text-[var(--text-muted)]"> {r.interfaces.join(', ')}</span> : null}
                        </td>
                        <td className="py-2 pr-2">{fmtBytes(r.tx_bytes)}</td>
                        <td className="py-2 pr-2">{fmtBytes(r.rx_bytes)}</td>
                        <td className="py-2 pr-2">{(r.tx_pkts + r.rx_pkts).toLocaleString()}</td>
                        <td className={`py-2 pr-2 ${r.drops > 0 ? statusToneClass('warn') : ''}`}>{r.drops.toLocaleString()}</td>
                        <td className="py-2 pr-2 text-[var(--text-muted)]">{r.since ? new Date(r.since).toLocaleString() : '—'}</td>
                        <td className="py-2">
                          {r.vm && (
                            <button
                              type="button"
                              className="btn-secondary text-xs"
                              onClick={() => run(resetBpfAccounting(r.vm ?? undefined), `Reset ${r.vm}`, () => void loadTab())}
                            >
                              Reset
                            </button>
                          )}
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </TahoeTableWrap>
          )}
        </MacGlassPanel>
      )}

      {available && tab === 'Live' && (
        <MacGlassPanel
          title="Live events"
          subtitle={`Streaming from machina-bpfd · last ${LIVE_MAX} kept`}
          action={
            <div className="flex gap-2">
              <button type="button" className="btn-secondary text-xs inline-flex items-center gap-1" onClick={() => setPaused((p) => !p)}>
                {paused ? <Play className="w-3 h-3" /> : <Pause className="w-3 h-3" />} {paused ? 'Resume' : 'Pause'}
              </button>
              <button type="button" className="btn-secondary text-xs" onClick={() => setLive([])}>Clear</button>
            </div>
          }
        >
          <div className="flex flex-wrap gap-3 mb-3">
            {TOPICS.map((t) => (
              <label key={t} className="inline-flex items-center gap-1 text-xs text-[var(--text-secondary)]">
                <input
                  type="checkbox"
                  checked={liveTopics.includes(t)}
                  onChange={() =>
                    setLiveTopics((prev) => (prev.includes(t) ? prev.filter((x) => x !== t) : [...prev, t]))
                  }
                />
                {t}
              </label>
            ))}
          </div>
          {live.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">Waiting for events…</p>
          ) : (
            <ul className="font-mono text-xs space-y-1 max-h-[60vh] overflow-y-auto" aria-live="off">
              {live.map((ev) => (
                <li key={ev.id} className="flex gap-2">
                  <span className="text-[var(--text-muted)] w-16 shrink-0">{ev.topic}</span>
                  <span className={ev.topic === 'anomaly' || ev.data.verdict === 'drop' || ev.data.denied ? statusToneClass('warn') : 'text-[var(--text-secondary)]'}>
                    {liveSummary(ev)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </MacGlassPanel>
      )}

      {available && tab === 'DNS & processes' && (
        <div className="grid gap-4 lg:grid-cols-2">
          <MacGlassPanel title="DNS" subtitle="Queries and answers seen on attached interfaces">
            {dns.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No DNS traffic recorded.</p>
            ) : (
              <ul className="text-xs font-mono space-y-1 max-h-[60vh] overflow-y-auto">
                {dns.map((d, i) => (
                  <li key={`${d.ts}-${i}`}>
                    <span className="text-[var(--text-muted)]">{new Date(d.ts).toLocaleTimeString()} </span>
                    {d.is_response ? '←' : '→'} {d.qname} {d.qtype}
                    {d.is_response && <span className="text-[var(--text-muted)]"> {d.rcode} {d.answers.map((a) => a.data).join(', ')}</span>}
                  </li>
                ))}
              </ul>
            )}
          </MacGlassPanel>
          <MacGlassPanel title="Processes" subtitle="exec / connect / file / capability events">
            {procs.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No process events recorded.</p>
            ) : (
              <ul className="text-xs font-mono space-y-1 max-h-[60vh] overflow-y-auto">
                {procs.map((p, i) => (
                  <li key={`${p.ts}-${p.pid}-${i}`} className={p.denied ? statusToneClass('warn') : undefined}>
                    <span className="text-[var(--text-muted)]">{new Date(p.ts).toLocaleTimeString()} </span>
                    {p.kind} {p.comm}[{p.pid}] {p.path ?? ''}
                    {p.daddr ? ` → ${p.daddr}:${p.dport ?? ''}` : ''}
                    {p.capability ? ` ${p.capability}` : ''}
                    {p.workload || p.vm || p.container || p.unit ? <span className="text-[var(--text-muted)]"> ({workloadLabel(p.workload) || p.vm || p.container || p.unit})</span> : null}
                    {p.denied ? ' · denied' : ''}
                  </li>
                ))}
              </ul>
            )}
          </MacGlassPanel>
        </div>
      )}

      {available && tab === 'Captures' && (
        <MacGlassPanel title="Packet capture" subtitle="Ring-buffer capture on one interface, downloaded as pcapng for Wireshark">
          <div className="flex flex-wrap items-center gap-2 mb-4">
            <select aria-label="Capture interface" className="input text-sm" value={capIface} onChange={(e) => setCapIface(e.target.value)}>
              <option value="">Interface…</option>
              {ifaces.map((i) => (
                <option key={i.name} value={i.name}>{i.name}{i.vm ? ` (${i.vm})` : ''}</option>
              ))}
            </select>
            <label className="text-xs text-[var(--text-muted)]" htmlFor="cap-secs">Seconds</label>
            <input
              id="cap-secs"
              type="number"
              min={1}
              max={600}
              className="input text-sm w-20"
              value={capSecs}
              onChange={(e) => setCapSecs(Math.max(1, Math.min(600, Number(e.target.value) || 1)))}
            />
            <button
              type="button"
              className="btn-primary text-sm"
              disabled={!capIface}
              onClick={() =>
                void startBpfCapture({ iface: capIface, duration_secs: capSecs })
                  .then(() => {
                    toast.success(`Capturing on ${capIface} for ${capSecs}s`)
                    void loadTab()
                  })
                  .catch((e: unknown) => toast.error(formatUserError(e)))
              }
            >
              Start capture
            </button>
          </div>
          {captures.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No captures yet.</p>
          ) : (
            <ul className="divide-y divide-white/[0.04] text-sm">
              {captures.map((c) => (
                <li key={c.id} className="flex flex-wrap items-center justify-between gap-2 py-2">
                  <span className="text-[var(--text-primary)]">
                    {c.iface}{c.vm ? ` (${c.vm})` : ''}
                    <span className="text-xs text-[var(--text-muted)] ml-2">
                      {c.packets} packets · {fmtBytes(c.bytes)} · {c.done ? 'done' : `until ${new Date(c.ends_at).toLocaleTimeString()}`}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="btn-secondary text-xs inline-flex items-center gap-1"
                    onClick={() =>
                      void downloadBpfCapture(c.id)
                        .then((blob) => {
                          const url = URL.createObjectURL(blob)
                          const a = document.createElement('a')
                          a.href = url
                          a.download = `machina-${c.id}.pcapng`
                          a.click()
                          URL.revokeObjectURL(url)
                        })
                        .catch((e: unknown) => toast.error(formatUserError(e)))
                    }
                  >
                    <Download className="w-3 h-3" /> pcapng
                  </button>
                </li>
              ))}
            </ul>
          )}
        </MacGlassPanel>
      )}

      {available && tab === 'QoS & telemetry' && (
        <>
          <MacGlassPanel title="Bandwidth limits" subtitle="Per-interface EDT pacing (egress) and policing (ingress), Mbit/s — 0 removes the limit">
            {ifaces.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No interfaces attached.</p>
            ) : (
              <ul className="divide-y divide-white/[0.04] text-sm">
                {ifaces.map((i) => {
                  const d = qosDraft[i.name] ?? {
                    egress: String(Math.round(i.qos_egress_bps / 1e6)),
                    ingress: String(Math.round(i.qos_ingress_bps / 1e6)),
                  }
                  const setD = (patch: Partial<typeof d>) => setQosDraft((prev) => ({ ...prev, [i.name]: { ...d, ...patch } }))
                  return (
                    <li key={i.name} className="flex flex-wrap items-center gap-2 py-2">
                      <span className="w-40 text-[var(--text-primary)] truncate">
                        {i.name}{i.vm ? <span className="text-xs text-[var(--text-muted)]"> {i.vm}</span> : null}
                      </span>
                      <span className="text-xs text-[var(--text-muted)] w-48">
                        now ↑{fmtBps(i.qos_egress_bps)} ↓{fmtBps(i.qos_ingress_bps)}
                      </span>
                      <input aria-label={`${i.name} egress Mbit/s`} type="number" min={0} className="input text-sm w-24" value={d.egress} onChange={(e) => setD({ egress: e.target.value })} />
                      <input aria-label={`${i.name} ingress Mbit/s`} type="number" min={0} className="input text-sm w-24" value={d.ingress} onChange={(e) => setD({ ingress: e.target.value })} />
                      <button
                        type="button"
                        className="btn-secondary text-xs"
                        onClick={() =>
                          run(
                            setBpfQos({
                              iface: i.name,
                              egress_bps: Math.max(0, Number(d.egress) || 0) * 1e6,
                              ingress_bps: Math.max(0, Number(d.ingress) || 0) * 1e6,
                            }),
                            `Limits updated on ${i.name}`,
                            () => setQosDraft((prev) => {
                              const next = { ...prev }
                              delete next[i.name]
                              return next
                            }),
                          )
                        }
                      >
                        Apply
                      </button>
                    </li>
                  )
                })}
              </ul>
            )}
          </MacGlassPanel>

          {telemetry && (
            <MacGlassPanel title="Telemetry" subtitle="Which events machina-bpfd records">
              <div className="space-y-2">
                {(['exec', 'fork', 'connect', 'flows', 'dns', 'l7'] as const).map((k) => (
                  <MacToggle
                    key={k}
                    label={k === 'l7' ? 'l7 (TLS SNI / HTTP)' : k}
                    checked={telemetry[k] ?? false}
                    onChange={(v) => setTelemetry({ ...telemetry, [k]: v })}
                  />
                ))}
                <label className="block text-xs text-[var(--text-muted)] pt-2" htmlFor="bpf-watch">File watch prefixes (one per line, max 8)</label>
                <textarea
                  id="bpf-watch"
                  className="input text-sm w-full font-mono h-28"
                  value={telemetry.file_watch.join('\n')}
                  onChange={(e) => setTelemetry({ ...telemetry, file_watch: e.target.value.split('\n') })}
                />
                <label className="block text-xs text-[var(--text-muted)]" htmlFor="bpf-patterns">Auto-attach interface patterns (comma separated)</label>
                <input
                  id="bpf-patterns"
                  className="input text-sm w-full font-mono"
                  value={telemetry.iface_patterns.join(', ')}
                  onChange={(e) => setTelemetry({ ...telemetry, iface_patterns: e.target.value.split(',').map((s) => s.trim()) })}
                />
                <button
                  type="button"
                  className="btn-primary text-sm"
                  onClick={() =>
                    run(
                      setBpfTelemetry({
                        ...telemetry,
                        file_watch: telemetry.file_watch.map((s) => s.trim()).filter(Boolean),
                        iface_patterns: telemetry.iface_patterns.filter(Boolean),
                      }),
                      'Telemetry updated',
                    )
                  }
                >
                  Save telemetry
                </button>
              </div>
            </MacGlassPanel>
          )}
        </>
      )}

      {available && tab === 'Service LB' && <ServiceLbTab />}
      {available && tab === 'VM Edge' && <VmEdgeTab />}
      {available && tab === 'Shield' && <ShieldTab />}
      {available && tab === 'TCP Health' && <TcpHealthTab />}
      {available && tab === 'TLS / JA4' && <TlsTab />}
      {available && tab === 'Node Isolation' && <NodeIsoTab />}
      {available && tab === 'Net changes' && <NetChangesTab />}
      {available && tab === 'L7 sampling' && <L7SampleTab />}
      {available && tab === 'VM runtime' && <VmRuntimeTab />}
      {available && tab === 'VMM guard' && <VmmGuardTab />}
      {available && tab === 'Direct redirect' && <DirectTab />}
      {available && tab === 'QUIC LB' && <QuicLbTab />}
      {available && tab === 'AF_XDP' && <AfxdpTab />}
      {available && tab === 'Scheduler' && <SchedulerTab />}
    </PlatformPageChrome>
  )
}

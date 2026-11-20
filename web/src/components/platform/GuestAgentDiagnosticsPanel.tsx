// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { CheckCircle2, Clock, Loader2, Network, Play, Terminal, Users, XCircle } from 'lucide-react'
import type {
  GuestNetworkConfig,
  GuestObservabilitySnapshot,
  VmGuestHealthReport,
  VmPortForwardRule,
} from '../../api/platform'
import {
  applyGuestNetwork,
  getGuestNetwork,
  guestFstrim,
  guestSyncTime,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { qgaHealthy } from '../../utils/guestAgentUx'
import { statusPillClasses, statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'
import {
  exposeGuestPortOnVm,
  isPrivateGuestIp,
  sshNatHostPort,
  type NatRuleLike,
} from '../../utils/vmPortForwardServices'

function installStateLabel(state: string) {
  switch (state) {
    case 'running':
      return 'Guest agent active'
    case 'channel_only':
      return 'Channel only'
    case 'none':
      return 'Not configured'
    default:
      return state
  }
}

export function installStateTone(state: string, agentPing?: boolean): 'ok' | 'warn' | 'error' | 'neutral' {
  if (state === 'running' && agentPing) return 'ok'
  if (state === 'running' && !agentPing) return 'warn'
  if (state === 'channel_only') return 'warn'
  if (state === 'none') return 'error'
  return 'neutral'
}

function formatDeltaMs(ms: number) {
  const abs = Math.abs(ms)
  if (abs < 1000) return `${ms} ms`
  if (abs < 60_000) return `${(ms / 1000).toFixed(1)} s`
  return `${(ms / 60_000).toFixed(1)} min`
}

export type RunGuestActionFn = (
  key: string,
  fn: () => Promise<unknown>,
  success: string,
) => Promise<void>

type Props = {
  vmId: string
  vmName?: string
  loading?: boolean
  report: VmGuestHealthReport | null
  error?: string | null
  vmState?: string
  lastRefreshedAt?: Date | null
  onRefresh?: () => void
  onInstall?: () => void
  onStartVm?: () => void
  installing?: boolean
  onRunAction?: RunGuestActionFn
  portForwardRules?: VmPortForwardRule[] | NatRuleLike[]
  hypervisorAddress?: string
  onPortForwardRefresh?: () => void
}

export default function GuestAgentDiagnosticsPanel({
  vmId,
  vmName,
  loading,
  report,
  error,
  vmState,
  lastRefreshedAt,
  onRefresh,
  onInstall,
  onStartVm,
  installing,
  onRunAction,
  portForwardRules = [],
  hypervisorAddress,
  onPortForwardRefresh,
}: Props) {
  const toast = useToastContext()
  const [actionBusy, setActionBusy] = useState<string | null>(null)
  const [net, setNet] = useState<GuestNetworkConfig | null>(null)
  const [netLoading, setNetLoading] = useState(false)
  const [iface, setIface] = useState('')
  const [addressCidr, setAddressCidr] = useState('')
  const [gateway, setGateway] = useState('')
  const [dnsServers, setDnsServers] = useState('')
  const [staticRoute, setStaticRoute] = useState('')
  const obs: GuestObservabilitySnapshot | undefined = report?.guest_observability

  const loadNetwork = async () => {
    if (!vmId) return
    setNetLoading(true)
    try {
      const cfg = await getGuestNetwork(vmId)
      setNet(cfg)
      const first = cfg.interfaces.find((i) => i.name !== 'lo') ?? cfg.interfaces[0]
      if (first) {
        setIface((prev) => prev || first.name)
        if (!addressCidr && first.addresses[0]) setAddressCidr(first.addresses[0])
      }
      if (!gateway && cfg.default_gateway) setGateway(cfg.default_gateway)
    } catch {
      setNet(null)
    } finally {
      setNetLoading(false)
    }
  }

  useEffect(() => {
    if (report && qgaHealthy(report)) {
      void loadNetwork()
    } else {
      setNet(null)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- refresh when health identity changes
  }, [vmId, report?.agent_ping, report?.install_state])

  const runAction = async (key: string, fn: () => Promise<unknown>, success: string) => {
    if (onRunAction) {
      await onRunAction(key, fn, success)
      return
    }
    setActionBusy(key)
    try {
      await fn()
      toast.success(success)
      onRefresh?.()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setActionBusy(null)
    }
  }

  if (loading && !report) {
    return (
      <p className="text-sm text-[var(--text-muted)] flex items-center gap-2">
        <Loader2 className="w-4 h-4 animate-spin" /> Testing guest agent…
      </p>
    )
  }

  if (error && !report) {
    return (
      <div className={`rounded-xl border p-4 text-sm ${statusSurfaceClasses('error')}`}>
        <p className="text-[var(--text-primary)] font-medium">Could not load guest health</p>
        <p className="text-xs text-[var(--text-muted)] mt-1">{error}</p>
        {onRefresh && (
          <button type="button" className="btn-secondary text-xs mt-3" onClick={onRefresh}>
            Retry
          </button>
        )}
      </div>
    )
  }

  if (!report) {
    const stopped = vmState === 'stopped' || vmState === 'shut off'
    return (
      <div className={`rounded-xl border p-4 text-sm ${statusSurfaceClasses(stopped ? 'neutral' : 'warn')}`}>
        <p className="text-[var(--text-primary)] font-medium">
          {stopped ? 'Start the VM to test the guest agent' : 'Guest health not available yet'}
        </p>
        <p className="text-xs text-[var(--text-muted)] mt-1">
          {stopped
            ? 'The QEMU guest agent channel is probed while the VM is running.'
            : 'Run a guest health check or wait for the next refresh.'}
        </p>
        <div className="flex flex-wrap gap-2 mt-3">
          {stopped && onStartVm && (
            <button type="button" className="btn-primary text-xs inline-flex items-center gap-1" onClick={onStartVm}>
              <Play className="w-3 h-3" /> Start VM
            </button>
          )}
          {onRefresh && (
            <button type="button" className="btn-secondary text-xs" disabled={loading} onClick={onRefresh}>
              {loading ? <Loader2 className="w-3 h-3 animate-spin inline" /> : null}
              Run health check
            </button>
          )}
        </div>
      </div>
    )
  }

  const tone = installStateTone(report.install_state, report.agent_ping)
  const agentActive = qgaHealthy(report)

  const osPill = [obs?.os_pretty_name || report.os_pretty_name, obs?.os_kernel, agentActive ? 'QGA' : null]
    .filter(Boolean)
    .join(' · ')

  return (
    <div className="space-y-4">
      <div className={`rounded-xl border p-4 backdrop-blur-sm ${statusSurfaceClasses(tone)}`}>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <p className="text-sm font-medium text-[var(--text-primary)]">Guest agent</p>
            <p className="text-xs text-[var(--text-muted)] mt-0.5">{report.summary}</p>
            {osPill && (
              <p className="text-xs text-[var(--text-secondary)] mt-2 font-medium">{osPill}</p>
            )}
          </div>
          <span className={statusPillClasses(tone)}>{installStateLabel(report.install_state)}</span>
        </div>
        {report.agent_version && (
          <p className="text-xs text-[var(--text-muted)] mt-2 font-mono">Agent {report.agent_version}</p>
        )}
        {lastRefreshedAt && (
          <p className="text-xs text-[var(--text-faint)] mt-1">
            Last refreshed {lastRefreshedAt.toLocaleTimeString()}
          </p>
        )}
      </div>

      {obs?.time && (
        <div className="rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2 text-sm flex items-start gap-2">
          <Clock className={`w-4 h-4 shrink-0 mt-0.5 ${Math.abs(obs.time.delta_ms) > 5000 ? statusToneClass('warn') : statusToneClass('ok')}`} />
          <div>
            <p className="text-[var(--text-primary)]">Guest vs host time</p>
            <p className="text-xs text-[var(--text-muted)] mt-0.5 font-mono">
              Δ {formatDeltaMs(obs.time.delta_ms)} · guest {new Date(obs.time.guest_time_rfc3339).toLocaleString()}
            </p>
          </div>
        </div>
      )}

      {obs?.fs_freeze?.frozen && (
        <p className={`text-xs px-3 py-2 rounded-lg border ${statusSurfaceClasses('warn')}`}>
          Filesystems frozen — {obs.fs_freeze.detail}
        </p>
      )}

      {(report.checks?.length ?? 0) > 0 && (
        <ul className="space-y-2">
          {(report.checks ?? []).map((c) => (
            <li
              key={c.id}
              className="flex items-start gap-2 rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2 text-sm"
            >
              {c.passed ? (
                <CheckCircle2 className={`w-4 h-4 shrink-0 mt-0.5 ${statusToneClass('ok')}`} />
              ) : (
                <XCircle className={`w-4 h-4 shrink-0 mt-0.5 ${statusToneClass('error')}`} />
              )}
              <div className="min-w-0">
                <p className="text-[var(--text-primary)]">{c.label}</p>
                <p className="text-xs text-[var(--text-muted)] mt-0.5">{c.detail}</p>
              </div>
            </li>
          ))}
        </ul>
      )}

      {obs?.users && obs.users.length > 0 && (
        <div className="rounded-xl border border-white/[0.06] overflow-hidden">
          <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] px-3 py-2 bg-[var(--apple-surface)] flex items-center gap-1">
            <Users className="w-3 h-3" /> Users & sessions
          </p>
          <ul className="text-xs divide-y divide-white/[0.04]">
            {obs.users.map((u) => (
              <li key={`${u.username}-${u.login_time ?? ''}`} className="px-3 py-2 text-[var(--text-secondary)]">
                <span className="font-medium">{u.username}</span>
                {u.login_time && (
                  <span className="text-[var(--text-muted)]"> · {new Date(u.login_time).toLocaleString()}</span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {obs?.filesystems && obs.filesystems.length > 0 && (
        <div className="rounded-xl border border-white/[0.06] overflow-hidden">
          <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] px-3 py-2 bg-[var(--apple-surface)]">
            Guest filesystems
          </p>
          <div className="overflow-x-auto">
          <table className="w-full text-xs" aria-label="Guest filesystems">
            <thead>
              <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                <th className="px-3 py-2">Mount</th>
                <th className="px-3 py-2 text-right">Used</th>
                <th className="px-3 py-2 text-right">Total</th>
              </tr>
            </thead>
            <tbody>
              {obs.filesystems.map((fs) => {
                const pct = fs.total_bytes > 0 ? Math.round((fs.used_bytes / fs.total_bytes) * 100) : 0
                return (
                  <tr key={fs.mountpoint} className="border-b border-white/[0.04]">
                    <td className="px-3 py-2 font-mono text-[var(--text-secondary)]">{fs.mountpoint}</td>
                    <td className="px-3 py-2 text-right text-[var(--text-muted)]">{pct}%</td>
                    <td className="px-3 py-2 text-right text-[var(--text-muted)]">
                      {(fs.used_bytes / 1024 ** 3).toFixed(1)} / {(fs.total_bytes / 1024 ** 3).toFixed(1)} GiB
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
          </div>
        </div>
      )}

      {obs?.ip_addresses && obs.ip_addresses.length > 0 && (
        <div className="text-xs text-[var(--text-muted)] space-y-1">
          <p className="font-semibold text-[var(--text-secondary)]">Internal IPs</p>
          {obs.ip_addresses
            .filter((a) => a.ip_type !== 'ipv6' && !a.address.startsWith('127.'))
            .map((a) => (
              <p key={`${a.name}-${a.address}`} className="font-mono">
                {a.address}
                <span className="text-[var(--text-muted)]"> · {a.name}</span>
                {a.source ? <span className="text-[var(--text-faint)]"> ({a.source})</span> : null}
              </p>
            ))}
        </div>
      )}

      {agentActive && (
        <div className="rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] p-3 space-y-3">
          <div className="flex items-center justify-between gap-2">
            <p className="text-sm font-medium text-[var(--text-primary)] flex items-center gap-2">
              <Network className="w-4 h-4 text-[var(--text-muted)]" />
              Guest network
            </p>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={netLoading || !!actionBusy}
              onClick={() => void loadNetwork()}
            >
              {netLoading ? <Loader2 className="w-3 h-3 animate-spin inline" /> : null}
              Refresh
            </button>
          </div>
          {net && (
            <div className="text-xs text-[var(--text-muted)] space-y-1">
              {(net.backend || net.backend_detail) && (
                <p className="text-[var(--text-muted)]">
                  Stack:{' '}
                  <span className="text-[var(--text-secondary)] font-mono">{net.backend || 'unknown'}</span>
                  {net.backend_detail ? (
                    <span className="text-[var(--text-faint)]"> — {net.backend_detail}</span>
                  ) : null}
                </p>
              )}
              {net.interfaces.map((i) => (
                <p key={i.name} className="font-mono">
                  {i.name}
                  {i.mac ? <span className="text-[var(--text-faint)]"> · {i.mac}</span> : null}
                  {i.addresses.length
                    ? ` · ${i.addresses.join(', ')}`
                    : ' · (no IPv4)'}
                </p>
              ))}
              {net.default_gateway && (
                <p className="font-mono text-[var(--text-secondary)]">
                  default via {net.default_gateway}
                </p>
              )}
              {net.routes.length > 0 && (
                <details className="mt-1">
                  <summary className="cursor-pointer text-[var(--text-muted)]">Routes ({net.routes.length})</summary>
                  <pre className="mt-1 whitespace-pre-wrap font-mono text-[11px] text-[var(--text-muted)]">
                    {net.routes.join('\n')}
                  </pre>
                </details>
              )}
            </div>
          )}
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
            <label className="text-xs text-[var(--text-muted)] space-y-1">
              <span>Interface</span>
              <input
                className="input w-full text-xs font-mono"
                value={iface}
                onChange={(e) => setIface(e.target.value)}
                placeholder="enp1s0"
                list={`guest-ifaces-${vmId}`}
              />
              <datalist id={`guest-ifaces-${vmId}`}>
                {(net?.interfaces ?? []).map((i) => (
                  <option key={i.name} value={i.name} />
                ))}
              </datalist>
            </label>
            <label className="text-xs text-[var(--text-muted)] space-y-1">
              <span>IPv4 CIDR</span>
              <input
                className="input w-full text-xs font-mono"
                value={addressCidr}
                onChange={(e) => setAddressCidr(e.target.value)}
                placeholder="192.168.122.50/24"
              />
            </label>
            <label className="text-xs text-[var(--text-muted)] space-y-1">
              <span>Gateway</span>
              <input
                className="input w-full text-xs font-mono"
                value={gateway}
                onChange={(e) => setGateway(e.target.value)}
                placeholder="192.168.122.1"
              />
            </label>
            <label className="text-xs text-[var(--text-muted)] space-y-1 sm:col-span-2">
              <span>DNS (space or comma separated)</span>
              <input
                className="input w-full text-xs font-mono"
                value={dnsServers}
                onChange={(e) => setDnsServers(e.target.value)}
                placeholder="1.1.1.1 8.8.8.8"
              />
            </label>
            <label className="text-xs text-[var(--text-muted)] space-y-1">
              <span>Extra route</span>
              <input
                className="input w-full text-xs font-mono"
                value={staticRoute}
                onChange={(e) => setStaticRoute(e.target.value)}
                placeholder="10.0.0.0/8 via 192.168.122.1"
              />
            </label>
          </div>
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={!!actionBusy || !iface.trim() || !addressCidr.trim()}
            onClick={() =>
              void runAction(
                'net',
                async () => {
                  const dns = dnsServers
                    .split(/[,\s]+/)
                    .map((s) => s.trim())
                    .filter(Boolean)
                  const routes: Array<{ to: string; via: string }> = []
                  const routeRaw = staticRoute.trim()
                  if (routeRaw) {
                    const m = routeRaw.match(/^(\S+)\s+via\s+(\S+)$/i)
                    if (!m) {
                      throw new Error('Extra route must look like: 10.0.0.0/8 via 192.168.122.1')
                    }
                    routes.push({ to: m[1], via: m[2] })
                  }
                  await applyGuestNetwork(vmId, {
                    iface: iface.trim(),
                    address_cidr: addressCidr.trim(),
                    gateway: gateway.trim() || undefined,
                    dns: dns.length ? dns : undefined,
                    routes: routes.length ? routes : undefined,
                    replace: true,
                  })
                  await loadNetwork()
                },
                'Guest network applied',
              )
            }
          >
            {actionBusy === 'net' ? <Loader2 className="w-3 h-3 animate-spin inline" /> : null}
            Apply network
          </button>
          <p className="text-[11px] text-[var(--text-faint)]">
            Auto-detects NetworkManager, systemd-networkd, netplan, or wicked; falls back to{' '}
            <code className="font-mono">ip</code> if needed. NM / netplan / wicked / networkd write
            persistent config under <code className="font-mono">/etc</code>; iproute2 fallback is
            runtime-only until reboot.
          </p>
        </div>
      )}

      <div className="flex flex-wrap gap-2">
        {onRefresh && (
          <button type="button" className="btn-secondary text-xs" disabled={loading} onClick={onRefresh}>
            {loading ? <Loader2 className="w-3 h-3 animate-spin inline" /> : null}
            Refresh guest info
          </button>
        )}
        {agentActive && (
          <>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={!!actionBusy}
              onClick={() =>
                void runAction('sync', () => guestSyncTime(vmId), 'Guest time synced to host')
              }
            >
              {actionBusy === 'sync' ? <Loader2 className="w-3 h-3 animate-spin inline" /> : null}
              Sync time to host
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={!!actionBusy}
              onClick={() => void runAction('trim', () => guestFstrim(vmId), 'Filesystem TRIM completed')}
            >
              {actionBusy === 'trim' ? <Loader2 className="w-3 h-3 animate-spin inline" /> : null}
              TRIM filesystems
            </button>
          </>
        )}
        {(() => {
          const guestIp =
            report.guest_ip?.trim() ||
            obs?.ip_addresses?.find((a) => a.ip_type !== 'ipv6' && !a.address.startsWith('127.'))
              ?.address ||
            ''
          const sshExposed = Boolean(sshNatHostPort(portForwardRules))
          const canExpose =
            agentActive &&
            vmName &&
            guestIp &&
            isPrivateGuestIp(guestIp) &&
            !sshExposed
          if (!canExpose && !sshExposed) return null
          return (
            <button
              type="button"
              className="btn-secondary text-xs inline-flex items-center gap-1"
              disabled={!!actionBusy || sshExposed || !vmName}
              title={
                sshExposed
                  ? `SSH already exposed${hypervisorAddress ? ` on ${hypervisorAddress}` : ''}`
                  : 'Create a hypervisor NAT rule for guest port 22'
              }
              onClick={() =>
                void runAction(
                  'ssh',
                  async () => {
                    await exposeGuestPortOnVm(vmId, vmName!, 22, portForwardRules)
                    onPortForwardRefresh?.()
                  },
                  'SSH exposed on hypervisor',
                )
              }
            >
              {actionBusy === 'ssh' ? <Loader2 className="w-3 h-3 animate-spin inline" /> : <Terminal className="w-3 h-3" />}
              {sshExposed ? 'SSH exposed' : 'Expose SSH'}
            </button>
          )
        })()}
        {onInstall && report.install_state !== 'running' && (
          <button type="button" className="btn-secondary text-xs" disabled={installing} onClick={onInstall}>
            {installing ? <Loader2 className="w-3 h-3 animate-spin inline" /> : null}
            Attach virtio channel
          </button>
        )}
      </div>
      {onInstall && report.install_state !== 'running' && (
        <p className="text-[11px] text-[var(--text-faint)]">
          Attach virtio channel only adds the QEMU guest-agent serial device. It does not inject
          guestkit-agent into the disk — install or start the agent inside the guest after the
          channel is present.
        </p>
      )}
    </div>
  )
}

/** Compact pill for VM detail header — click opens Guest health tab. */
export function GuestAgentHeaderPill({
  report,
  onClick,
}: {
  report: VmGuestHealthReport | null | undefined
  onClick?: () => void
}) {
  if (!report) return null
  const agentActive = qgaHealthy(report)
  const tone = installStateTone(report.install_state, report.agent_ping)
  const label = report.os_pretty_name || installStateLabel(report.install_state)
  const kernel = report.guest_observability?.os_kernel
  const parts = [label, kernel, agentActive ? 'QGA' : report.install_state === 'channel_only' ? 'channel' : null].filter(Boolean)
  if (!parts.length) return null
  const className = `${statusPillClasses(tone)} ${onClick ? 'cursor-pointer hover:opacity-90 max-lg:inline-flex max-lg:min-h-11 max-lg:items-center' : ''}`
  if (onClick) {
    return (
      <button type="button" className={className} title={`${report.summary} — open Guest health`} onClick={onClick}>
        {parts.join(' · ')}
      </button>
    )
  }
  return (
    <span className={className} title={report.summary}>
      {parts.join(' · ')}
    </span>
  )
}

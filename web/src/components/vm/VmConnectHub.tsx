// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Link } from 'react-router'
import {
  ArrowRight,
  CheckCircle2,
  Circle,
  Copy,
  Download,
  ExternalLink,
  HelpCircle,
  Loader2,
  Monitor,
  Terminal,
} from 'lucide-react'
import type { GuestPortReport } from '../../api/zeusFirewall'
import { type VmPortForwardRule } from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import VmPortForwardPanel from './VmPortForwardPanel'
import { hubLinkClasses } from '../../utils/semanticColors'
import { VM_DAILY_ACCESS_GUIDE_URL } from '../../utils/vmDailyAccessGuide'
import VmSshConnectDialog, { navigateVmSshSession } from './VmSshConnectDialog'
import type { GuestAccessHints } from '../../utils/guestAccessHints'
import {
  exposeGuestPortOnVm,
  isPrivateGuestIp,
  laptopHttpHref,
  laptopSshCommand,
  natRuleForGuestPort,
  sshNatHostPort,
} from '../../utils/vmPortForwardServices'

function downloadText(filename: string, content: string, mime: string) {
  const blob = new Blob([content], { type: mime })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

export interface VmConnectHubProps {
  vmName: string
  vmState: string
  sshUser: string
  guestIp?: string
  detectedIps?: string[]
  consoleHref: string
  specJson?: string
  onExportXml?: () => Promise<string>
  guestPorts?: GuestPortReport | null
  guestPortsLoading?: boolean
  onRefreshPorts?: () => void
  onAllPorts?: () => void
  natForwardHref?: string
  platformVmId?: string
  hypervisorAddress?: string
  guestAccess?: GuestAccessHints | null
  portForwardRules?: VmPortForwardRule[]
  onRefreshPortForwards?: () => void
  guestIpWaiting?: boolean
  onRefreshGuestIp?: () => void
  onInstallGuestTools?: () => void
  guestToolsInstalling?: boolean
  guestIpHint?: string
  helpGuideHref?: string
  disabled?: boolean
  onNotify?: (message: string) => void
  /** Overview: collapse NAT panel; Access tab: show full panel. */
  natExpanded?: boolean
  onOpenAccessTab?: () => void
  showExport?: boolean
}

export default function VmConnectHub({
  vmName,
  vmState,
  sshUser,
  guestIp = '',
  detectedIps = [],
  consoleHref,
  specJson,
  onExportXml,
  guestPorts,
  guestPortsLoading,
  onRefreshPorts,
  onAllPorts,
  natForwardHref,
  platformVmId,
  hypervisorAddress,
  guestAccess,
  portForwardRules = [],
  onRefreshPortForwards,
  guestIpWaiting = false,
  onRefreshGuestIp,
  onInstallGuestTools,
  guestToolsInstalling = false,
  guestIpHint,
  helpGuideHref = VM_DAILY_ACCESS_GUIDE_URL,
  disabled = false,
  onNotify,
  natExpanded = false,
  onOpenAccessTab,
  showExport = false,
}: VmConnectHubProps) {
  const [sshOpen, setSshOpen] = useState(false)
  const [exposeBusy, setExposeBusy] = useState<number | null>(null)
  const [natOpen, setNatOpen] = useState(natExpanded)
  const [copiedKey, setCopiedKey] = useState<string | null>(null)
  const running = vmState === 'running'
  const ip = guestIp.trim()
  const topPorts = (guestPorts?.ports ?? []).slice(0, 5)
  const privateIp = ip ? isPrivateGuestIp(ip) : Boolean(guestAccess?.guest_ip_private)
  const sshExposed = Boolean(sshNatHostPort(portForwardRules))
  const sshCommand = ip ? laptopSshCommand(sshUser, ip, hypervisorAddress, portForwardRules) : ''

  const notify = (msg: string) => onNotify?.(msg)

  const copy = async (text: string, label: string, key: string) => {
    try {
      await navigator.clipboard.writeText(text)
      notify(label)
      setCopiedKey(key)
      window.setTimeout(() => setCopiedKey((k) => (k === key ? null : k)), 700)
    } catch {
      notify('Copy failed')
    }
  }

  const openSsh = (host: string, user: string, port?: number) => {
    if (port && port !== 22) {
      const qs = new URLSearchParams({ host, user, port: String(port) })
      if (platformVmId) qs.set('vmId', platformVmId)
      if (vmName) qs.set('vmName', vmName)
      window.location.href = `/ssh?${qs.toString()}`
      return
    }
    navigateVmSshSession(vmName, host, user, platformVmId)
  }

  const exposeGuestPort = async (guestPort: number) => {
    if (!platformVmId) return
    setExposeBusy(guestPort)
    try {
      await exposeGuestPortOnVm(platformVmId, vmName, guestPort, portForwardRules)
      notify(`Exposed guest port ${guestPort} on hypervisor`)
      onRefreshPortForwards?.()
    } catch (e: unknown) {
      notify(formatUserError(e))
    } finally {
      setExposeBusy(null)
    }
  }

  const exportBundle = async () => {
    if (specJson) downloadText(`${vmName}-spec.json`, specJson, 'application/json')
    if (onExportXml) {
      try {
        const xml = await onExportXml()
        downloadText(`${vmName}.xml`, xml, 'application/xml')
        notify('Spec and domain XML downloaded')
      } catch {
        notify('Domain XML export failed')
      }
    } else if (specJson) {
      notify('Spec downloaded')
    }
  }

  const natRuleCount = portForwardRules.length
  const laptopStepsDone = [running, Boolean(ip), sshExposed].filter(Boolean).length
  const laptopProgress = guestAccess?.guest_ip_private ? Math.round((laptopStepsDone / 3) * 100) : 0

  return (
    <>
      <div className="vm-connect-hub p-4 space-y-4 animate-fade-in" data-testid="vm-daily-access">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <p className="apple-eyebrow">Connect</p>
            <p className="apple-lede mt-1 text-base">Cinema, SSH, NAT, and laptop commands.</p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {ip && (
              <span className="text-xs font-mono text-emerald-600/90 px-2 py-1 rounded-full bg-emerald-500/10 border border-emerald-500/20">
                {privateIp ? `${sshUser}@${ip} (NAT)` : `${sshUser}@${ip}`}
                {sshExposed && hypervisorAddress ? ` · SSH :${sshNatHostPort(portForwardRules)}` : ''}
              </span>
            )}
            {running && !ip && guestIpWaiting && (
              <span className="text-xs text-amber-600/90 inline-flex items-center gap-1">
                <Loader2 className="w-3 h-3 animate-spin" /> Waiting for guest IP…
              </span>
            )}
            <a
              href={helpGuideHref}
              target="_blank"
              rel="noreferrer"
              className={`text-xs inline-flex items-center gap-1 ${hubLinkClasses()}`}
              title="Daily access guide"
            >
              <HelpCircle className="w-3 h-3" /> Guide
            </a>
          </div>
        </div>

        {guestAccess?.guest_ip_private && (
          <div className="space-y-2 py-2 border-y border-[var(--apple-hairline)]" data-testid="vm-laptop-access-checklist">
            <div className="flex items-center justify-between gap-2">
              <p className="apple-eyebrow mb-0">Laptop path</p>
              <span className="text-[10px] font-medium text-[var(--text-muted)]">{laptopProgress}%</span>
            </div>
            <div className="vm-laptop-progress" aria-hidden>
              <div className="vm-laptop-progress-bar" style={{ width: `${laptopProgress}%` }} />
            </div>
            <ul className="space-y-0.5 text-xs">
              <li className={`vm-laptop-step ${running ? 'vm-laptop-step--done' : ''}`}>
                {running ? <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" /> : <Circle className="w-3.5 h-3.5 text-[var(--text-muted)]" />}
                <span className="vm-laptop-step-label text-[var(--text-muted)]">VM running</span>
              </li>
              <li className={`vm-laptop-step ${ip ? 'vm-laptop-step--done' : ''}`}>
                {ip ? <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" /> : <Circle className="w-3.5 h-3.5 text-[var(--text-muted)]" />}
                <span className="vm-laptop-step-label text-[var(--text-muted)]">{ip || 'Guest IP'}</span>
              </li>
              <li className={`vm-laptop-step ${sshExposed ? 'vm-laptop-step--done' : ''}`}>
                {sshExposed ? <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" /> : <Circle className="w-3.5 h-3.5 text-[var(--text-muted)]" />}
                <span className="vm-laptop-step-label text-[var(--text-muted)]">{sshExposed ? 'SSH exposed' : 'Expose SSH'}</span>
              </li>
            </ul>
          </div>
        )}

        <div className="flex flex-wrap gap-2">
          <Link
            to={consoleHref}
            className={`btn-primary text-xs inline-flex items-center gap-1 vm-cinema-cta ${!running || disabled ? 'pointer-events-none opacity-50' : ''}`}
            aria-disabled={!running || disabled}
          >
            <Monitor className="w-3.5 h-3.5" /> Open Cinema
          </Link>
          <button
            type="button"
            className="btn-secondary text-xs inline-flex items-center gap-1"
            disabled={disabled}
            onClick={() => {
              if (ip && running) {
                const nat = natRuleForGuestPort(portForwardRules, 22)
                if (privateIp && nat && hypervisorAddress) openSsh(hypervisorAddress, sshUser, nat.host_port)
                else openSsh(ip, sshUser)
              } else setSshOpen(true)
            }}
          >
            <Terminal className="w-3.5 h-3.5" /> SSH
          </button>
          <button
            type="button"
            className={`btn-secondary text-xs inline-flex items-center gap-1 ${copiedKey === 'ssh' ? 'vm-copy-flash' : ''}`}
            disabled={!sshCommand}
            onClick={() => void copy(sshCommand, 'SSH command copied (add -i your-key if needed)', 'ssh')}
          >
            <Copy className="w-3 h-3" /> {copiedKey === 'ssh' ? 'Copied!' : 'Copy laptop cmd'}
          </button>
          <button
            type="button"
            className={`btn-secondary text-xs ${copiedKey === 'ip' ? 'vm-copy-flash' : ''}`}
            disabled={!ip}
            onClick={() => void copy(ip, 'Guest IP copied', 'ip')}
          >
            <Copy className="w-3 h-3 inline" /> {copiedKey === 'ip' ? 'Copied!' : 'IP'}
          </button>
        </div>

        {running && !ip && guestIpHint && (
          <p className="text-xs text-amber-700/90">{guestIpHint}</p>
        )}

        <div className="grid gap-4 md:grid-cols-2">
          <section className="space-y-2 min-w-0">
            <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Guest ports</p>
            {guestPortsLoading && <p className="text-xs text-[var(--text-muted)]">Loading…</p>}
            {!guestPortsLoading && running && topPorts.length === 0 && (
              <p className="text-xs text-[var(--text-muted)]">
                {guestPorts && !guestPorts.agent_reachable ? 'Install guest tools for port list.' : 'No listening ports reported.'}
              </p>
            )}
            {!guestPortsLoading && topPorts.length > 0 && (
              <ul className="space-y-1 text-xs font-mono">
                {topPorts.map((p) => {
                  const exposed = natRuleForGuestPort(portForwardRules, p.port)
                  const href = laptopHttpHref(p.port, hypervisorAddress, portForwardRules, ip)
                  return (
                    <li key={`${p.protocol}-${p.port}`} className="flex flex-wrap items-center gap-1">
                      <span className="text-[var(--text-secondary)]">{p.port}/{p.protocol}</span>
                      {exposed ? (
                        <span className="text-emerald-400/80">{exposed.host_port}→{p.port}</span>
                      ) : platformVmId ? (
                        <button
                          type="button"
                          className="text-emerald-400 hover:underline"
                          disabled={exposeBusy === p.port}
                          data-testid={`expose-guest-port-${p.port}`}
                          onClick={() => void exposeGuestPort(p.port)}
                        >
                          {exposeBusy === p.port ? '…' : 'Expose'}
                        </button>
                      ) : null}
                      {href ? (
                        <a href={href} target="_blank" rel="noreferrer" className="text-[var(--link)] hover:underline inline-flex items-center gap-0.5">
                          open <ExternalLink className="w-2.5 h-2.5" />
                        </a>
                      ) : null}
                    </li>
                  )
                })}
              </ul>
            )}
            <div className="flex flex-wrap gap-2">
              {onRefreshPorts && (
                <button type="button" className="text-xs text-[var(--text-muted)] hover:text-[var(--text-primary)]" onClick={onRefreshPorts}>
                  Refresh
                </button>
              )}
              {onAllPorts && (
                <button type="button" className={`text-xs inline-flex items-center gap-0.5 ${hubLinkClasses()}`} onClick={onAllPorts}>
                  All ports <ArrowRight className="w-3 h-3" />
                </button>
              )}
            </div>
          </section>

          {platformVmId && (
            <section className="space-y-2 min-w-0">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Hypervisor NAT</p>
                {!natExpanded && onOpenAccessTab && (
                  <button type="button" className={`text-xs ${hubLinkClasses()}`} onClick={onOpenAccessTab}>
                    Access tab →
                  </button>
                )}
              </div>
              {natExpanded || natOpen ? (
                <VmPortForwardPanel
                  platformVmId={platformVmId}
                  vmName={vmName}
                  guestIp={ip}
                  sshUser={sshUser}
                  hypervisorAddress={hypervisorAddress}
                  disabled={disabled}
                  onNotify={notify}
                />
              ) : (
                <div className="rounded-lg border border-[var(--apple-hairline)]/80 p-3 text-xs text-[var(--text-muted)] space-y-2 bg-[var(--apple-surface)]">
                  <p>{natRuleCount > 0 ? `${natRuleCount} NAT rule(s) active` : 'No NAT rules on this hypervisor yet.'}</p>
                  <button type="button" className="btn-secondary text-xs" onClick={() => setNatOpen(true)}>
                    Show expose panel
                  </button>
                </div>
              )}
            </section>
          )}
        </div>

        {showExport && (
          <section className="space-y-2 border-t border-white/5 pt-3">
            <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Export</p>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={!specJson}
                onClick={() => specJson && downloadText(`${vmName}-spec.json`, specJson, 'application/json')}
              >
                <Download className="w-3 h-3 inline" /> Spec
              </button>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={!onExportXml}
                onClick={() => void exportBundle()}
              >
                <Download className="w-3 h-3 inline" /> Spec + XML
              </button>
            </div>
          </section>
        )}
      </div>

      <VmSshConnectDialog
        open={sshOpen}
        vmName={vmName}
        platformVmId={platformVmId}
        defaultIp={ip}
        defaultUser={sshUser}
        detectedIps={detectedIps.length > 0 ? detectedIps : ip ? [ip] : []}
        hypervisorAddress={hypervisorAddress}
        guestIpPrivate={privateIp}
        portForwardRules={portForwardRules}
        onRefreshPortForwards={onRefreshPortForwards}
        onClose={() => setSshOpen(false)}
        onConnect={openSsh}
        onNotify={notify}
      />
    </>
  )
}

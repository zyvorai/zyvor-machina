// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { Terminal, X } from 'lucide-react'
import { useFocusTrap } from '../../hooks/useFocusTrap'
import { formatUserError } from '../../utils/apiError'
import { loadVmSshPrefs, saveVmSshPrefs } from '../../utils/vmSshPrefs'
import { statusToneClass } from '../../utils/semanticColors'
import {
  exposeGuestPortOnVm,
  laptopSshCommand,
  type NatRuleLike,
  sshNatHostPort,
} from '../../utils/vmPortForwardServices'

interface VmSshConnectDialogProps {
  open: boolean
  vmName: string
  platformVmId?: string
  defaultIp?: string
  defaultUser?: string
  detectedIps?: string[]
  hypervisorAddress?: string
  guestIpPrivate?: boolean
  portForwardRules?: NatRuleLike[]
  onRefreshPortForwards?: () => void
  onClose: () => void
  onConnect?: (host: string, user: string, port?: number) => void
  onNotify?: (message: string) => void
}

export function navigateVmSshSession(vmName: string, host: string, user: string, vmId?: string, port?: number) {
  const h = host.trim()
  const u = user.trim() || 'root'
  if (!h) return
  saveVmSshPrefs(vmName, { host: h, user: u })
  const qs = new URLSearchParams({ host: h, user: u })
  if (vmId) qs.set('vmId', vmId)
  if (vmName) qs.set('vmName', vmName)
  if (port && port !== 22) qs.set('port', String(port))
  window.location.href = `/ssh?${qs.toString()}`
}

export default function VmSshConnectDialog({
  open,
  vmName,
  platformVmId,
  defaultIp = '',
  defaultUser = 'root',
  detectedIps = [],
  hypervisorAddress,
  guestIpPrivate = false,
  portForwardRules = [],
  onRefreshPortForwards,
  onClose,
  onConnect,
  onNotify,
}: VmSshConnectDialogProps) {
  const [ip, setIp] = useState(defaultIp)
  const [user, setUser] = useState(defaultUser)
  const [busy, setBusy] = useState(false)
  const panelRef = useRef<HTMLDivElement>(null)
  useFocusTrap(panelRef, open, onClose)

  const natPort = sshNatHostPort(portForwardRules)
  const useNat = guestIpPrivate && Boolean(hypervisorAddress?.trim())
  const connectHost = useNat ? (hypervisorAddress?.trim() || '') : ip.trim()
  const connectPort = useNat ? natPort : undefined

  useEffect(() => {
    if (!open) return
    const prefs = loadVmSshPrefs(vmName)
    const fromGuest = defaultIp.trim() || detectedIps[0]?.trim() || ''
    setIp(fromGuest || prefs?.host?.trim() || '')
    setUser(defaultUser.trim() || prefs?.user?.trim() || 'root')
  }, [open, vmName, defaultIp, defaultUser, detectedIps])

  if (!open) return null

  const notify = (msg: string) => onNotify?.(msg)

  const connect = () => {
    const h = useNat ? connectHost : ip.trim()
    const u = user.trim() || 'root'
    const p = useNat ? connectPort : undefined
    if (!h) return
    if (onConnect) onConnect(h, u, p)
    else navigateVmSshSession(vmName, h, u, platformVmId, p)
    onClose()
  }

  const exposeAndCopy = async () => {
    if (!platformVmId) return
    setBusy(true)
    try {
      await exposeGuestPortOnVm(platformVmId, vmName, 22, portForwardRules)
      notify('SSH exposed on hypervisor')
      onRefreshPortForwards?.()
      const cmd = laptopSshCommand(user, defaultIp, hypervisorAddress, [
        ...portForwardRules,
        { protocol: 'tcp', host_port: 2222, vm_port: 22 },
      ])
      if (cmd) {
        await navigator.clipboard.writeText(cmd)
        notify('SSH command copied')
      }
    } catch (e: unknown) {
      notify(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      ref={panelRef}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      onClick={onClose}
    >
      <div
        className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-md mx-4"
        data-testid="vm-ssh-connect-dialog"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between">
          <span className="text-lg font-semibold flex items-center gap-2">
            <Terminal className={`w-5 h-5 ${statusToneClass('ok')}`} /> SSH — {vmName}
          </span>
          <button type="button" onClick={onClose} className="p-1 hover:bg-[var(--surface-hover)] rounded transition" aria-label="Close">
            <X className="w-4 h-4 text-[var(--text-muted)]" />
          </button>
        </div>
        <div className="p-5 space-y-3">
          {useNat ? (
            <div className="rounded-lg border border-amber-500/25 bg-amber-950/20 px-3 py-2 text-xs text-amber-100/90 space-y-2" data-testid="vm-ssh-nat-banner">
              <p>Guest IP is on hypervisor NAT — connect via the hypervisor host{connectPort ? ` port ${connectPort}` : ''}.</p>
              {!natPort && platformVmId ? (
                <button type="button" className="btn-primary text-xs" disabled={busy} onClick={() => void exposeAndCopy()}>
                  {busy ? 'Exposing…' : 'Expose SSH & copy command'}
                </button>
              ) : null}
              {natPort && hypervisorAddress ? (
                <p className="font-mono text-emerald-700/90 break-all">
                  {laptopSshCommand(user, defaultIp, hypervisorAddress, portForwardRules)}
                </p>
              ) : null}
            </div>
          ) : null}
          {!useNat ? (
            <>
              <label htmlFor="vm-ssh-ip" className="block text-sm text-[var(--text-muted)] mb-1">
                Guest IP (guest agent first; edit if needed)
              </label>
              <input
                id="vm-ssh-ip"
                type="text"
                autoFocus
                value={ip}
                onChange={(e) => setIp(e.target.value)}
                placeholder="192.168.122.100"
                className="input w-full"
                onKeyDown={(e) => { if (e.key === 'Enter' && ip.trim()) connect() }}
              />
            </>
          ) : (
            <p className="text-sm text-[var(--text-secondary)]">
              Target: <span className="font-mono text-emerald-600">{connectHost}{connectPort ? `:${connectPort}` : ''}</span>
            </p>
          )}
          <label htmlFor="vm-ssh-user" className="block text-sm text-[var(--text-muted)] mb-1 mt-3">
            SSH user
          </label>
          <input
            id="vm-ssh-user"
            type="text"
            value={user}
            onChange={(e) => setUser(e.target.value)}
            placeholder="ubuntu"
            className="input w-full"
            autoComplete="username"
          />
          {detectedIps.length > 0 && !useNat && (
            <div>
              <span className="text-xs text-[var(--text-muted)]">Detected IPs:</span>
              <div className="flex flex-wrap gap-1 mt-1">
                {detectedIps.map((addr) => (
                  <button
                    key={addr}
                    type="button"
                    onClick={() => setIp(addr)}
                    className="px-2 py-0.5 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded text-xs font-mono hover:bg-[var(--surface-hover)] transition text-[var(--link)]"
                  >
                    {addr}
                  </button>
                ))}
              </div>
            </div>
          )}
          <p className="text-xs text-[var(--text-muted)]">
            In-browser SSH uses hypervisor keys. From your laptop use the copied command with your cloud-init private key.
          </p>
        </div>
        <div className="flex justify-end gap-3 px-5 pb-5">
          <button type="button" onClick={onClose} className="btn-secondary text-sm">Cancel</button>
          <button
            type="button"
            onClick={connect}
            disabled={!connectHost || (useNat && !natPort)}
            className="btn-primary text-sm"
          >
            Connect
          </button>
        </div>
      </div>
    </div>
  )
}

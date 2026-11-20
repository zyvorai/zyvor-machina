// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Copy, ExternalLink, Trash2 } from 'lucide-react'
import {
  createVmPortForward,
  deleteVmPortForward,
  listVmPortForwards,
  listVmPortForwardTemplates,
  upsertVmPortForwardTemplate,
  type VmPortForwardRule,
} from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import {
  deleteCustomPortForwardService,
  inferAccessFromPorts,
  KNOWN_PORT_FORWARD_SERVICES,
  loadCustomPortForwardServices,
  newCustomServiceId,
  resolveServiceForRule,
  ruleMatchesService,
  saveCustomPortForwardService,
  serviceAccessHref,
  serviceAccessLabel,
  suggestHostPort,
  publicHostname,
  type PortForwardAccessKind,
  type PortForwardServiceTemplate,
} from '../../utils/vmPortForwardServices'

interface VmPortForwardPanelProps {
  platformVmId: string
  vmName: string
  guestIp: string
  sshUser?: string
  hypervisorAddress?: string
  disabled?: boolean
  onNotify?: (message: string) => void
  className?: string
  compact?: boolean
}

export default function VmPortForwardPanel({
  platformVmId,
  vmName,
  guestIp,
  sshUser = 'ubuntu',
  hypervisorAddress,
  disabled = false,
  onNotify,
  className = '',
  compact = false,
}: VmPortForwardPanelProps) {
  const [rules, setRules] = useState<VmPortForwardRule[]>([])
  const [customServices, setCustomServices] = useState<PortForwardServiceTemplate[]>([])
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [customName, setCustomName] = useState('')
  const [customGuestPort, setCustomGuestPort] = useState('')
  const [customHostPort, setCustomHostPort] = useState('')
  const [customAccess, setCustomAccess] = useState<PortForwardAccessKind>('tcp')
  const [manualHostPort, setManualHostPort] = useState('')
  const [manualGuestPort, setManualGuestPort] = useState('')
  const ip = guestIp.trim()

  const notify = (message: string) => onNotify?.(message)

  const takenHostPorts = useMemo(() => rules.map((r) => r.host_port), [rules])

  const catalog = useMemo(
    () => [...KNOWN_PORT_FORWARD_SERVICES, ...customServices],
    [customServices],
  )

  const load = useCallback(async () => {
    if (!platformVmId || !ip) {
      setRules([])
      return
    }
    setLoading(true)
    try {
      setRules(await listVmPortForwards(platformVmId))
    } catch {
      setRules([])
    } finally {
      setLoading(false)
    }
  }, [platformVmId, ip])

  const loadCustomTemplates = useCallback(async () => {
    try {
      const server = await listVmPortForwardTemplates(platformVmId)
      if (server.length > 0) {
        setCustomServices(
          server.map((t) => ({
            id: t.id,
            name: t.name,
            vmPort: t.vm_port,
            hostPort: t.host_port,
            access: t.access as PortForwardAccessKind,
          })),
        )
        return
      }
    } catch {
      /* fall back to browser storage */
    }
    setCustomServices(loadCustomPortForwardServices(platformVmId))
  }, [platformVmId])

  useEffect(() => {
    void loadCustomTemplates()
    void load()
  }, [platformVmId, load, loadCustomTemplates])

  const createRule = async (
    host: number,
    guest: number,
    description: string,
    saveTemplate?: PortForwardServiceTemplate,
  ) => {
    setBusy(true)
    try {
      await createVmPortForward(platformVmId, {
        protocol: 'tcp',
        host_port: host,
        vm_port: guest,
        description,
      })
      if (saveTemplate) {
        try {
          const saved = await upsertVmPortForwardTemplate(platformVmId, {
            id: saveTemplate.id,
            name: saveTemplate.name,
            vm_port: saveTemplate.vmPort,
            host_port: saveTemplate.hostPort,
            access: saveTemplate.access,
          })
          setCustomServices(
            saved.map((t) => ({
              id: t.id,
              name: t.name,
              vmPort: t.vm_port,
              hostPort: t.host_port,
              access: t.access as PortForwardAccessKind,
            })),
          )
        } catch {
          setCustomServices(saveCustomPortForwardService(platformVmId, saveTemplate))
        }
      }
      notify('NAT rule created on hypervisor')
      await load()
    } catch (e: unknown) {
      notify(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const exposeService = async (service: PortForwardServiceTemplate) => {
    await createRule(service.hostPort, service.vmPort, `${vmName}:${service.name}`)
  }

  const onCustomGuestPortChange = (value: string) => {
    setCustomGuestPort(value)
    const guest = parseInt(value, 10)
    if (!Number.isFinite(guest) || guest <= 0) return
    setCustomHostPort(String(suggestHostPort(guest, takenHostPorts)))
    setCustomAccess(inferAccessFromPorts(guest, true))
  }

  const exposeCustom = async () => {
    const name = customName.trim()
    const guest = parseInt(customGuestPort, 10)
    const host = parseInt(customHostPort, 10)
    if (!name) {
      notify('Name your service (e.g. API, Jenkins, game server)')
      return
    }
    if (!Number.isFinite(guest) || !Number.isFinite(host) || guest <= 0 || host <= 0) {
      notify('Enter valid guest and host ports')
      return
    }
    const template: PortForwardServiceTemplate = {
      id: newCustomServiceId(name),
      name,
      vmPort: guest,
      hostPort: host,
      access: customAccess,
    }
    await createRule(host, guest, `${vmName}:${name}`, template)
    setCustomName('')
    setCustomGuestPort('')
    setCustomHostPort('')
  }

  const exposeManual = async () => {
    const guest = parseInt(manualGuestPort, 10)
    const host = parseInt(manualHostPort, 10)
    if (!Number.isFinite(guest) || !Number.isFinite(host)) {
      notify('Enter valid port numbers')
      return
    }
    await createRule(host, guest, `${vmName}:manual-${guest}`)
  }

  const remove = async (rule: VmPortForwardRule) => {
    setBusy(true)
    try {
      await deleteVmPortForward(platformVmId, {
        protocol: rule.protocol,
        host_port: rule.host_port,
        vm_port: rule.vm_port,
      })
      notify('NAT rule removed')
      await load()
    } catch (e: unknown) {
      notify(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const removeCustomTemplate = (id: string) => {
    setCustomServices(deleteCustomPortForwardService(platformVmId, id))
    notify('Removed saved custom service')
  }

  const copyText = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text)
      notify('Copied access URL')
    } catch {
      notify('Could not copy to clipboard')
    }
  }

  if (!ip) {
    return (
      <p className={`text-sm text-[var(--text-muted)] ${className}`} data-testid="vm-port-forward-panel">
        Guest IP required — start the VM and install guest tools to manage hypervisor NAT rules.
      </p>
    )
  }

  return (
    <div className={`space-y-4 ${className}`} data-testid="vm-port-forward-panel">
      <p className="text-xs text-[var(--text-muted)]">
        Expose any guest TCP service on the hypervisor. From your laptop use{' '}
        <span className="font-mono text-[var(--text-muted)]">{publicHostname(hypervisorAddress) || 'hypervisor-ip'}:host-port</span> →{' '}
        <span className="font-mono text-[var(--text-muted)]">{ip}:guest-port</span>.
      </p>

      {!compact ? (
        <>
      <section className="space-y-2">
        <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Known services</p>
        <div className="flex flex-wrap gap-2">
          {KNOWN_PORT_FORWARD_SERVICES.map((service) => {
            const active = rules.some((rule) => ruleMatchesService(rule, service))
            return (
              <button
                key={service.id}
                type="button"
                className={active ? 'btn-secondary text-xs opacity-80' : 'btn-primary text-xs'}
                disabled={busy || disabled || active}
                data-testid={`expose-service-${service.id}`}
                title={`${service.name}: host ${service.hostPort} → guest ${service.vmPort}`}
                onClick={() => void exposeService(service)}
              >
                {active ? `${service.name} ✓` : service.name}
              </button>
            )
          })}
        </div>
      </section>

      {customServices.length > 0 && (
        <section className="space-y-2">
          <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Your saved services</p>
          <div className="flex flex-wrap gap-2">
            {customServices.map((service) => {
              const active = rules.some((rule) => ruleMatchesService(rule, service))
              return (
                <span key={service.id} className="inline-flex items-center gap-1">
                  <button
                    type="button"
                    className={active ? 'btn-secondary text-xs opacity-80' : 'btn-secondary text-xs'}
                    disabled={busy || disabled || active}
                    data-testid={`expose-custom-${service.id}`}
                    onClick={() => void exposeService(service)}
                  >
                    {active ? `${service.name} ✓` : service.name}
                  </button>
                  <button
                    type="button"
                    className="text-[var(--text-muted)] hover:text-red-400 p-0.5"
                    aria-label={`Remove saved ${service.name}`}
                    onClick={() => removeCustomTemplate(service.id)}
                  >
                    <Trash2 className="w-3 h-3" />
                  </button>
                </span>
              )
            })}
          </div>
        </section>
      )}

      <section className="space-y-2 rounded-lg border border-[var(--apple-hairline)]/80 p-3">
        <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Custom service</p>
        <div className="flex flex-wrap gap-2 items-end text-sm">
          <label className="space-y-1 min-w-[8rem]">
            <span className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Name</span>
            <input
              className="input py-1 text-xs"
              value={customName}
              onChange={(e) => setCustomName(e.target.value)}
              placeholder="My API"
              aria-label="Service name"
              data-testid="custom-service-name"
            />
          </label>
          <label className="space-y-1">
            <span className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Guest port</span>
            <input
              className="input w-20 py-1 text-xs font-mono"
              value={customGuestPort}
              onChange={(e) => onCustomGuestPortChange(e.target.value)}
              placeholder="3000"
              aria-label="Custom guest port"
              data-testid="custom-service-guest-port"
            />
          </label>
          <label className="space-y-1">
            <span className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Host port</span>
            <input
              className="input w-20 py-1 text-xs font-mono"
              value={customHostPort}
              onChange={(e) => setCustomHostPort(e.target.value)}
              placeholder="auto"
              aria-label="Custom host port"
              data-testid="custom-service-host-port"
            />
          </label>
          <label className="space-y-1">
            <span className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Access</span>
            <select
              className="input py-1 text-xs"
              value={customAccess}
              onChange={(e) => setCustomAccess(e.target.value as PortForwardAccessKind)}
              aria-label="Access type"
            >
              <option value="tcp">TCP</option>
              <option value="http">HTTP</option>
              <option value="https">HTTPS</option>
              <option value="ssh">SSH</option>
            </select>
          </label>
          <button
            type="button"
            className="btn-primary text-xs"
            disabled={busy || disabled}
            data-testid="expose-custom-service"
            onClick={() => void exposeCustom()}
          >
            Expose &amp; save
          </button>
        </div>
      </section>

      <details className="text-xs text-[var(--text-muted)]">
        <summary className="cursor-pointer text-[var(--text-muted)] hover:text-[var(--text-secondary)]">Manual port mapping</summary>
        <div className="flex flex-wrap gap-2 items-end text-sm mt-2">
          <label className="space-y-1">
            <span className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Host port</span>
            <input
              className="input w-20 py-1 text-xs font-mono"
              value={manualHostPort}
              onChange={(e) => setManualHostPort(e.target.value)}
              aria-label="Manual host port"
            />
          </label>
          <span className="text-[var(--text-muted)] pb-1">→ guest</span>
          <label className="space-y-1">
            <span className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Guest port</span>
            <input
              className="input w-16 py-1 text-xs font-mono"
              value={manualGuestPort}
              onChange={(e) => setManualGuestPort(e.target.value)}
              aria-label="Manual guest port"
            />
          </label>
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={busy || disabled}
            onClick={() => void exposeManual()}
          >
            Expose
          </button>
        </div>
      </details>
        </>
      ) : (
        <div className="flex flex-wrap gap-2">
          {KNOWN_PORT_FORWARD_SERVICES.filter((s) => s.id === 'ssh' || s.id === 'http').map((service) => {
            const active = rules.some((rule) => ruleMatchesService(rule, service))
            return (
              <button
                key={service.id}
                type="button"
                className={active ? 'btn-secondary text-xs opacity-80' : 'btn-primary text-xs'}
                disabled={busy || disabled || active}
                data-testid={`expose-service-${service.id}`}
                onClick={() => void exposeService(service)}
              >
                {active ? `${service.name} ✓` : `Expose ${service.name}`}
              </button>
            )
          })}
        </div>
      )}

      {rules.length > 0 && (
        <section className="space-y-2">
          <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Active exposure</p>
          <ul className="text-xs space-y-2 rounded-lg border border-emerald-500/20 bg-[var(--apple-surface)] p-3">
            {rules.map((rule) => {
              const service = resolveServiceForRule(rule, catalog)
              if (!service) return null
              const access = serviceAccessLabel(service, sshUser, hypervisorAddress)
              const href = serviceAccessHref(service, hypervisorAddress)
              return (
                <li key={rule.id} className="flex flex-wrap items-center gap-2">
                  <span className="text-emerald-700/90 font-medium">{service.name}</span>
                  <span className="text-[var(--text-muted)] font-mono">
                    {rule.host_port}→{rule.vm_port}
                  </span>
                  <code className="font-mono text-emerald-800/90 break-all">{access}</code>
                  <button
                    type="button"
                    className="text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                    aria-label={`Copy ${service.name} access`}
                    onClick={() => void copyText(access)}
                  >
                    <Copy className="w-3.5 h-3.5" />
                  </button>
                  {href ? (
                    <a
                      href={href}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-[var(--link)] hover:text-[var(--link)] inline-flex items-center gap-0.5"
                    >
                      open <ExternalLink className="w-3 h-3" />
                    </a>
                  ) : null}
                  <button
                    type="button"
                    className="text-red-400/90 hover:underline ml-auto"
                    disabled={busy || disabled}
                    onClick={() => void remove(rule)}
                  >
                    remove
                  </button>
                </li>
              )
            })}
          </ul>
        </section>
      )}

      {loading && <p className="text-xs text-[var(--text-muted)]">Loading rules…</p>}
      {!loading && rules.length === 0 && !compact && (
        <p className="text-xs text-[var(--text-muted)]">No NAT rules on this hypervisor yet.</p>
      )}
    </div>
  )
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Cloud project networking: default isolation between projects,
// per-project egress allowlists and egress IPs. Settings become generated
// policies (and SNAT rules on the hosts) on the controller.

import { Fragment, useCallback, useEffect, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { Empty, Field, headRowCls, rowCls, thCls } from '../bpf/shared'
import {
  describeEgress,
  evidenceFilename,
  getEvidence,
  isProjectPending,
  listEgressIps,
  listProjects,
  previewProject,
  resetProject,
  setProject,
  splitPorts,
  type EgressHostStatus,
  type EvidenceFormat,
  type ProjectIsolation,
  type ProjectNet,
  type ProjectRow,
} from '../../api/vmNetpol'
import { listPlatformVms, patchVm, type PlatformVm } from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import { downloadText } from '../../utils/export'
import { useToastContext } from '../../contexts/ToastContext'
import { statusPillClasses, statusToneClass } from '../../utils/semanticColors'

function body(s: ProjectNet): Omit<ProjectNet, 'project'> {
  return {
    isolation: s.isolation,
    allow_host: s.allow_host,
    egress_restricted: s.egress_restricted,
    egress_allow: s.egress_allow,
    egress_ips: s.egress_ips,
    egress_ip_required: s.egress_ip_required ?? false,
  }
}

interface MatrixCell {
  from: string
  to: string
  allowed: string[]
  denied: string[]
}

function ProjectMatrix({ project }: { project: string }) {
  const toast = useToastContext()
  const [cells, setCells] = useState<MatrixCell[] | null>(null)
  const [probes, setProbes] = useState('')
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const e = JSON.parse(await getEvidence('fleet', 'json', { project, probes })) as { matrix?: MatrixCell[] }
      setCells(e.matrix ?? [])
      setError(null)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [project, probes])

  useEffect(() => { void load() }, [load])

  const exportEvidence = async (format: EvidenceFormat) => {
    try {
      const text = await getEvidence('fleet', format, { project, probes })
      downloadText(text, evidenceFilename(format, new Date(), project), format === 'md' ? 'text/markdown' : 'application/json')
      toast.success(`Evidence for ${project} downloaded`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <div className="space-y-2" aria-label={`Segmentation of ${project}`}>
      <div className="flex flex-wrap items-end gap-2">
        <Field label="Probes" htmlFor={`pm-probes-${project}`}>
          <input
            id={`pm-probes-${project}`}
            className="input text-sm w-48 font-mono"
            placeholder="tcp/22, tcp/443"
            defaultValue={probes}
            onBlur={(e) => setProbes(e.target.value)}
          />
        </Field>
        <button type="button" className="btn-secondary text-xs" aria-label={`Export evidence of ${project} as JSON`} onClick={() => void exportEvidence('json')}>Evidence JSON</button>
        <button type="button" className="btn-secondary text-xs" aria-label={`Export evidence of ${project} as Markdown`} onClick={() => void exportEvidence('md')}>Evidence Markdown</button>
      </div>
      {error ? (
        <div className={statusToneClass('error')}>{error}</div>
      ) : cells === null ? (
        <Empty>Loading…</Empty>
      ) : cells.length === 0 ? (
        <Empty>No VMs to trace.</Empty>
      ) : (
        <table className="text-xs" aria-label={`Matrix of ${project}`}>
          <thead>
            <tr className={headRowCls}>
              <th scope="col" className={thCls}>From</th>
              <th scope="col" className={thCls}>To</th>
              <th scope="col" className={thCls}>Allowed</th>
            </tr>
          </thead>
          <tbody>
            {cells.map((c) => (
              <tr key={`${c.from}>${c.to}`} className={rowCls}>
                <td className="py-1 pr-3">{c.from}</td>
                <td className="py-1 pr-3">{c.to}</td>
                <td className="py-1 pr-3">
                  <span className={statusPillClasses(c.allowed.length === 0 ? 'ok' : c.from === c.to ? 'neutral' : 'warn')}>
                    {c.allowed.length === 0 ? 'Segmented' : c.allowed.join(', ')}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}

export default function ProjectsPanel({ onChanged }: { onChanged: () => void }) {
  const toast = useToastContext()
  const [def, setDef] = useState<ProjectNet | null>(null)
  const [rows, setRows] = useState<ProjectRow[]>([])
  const [warnings, setWarnings] = useState<string[]>([])
  const [hosts, setHosts] = useState<EgressHostStatus[]>([])
  const [vms, setVms] = useState<PlatformVm[]>([])
  const [open, setOpen] = useState<string | null>(null)
  const [dest, setDest] = useState({ to: '', ports: '' })
  const [eip, setEip] = useState({ host: '', ip: '' })
  const [assign, setAssign] = useState({ vm: '', project: '' })
  const [previewFirst, setPreviewFirst] = useState(false)
  const [propose, setPropose] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const r = await listProjects()
      setDef(r.default)
      setRows(r.items)
      setWarnings(r.warnings ?? [])
      setError(null)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
    try {
      setHosts((await listEgressIps()).items)
    } catch {
      setHosts([])
    }
    try {
      setVms(await listPlatformVms())
    } catch {
      setVms([])
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const save = async (project: string, s: ProjectNet, msg: string) => {
    setBusy(true)
    try {
      if (previewFirst) {
        const p = await previewProject(project, body(s))
        const breaking = p.replay.would_break.length
        const text = `${p.describe}\n\nAgainst recorded traffic this ${p.summary}.${breaking ? '\n\nApply anyway?' : ''}`
        if (!window.confirm(text)) return
      }
      const r = await setProject(project, body(s), propose)
      if (isProjectPending(r)) {
        toast.success(`Sent for approval: ${r.preview}`)
      } else {
        toast.success(msg)
      }
      await load()
      onChanged()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const reset = async (project: string) => {
    if (!window.confirm(`Drop every network setting of project ${project}? It follows the default again.`)) return
    try {
      const r = await resetProject(project, propose)
      toast.success(isProjectPending(r) ? `Sent for approval: ${r.preview}` : `Project ${project} follows the default`)
      await load()
      onChanged()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const moveVm = async (vmName: string, project: string) => {
    const vm = vms.find((v) => v.name === vmName)
    if (!vm) {
      toast.error(`No VM named ${vmName}`)
      return
    }
    setBusy(true)
    try {
      await patchVm(vm.id, { project })
      toast.success(project ? `${vmName} is in project ${project}` : `${vmName} left its project`)
      await load()
      onChanged()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  if (error) return <MacGlassPanel title="Projects"><div className={statusToneClass('error')}>{error}</div></MacGlassPanel>
  if (!def) return <MacGlassPanel title="Projects"><Empty>Loading…</Empty></MacGlassPanel>

  const hostNames = hosts.map((h) => h.hostname)

  return (
    <div className="space-y-4">
      {warnings.length > 0 && (
        <MacGlassPanel title="Warnings" subtitle="What isolation cannot see or enforce as configured.">
          <ul className="space-y-1 text-sm" aria-label="Project warnings">
            {warnings.map((w) => <li key={w} className={statusToneClass('warn')}>{w}</li>)}
          </ul>
        </MacGlassPanel>
      )}

      <MacGlassPanel
        title="Default project isolation"
        subtitle="An isolated project's VMs accept connections only from VMs of the same project (and the host, if allowed). Projects follow this default unless set below. Ordinary allow policies still open exceptions; drops need the enforcement lease."
      >
        <div className="flex flex-wrap items-center gap-3">
          <select
            aria-label="Default isolation"
            className="input text-sm"
            value={def.isolation}
            disabled={busy}
            onChange={(e) => void save('*', { ...def, isolation: e.target.value as ProjectIsolation }, `Projects are ${e.target.value} by default`)}
          >
            <option value="open">Open by default</option>
            <option value="isolated">Isolated by default</option>
          </select>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              aria-label="Default allows the host"
              checked={def.allow_host}
              disabled={busy || def.isolation !== 'isolated'}
              onChange={(e) => void save('*', { ...def, allow_host: e.target.checked }, e.target.checked ? 'The host can reach isolated projects' : 'The host is blocked from isolated projects')}
            />
            Host may connect
          </label>
          <span className="flex-1" />
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" aria-label="Preview changes" checked={previewFirst} onChange={(e) => setPreviewFirst(e.target.checked)} />
            Preview against recorded traffic
          </label>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" aria-label="Ask a second admin" checked={propose} onChange={(e) => setPropose(e.target.checked)} />
            Ask a second admin
          </label>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title="Projects" subtitle="Isolation, egress allowlist and egress IPs per Fleet Cloud project.">
        <div className="flex flex-wrap items-end gap-2 mb-3" aria-label="Assign a VM">
          <Field label="VM" htmlFor="pj-vm">
            <select id="pj-vm" className="input text-sm w-48" value={assign.vm} onChange={(e) => setAssign({ ...assign, vm: e.target.value })}>
              <option value="">Choose a VM…</option>
              {vms.map((v) => <option key={v.id} value={v.name}>{v.name}{v.project ? ` (${v.project})` : ''}</option>)}
            </select>
          </Field>
          <Field label="Project" htmlFor="pj-project">
            <input id="pj-project" list="pj-projects" className="input text-sm w-40" placeholder="new or existing" value={assign.project} onChange={(e) => setAssign({ ...assign, project: e.target.value })} />
          </Field>
          <datalist id="pj-projects">{rows.map((r) => <option key={r.project} value={r.project} />)}</datalist>
          <button
            type="button"
            className="btn-primary text-sm"
            disabled={busy || !assign.vm || !assign.project.trim()}
            onClick={() => {
              const a = assign
              setAssign({ vm: '', project: '' })
              void moveVm(a.vm, a.project.trim())
            }}
          >
            Assign
          </button>
        </div>
        {rows.length === 0 ? (
          <Empty>No projects yet. Assign a VM above or give it a project in Fleet Cloud.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="Project networking">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Project</th>
                  <th scope="col" className={thCls}>VMs</th>
                  <th scope="col" className={thCls}>Isolation</th>
                  <th scope="col" className={thCls}>Egress</th>
                  <th scope="col" className={thCls}>Egress IPs</th>
                  <th scope="col" className="py-2"><span className="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const s = r.settings
                  const ips = Object.entries(s.egress_ips)
                  const gaps = r.egress_gaps ?? []
                  return (
                    <Fragment key={r.project}>
                      <tr className={rowCls}>
                        <td className="py-2 pr-2 font-medium">
                          <span>{r.project}</span>
                          {r.cross_host_nat && (
                            <span
                              className={`ml-2 ${statusPillClasses('warn')}`}
                              title={`VMs on ${r.cross_host_nat.hosts.join(', ')} behind per-host NAT (${r.cross_host_nat.subnets.join(', ')}): cross-host traffic arrives as the host and isolation cannot tell VMs apart.`}
                            >
                              Cross-host NAT
                            </span>
                          )}
                        </td>
                        <td className="py-2 pr-2" title={r.vms.join(', ')}>{r.vms.length}</td>
                        <td className="py-2 pr-2">
                          <select
                            aria-label={`Isolation of ${r.project}`}
                            className="input text-xs py-1"
                            value={s.isolation}
                            disabled={busy}
                            onChange={(e) => void save(r.project, { ...s, isolation: e.target.value as ProjectIsolation }, `Project ${r.project}: ${e.target.value}`)}
                          >
                            <option value="inherit">Default ({def.isolation})</option>
                            <option value="isolated">Isolated</option>
                            <option value="open">Open</option>
                          </select>
                          <span className={`ml-2 ${statusPillClasses(r.isolated ? 'ok' : 'neutral')}`}>{r.isolated ? (r.allow_host ? 'Isolated' : 'Isolated, no host') : 'Open'}</span>
                        </td>
                        <td className="py-2 pr-2 max-w-[280px] truncate" title={describeEgress(s)}>{describeEgress(s)}</td>
                        <td className="py-2 pr-2 font-mono">
                          {ips.length ? ips.map(([h, ip]) => `${ip} @ ${h}`).join(', ') : '—'}
                          {gaps.length > 0 && (
                            <span
                              className={`ml-2 font-sans ${statusPillClasses(s.egress_ip_required ? 'error' : 'warn')}`}
                              title={gaps.map((g) => `${g.vm} on ${g.host}`).join(', ')}
                            >
                              {gaps.length} VM{gaps.length === 1 ? '' : 's'} {s.egress_ip_required ? 'blocked' : 'without it'}
                            </span>
                          )}
                        </td>
                        <td className="py-2 text-right whitespace-nowrap">
                          <button type="button" className="btn-secondary text-xs mr-2" onClick={() => setOpen(open === r.project ? null : r.project)}>
                            {open === r.project ? 'Close' : 'Manage…'}
                          </button>
                          {r.explicit && <button type="button" className="btn-secondary text-xs" onClick={() => void reset(r.project)}>Reset</button>}
                        </td>
                      </tr>
                      {open === r.project && (
                        <tr className={rowCls}>
                          <td colSpan={6} className="py-3">
                            <div className="space-y-4">
                              <div className="space-y-1" aria-label={`VMs of ${r.project}`}>
                                <div className="font-medium text-sm">VMs</div>
                                {r.vms.length === 0 ? (
                                  <Empty>No VMs.</Empty>
                                ) : (
                                  <ul className="flex flex-wrap gap-2">
                                    {r.vms.map((v) => (
                                      <li key={v} className="flex items-center gap-1 font-mono">
                                        {v}
                                        <button
                                          type="button"
                                          className="text-[var(--accent,#0071e3)] hover:underline font-sans"
                                          aria-label={`Remove ${v} from ${r.project}`}
                                          disabled={busy}
                                          onClick={() => void moveVm(v, '')}
                                        >
                                          Remove
                                        </button>
                                      </li>
                                    ))}
                                  </ul>
                                )}
                              </div>

                              <div className="space-y-3" aria-label={`Egress of ${r.project}`}>
                                <label className="flex items-center gap-2 text-sm">
                                  <input
                                    type="checkbox"
                                    aria-label={`Limit egress of ${r.project}`}
                                    checked={s.egress_restricted}
                                    disabled={busy}
                                    onChange={(e) => void save(r.project, { ...s, egress_restricted: e.target.checked }, e.target.checked ? `Project ${r.project}: egress limited to the allowlist` : `Project ${r.project}: egress unrestricted`)}
                                  />
                                  Limit egress to the allowlist (plus the project's VMs, the host and DNS)
                                </label>
                                {s.egress_allow.length > 0 && (
                                  <ul className="space-y-1">
                                    {s.egress_allow.map((e) => (
                                      <li key={e.to} className="flex items-center gap-2 font-mono">
                                        {e.to}{e.ports?.length ? ` · ${e.ports.join(', ')}` : ' · any port'}
                                        <button
                                          type="button"
                                          className="text-[var(--accent,#0071e3)] hover:underline font-sans"
                                          onClick={() => void save(r.project, { ...s, egress_allow: s.egress_allow.filter((x) => x.to !== e.to) }, `Removed ${e.to}`)}
                                        >
                                          Remove
                                        </button>
                                      </li>
                                    ))}
                                  </ul>
                                )}
                                <div className="flex flex-wrap items-end gap-2">
                                  <Field label="Destination" htmlFor="eg-to">
                                    <input id="eg-to" className="input text-sm w-56" placeholder="203.0.113.0/24, api.example.com, world" value={dest.to} onChange={(e) => setDest({ ...dest, to: e.target.value })} />
                                  </Field>
                                  <Field label="Ports" htmlFor="eg-ports">
                                    <input id="eg-ports" className="input text-sm w-32" placeholder="443, 53/udp" value={dest.ports} onChange={(e) => setDest({ ...dest, ports: e.target.value })} />
                                  </Field>
                                  <button
                                    type="button"
                                    className="btn-primary text-sm"
                                    disabled={busy || dest.to.trim() === ''}
                                    onClick={() => {
                                      const to = dest.to.trim()
                                      const next = [...s.egress_allow.filter((x) => x.to !== to), { to, ports: splitPorts(dest.ports) }]
                                      setDest({ to: '', ports: '' })
                                      void save(r.project, { ...s, egress_restricted: true, egress_allow: next }, `Project ${r.project}: allowed ${to}`)
                                    }}
                                  >
                                    Allow
                                  </button>
                                </div>
                                <div className="flex flex-wrap items-end gap-2">
                                  <Field label="Egress IP host" htmlFor="eg-host">
                                    <input id="eg-host" list="eg-hosts" className="input text-sm w-40" value={eip.host} onChange={(e) => setEip({ ...eip, host: e.target.value })} />
                                  </Field>
                                  <Field label="Egress IP" htmlFor="eg-ip">
                                    <input id="eg-ip" className="input text-sm w-56 font-mono" placeholder="203.0.113.10, 2001:db8::10" value={eip.ip} onChange={(e) => setEip({ ...eip, ip: e.target.value })} />
                                  </Field>
                                  <button
                                    type="button"
                                    className="btn-secondary text-sm"
                                    disabled={busy || !eip.host.trim() || !eip.ip.trim()}
                                    onClick={() => {
                                      const next = { ...s.egress_ips, [eip.host.trim()]: eip.ip.trim() }
                                      setEip({ host: '', ip: '' })
                                      void save(r.project, { ...s, egress_ips: next }, `Project ${r.project}: egress IP set`)
                                    }}
                                  >
                                    Set egress IP
                                  </button>
                                  {ips.map(([h]) => (
                                    <button
                                      key={h}
                                      type="button"
                                      className="text-xs text-[var(--accent,#0071e3)] hover:underline"
                                      onClick={() => {
                                        const next = { ...s.egress_ips }
                                        delete next[h]
                                        void save(r.project, { ...s, egress_ips: next }, `Project ${r.project}: egress IP on ${h} removed`)
                                      }}
                                    >
                                      Remove {h}
                                    </button>
                                  ))}
                                  <datalist id="eg-hosts">{hostNames.map((h) => <option key={h} value={h} />)}</datalist>
                                </div>
                                <label className="flex items-center gap-2 text-sm">
                                  <input
                                    type="checkbox"
                                    aria-label={`Require the egress IP of ${r.project}`}
                                    checked={s.egress_ip_required ?? false}
                                    disabled={busy || ips.length === 0}
                                    onChange={(e) => void save(r.project, { ...s, egress_ip_required: e.target.checked }, e.target.checked ? `Project ${r.project}: internet egress blocked on hosts without its egress IP` : `Project ${r.project}: hosts without its egress IP use their own address`)}
                                  />
                                  Block internet egress on hosts without one of these egress IPs
                                </label>
                                {gaps.length > 0 && (
                                  <p className={statusToneClass(s.egress_ip_required ? 'error' : 'warn')}>
                                    {gaps.map((g) => `${g.vm} on ${g.host}`).join(', ')} {s.egress_ip_required ? 'cannot reach the internet' : 'leave with the host\'s address'}: no egress IP for that host.
                                  </p>
                                )}
                                <p className="text-[var(--text-muted)]">One IPv4, one IPv6, or both (comma-separated) per host; each must already be configured on that host. Private, link-local, CGNAT, ULA and multicast destinations keep the VM's own address.</p>
                              </div>

                              <ProjectMatrix project={r.project} />
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  )
                })}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
      </MacGlassPanel>

      <MacGlassPanel title="Egress IPs on hosts" subtitle="What each host's machina-bpfd installed (nftables tables ip/ip6 machina_egress).">
        {hosts.length === 0 ? (
          <Empty>No host reported.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="Egress IPs on hosts">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Host</th>
                  <th scope="col" className={thCls}>State</th>
                  <th scope="col" className={thCls}>Rules</th>
                  <th scope="col" className={thCls}>Notes</th>
                </tr>
              </thead>
              <tbody>
                {hosts.map((h) => (
                  <tr key={h.hostname} className={rowCls}>
                    <td className="py-2 pr-2 font-medium">{h.hostname}</td>
                    <td className="py-2 pr-2"><span className={statusPillClasses(h.error ? 'error' : h.active ? 'ok' : 'neutral')}>{h.error ? 'Error' : h.active ? 'Active' : 'None'}</span></td>
                    <td className="py-2 pr-2 font-mono">{h.rules.length ? h.rules.map((x) => `${x.project}: ${x.sources.length} VM address(es) → ${x.egress_ip}`).join('; ') : '—'}</td>
                    <td className="py-2 pr-2">{[...h.skipped, ...(h.error ? [h.error] : [])].join('; ') || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
      </MacGlassPanel>
    </div>
  )
}

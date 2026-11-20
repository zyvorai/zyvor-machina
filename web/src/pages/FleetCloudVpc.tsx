// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import * as cloud from '../api/cloud'
import { listProjectRegistry, type NativeProject } from '../api/nativeProjects'
import { platformFetch } from '../api/platform'
import PageLayout from '../components/PageLayout'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import GroupAutoscale from '../components/cloud/GroupAutoscale'
import { listLoadBalancers, type NativeLoadBalancer } from '../api/nativeLoadBalancers'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'

type Host = { id: string; hostname: string; state: string }

// 44px touch targets everywhere; controls stack full-width on a phone and sit side by side from `sm` up.
const field = 'form-selector p-2 min-h-11 w-full sm:w-auto text-[var(--text-primary)]'
const primary = 'btn btn-primary min-h-11'
const secondary = 'btn btn-secondary min-h-11'
const POLL_MS = 5000

/** A visible caption above a control. The control keeps its own aria-label, so its accessible name is unchanged. */
function Labeled({ text, children }: { text: string; children: ReactNode }) {
  return (
    <label className="flex w-full flex-col gap-1 sm:w-auto">
      <span aria-hidden="true" className="text-xs text-[var(--text-secondary)]">{text}</span>
      {children}
    </label>
  )
}

/** A group's stored policy as the controller wrote it. A malformed row must not take the whole page down. */
function parsePolicy(json: string): cloud.ScalingPolicy | null {
  try {
    const p = JSON.parse(json) as Partial<cloud.ScalingPolicy> | null
    if (p && typeof p.min === 'number' && typeof p.max === 'number' && typeof p.desired === 'number') return p as cloud.ScalingPolicy
  } catch {
    // fall through: unreadable
  }
  return null
}

function parseVmJson(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch (e) {
    throw new Error(`The VM spec is not valid JSON (${e instanceof Error ? e.message : 'parse error'}).`)
  }
}

export default function FleetCloudVpc() {
  const toast = useToastContext()
  const [projects, setProjects] = useState<NativeProject[]>([])
  const [hosts, setHosts] = useState<Host[]>([])
  const [project, setProject] = useState('')
  const [loading, setLoading] = useState(true)
  const [vpcs, setVpcs] = useState<cloud.Vpc[]>([])
  const [selected, setSelected] = useState('')
  const [subnets, setSubnets] = useState<cloud.Subnet[]>([])
  const [groups, setGroups] = useState<cloud.InstanceGroup[]>([])
  const [templates, setTemplates] = useState<cloud.LaunchTemplate[]>([])
  const [plan, setPlan] = useState<cloud.CloudPlan | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [name, setName] = useState('')
  const [cidr, setCidr] = useState('10.20.0.0/16')
  const [host, setHost] = useState('')
  const [subnetName, setSubnetName] = useState('')
  const [subnetCidr, setSubnetCidr] = useState('10.20.1.0/24')
  const [templateName, setTemplateName] = useState('')
  const [vmJson, setVmJson] = useState('')
  const [groupName, setGroupName] = useState('')
  const [templateId, setTemplateId] = useState('')
  const [subnetId, setSubnetId] = useState('')
  const [desired, setDesired] = useState(1)
  const [lbs, setLbs] = useState<NativeLoadBalancer[]>([])
  const selection = useRef({ project, selected })
  selection.current = { project, selected }

  useEffect(() => {
    let alive = true
    Promise.all([listProjectRegistry(), platformFetch<Host[]>('/api/v1/hosts')])
      .then(([p, h]) => {
        if (!alive) return
        setProjects(p.filter((x) => x.enabled))
        setHosts(h.filter((x) => x.state === 'online'))
        setProject(p.find((x) => x.enabled)?.id ?? '')
        setHost(h.find((x) => x.state === 'online')?.id ?? '')
      })
      .catch((e) => { if (alive) setError(formatUserError(e)) })
      .finally(() => { if (alive) setLoading(false) })
    listLoadBalancers().then((l) => { if (alive) setLbs(l) }).catch(() => undefined)
    return () => { alive = false }
  }, [])

  useEffect(() => {
    setVpcs([]); setGroups([]); setTemplates([]); setSelected(''); setPlan(null); setSubnets([]); setSubnetId(''); setTemplateId('')
    if (!project) return
    let alive = true
    Promise.all([cloud.listVpcs(project), cloud.listInstanceGroups(project), cloud.listLaunchTemplates(project)])
      .then(([v, g, t]) => {
        if (!alive) return
        setVpcs(v); setGroups(g); setTemplates(t); setSelected(v[0]?.id ?? ''); setError(null)
      })
      .catch((e) => { if (alive) setError(formatUserError(e)) })
    return () => { alive = false }
  }, [project])

  const refresh = useCallback(async () => {
    if (!project) return
    const [v, g, t] = await Promise.all([cloud.listVpcs(project), cloud.listInstanceGroups(project), cloud.listLaunchTemplates(project)])
    if (selection.current.project !== project) return
    setVpcs(v); setGroups(g); setTemplates(t)
    if (selected) {
      const s = await cloud.listSubnets(selected)
      if (selection.current.project === project && selection.current.selected === selected) setSubnets(s)
    }
  }, [project, selected])

  // Load the subnets of the chosen VPC once...
  useEffect(() => {
    setPlan(null); setSubnets([]); setSubnetId('')
    if (!selected) return
    let alive = true
    cloud.listSubnets(selected)
      .then((s) => { if (alive) setSubnets(s) })
      .catch((e) => { if (alive) setError(formatUserError(e)) })
    return () => { alive = false }
  }, [selected])

  // ...and keep polling only while one is still provisioning, and only while the tab is visible.
  const hasPending = subnets.some((s) => s.status === 'pending')
  useEffect(() => {
    if (!selected || !hasPending) return
    let alive = true
    const timer = setInterval(() => {
      if (document.visibilityState !== 'visible') return
      cloud.listSubnets(selected)
        .then((s) => { if (alive && selection.current.selected === selected) setSubnets(s) })
        .catch((e) => { if (alive) setError(formatUserError(e)) })
    }, POLL_MS)
    return () => { alive = false; clearInterval(timer) }
  }, [selected, hasPending])

  async function action(work: () => Promise<unknown>) {
    setBusy(true)
    try {
      await work()
      await refresh()
      setError(null)
    } catch (e) {
      setError(formatUserError(e))
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <PageLayout title="Virtual private clouds" subtitle="Isolated networks and elastic instances on your hosts." prepend={<FleetCloudSubNav />} error={error}>
      <div className="space-y-6">
        <div className="flex flex-wrap items-end gap-3">
          <Labeled text="Project">
            <select aria-label="Project" disabled={busy} className={field} value={project} onChange={(e) => setProject(e.target.value)}>
              {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </select>
          </Labeled>
          <button className={secondary} disabled={!project || busy} onClick={() => void action(async () => undefined)}>Refresh</button>
        </div>
        {loading && <p className="text-[var(--text-secondary)]" role="status">Loading…</p>}
        {!loading && projects.length === 0 && !error && (
          <p className="text-[var(--text-secondary)]">You do not belong to any project yet. Ask an administrator to add you to one.</p>
        )}
        <p className="text-[var(--text-secondary)]">Subnets currently operate on one host. Routing and peering are available as plans; they do not forward traffic yet.</p>

        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Create VPC">
          <h2 className="text-lg font-semibold">Create a VPC</h2>
          <form
            className="flex flex-wrap items-end gap-3"
            onSubmit={(e) => {
              e.preventDefault()
              void action(async () => { const v = await cloud.createVpc(project, { name, cidr, host_id: host }); setSelected(v.id); setName('') })
            }}
          >
            <Labeled text="VPC name"><input aria-label="VPC name" placeholder="VPC name" className={field} required value={name} onChange={(e) => setName(e.target.value)} /></Labeled>
            <Labeled text="CIDR"><input aria-label="VPC CIDR" className={field} required value={cidr} onChange={(e) => setCidr(e.target.value)} /></Labeled>
            <Labeled text="Host">
              <select aria-label="Host" className={field} value={host} onChange={(e) => setHost(e.target.value)}>
                {hosts.map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
              </select>
            </Labeled>
            <button className={primary} disabled={busy || !project || !host}>Create VPC</button>
          </form>
        </section>

        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Subnets">
          <h2 className="text-lg font-semibold">Subnets</h2>
          <Labeled text="VPC">
            <select aria-label="VPC" className={field} value={selected} onChange={(e) => setSelected(e.target.value)}>
              <option value="">Choose a VPC</option>
              {vpcs.map((v) => <option key={v.id} value={v.id}>{v.name} · {v.cidr}</option>)}
            </select>
          </Labeled>
          {!loading && vpcs.length === 0 && project && <p className="text-[var(--text-secondary)]">No VPCs in this project yet — create one above.</p>}
          <form
            className="flex flex-wrap items-end gap-3"
            onSubmit={(e) => { e.preventDefault(); void action(() => cloud.createSubnet(selected, { name: subnetName, cidr: subnetCidr })) }}
          >
            <Labeled text="Subnet name"><input aria-label="Subnet name" placeholder="Subnet name" className={field} required value={subnetName} onChange={(e) => setSubnetName(e.target.value)} /></Labeled>
            <Labeled text="Subnet CIDR"><input aria-label="Subnet CIDR" className={field} required value={subnetCidr} onChange={(e) => setSubnetCidr(e.target.value)} /></Labeled>
            <button className={primary} disabled={busy || !selected}>Create subnet</button>
            <button type="button" className={secondary} disabled={busy || !selected} onClick={() => void action(async () => setPlan(await cloud.getCloudPlan(selected)))}>Inspect plan</button>
          </form>
          {selected && subnets.length === 0 && <p>No subnets yet.</p>}
          <ul className="divide-y divide-[var(--apple-hairline)]">
            {subnets.map((s) => (
              <li key={s.id} className="flex flex-wrap items-center gap-3 py-3">
                <span>{s.name} · {s.cidr}</span>
                <span>{s.status}</span>
                {s.last_error && <span role="alert">{s.last_error}</span>}
                {s.status === 'error' && <button className={secondary} disabled={busy} onClick={() => void action(() => cloud.retrySubnet(s.id))}>Retry {s.name}</button>}
              </li>
            ))}
          </ul>
          {plan && (
            <div className="space-y-2">
              <h3 className="font-semibold">Network plan</h3>
              <p>Forwarding active: {plan.forwarding_active ? 'yes' : 'no'}</p>
              <ul>{plan.warnings.map((w) => <li key={w}>{w}</li>)}</ul>
              <pre className="overflow-auto text-xs">{JSON.stringify({ routes: plan.routes, peerings: plan.peerings }, null, 2)}</pre>
            </div>
          )}
        </section>

        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Launch templates">
          <h2 className="text-lg font-semibold">Launch templates</h2>
          <p className="text-[var(--text-secondary)]">Save a VM spec as an immutable template. Passwords and public console listeners are rejected.</p>
          <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); void action(() => cloud.createLaunchTemplate(project, templateName, parseVmJson(vmJson))) }}>
            <Labeled text="Template name"><input aria-label="Template name" placeholder="Template name" required className={field} value={templateName} onChange={(e) => setTemplateName(e.target.value)} /></Labeled>
            <Labeled text="VM spec (JSON)">
              <textarea aria-label="VM spec JSON" placeholder="Paste a VirtualMachine JSON spec" required className={`${field} block min-h-32 w-full font-mono`} value={vmJson} onChange={(e) => setVmJson(e.target.value)} />
            </Labeled>
            <button className={primary} disabled={busy || !project}>Save template</button>
          </form>
          {!loading && project && templates.length === 0 && <p className="text-[var(--text-secondary)]">No templates yet.</p>}
          {templates.length > 0 && <ul className="text-sm text-[var(--text-secondary)]">{templates.map((t) => <li key={t.id}>{t.name}</li>)}</ul>}
        </section>

        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Instance groups">
          <h2 className="text-lg font-semibold">Elastic instance groups</h2>
          <p className="text-[var(--text-secondary)]">Set capacity by hand, or open Autoscale to follow a CPU target, scale ahead of the usual rush, sleep instead of stop on scale-in, and drain members from a load balancer first. Pausing leaves current instances unchanged.</p>
          <form
            className="flex flex-wrap items-end gap-3"
            onSubmit={(e) => {
              e.preventDefault()
              void action(() => cloud.createInstanceGroup(project, { name: groupName, template_id: templateId, subnet_id: subnetId, policy: { min: 0, max: 10, desired, target_cpu: null, cooldown_secs: 300 } }))
            }}
          >
            <Labeled text="Group name"><input aria-label="Group name" placeholder="Group name" required className={field} value={groupName} onChange={(e) => setGroupName(e.target.value)} /></Labeled>
            <Labeled text="Launch template">
              <select aria-label="Launch template" required className={field} value={templateId} onChange={(e) => setTemplateId(e.target.value)}>
                <option value="">Choose a template</option>
                {templates.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
              </select>
            </Labeled>
            <Labeled text="Subnet">
              <select aria-label="Group subnet" required className={field} value={subnetId} onChange={(e) => setSubnetId(e.target.value)}>
                <option value="">Choose a ready subnet</option>
                {subnets.filter((s) => s.status === 'ready').map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
              </select>
            </Labeled>
            <Labeled text="Instances"><input aria-label="Desired instances" type="number" min={0} max={10} className={field} value={desired} onChange={(e) => setDesired(Number(e.target.value))} /></Labeled>
            <button className={primary} disabled={busy || !templateId || !subnetId}>Create group</button>
          </form>
          {!loading && project && groups.length === 0 && <p className="text-[var(--text-secondary)]">No instance groups yet.</p>}
          <ul>
            {groups.map((g) => {
              const policy = parsePolicy(g.policy_json)
              if (!policy) {
                return (
                  <li className="flex flex-wrap gap-3 py-3" key={g.id}>
                    <span>{g.name} · its scaling policy could not be read</span>
                    {g.last_error && <span role="alert">{g.last_error}</span>}
                  </li>
                )
              }
              return (
                <li className="flex flex-wrap gap-3 py-3" key={g.id}>
                  <span>{g.name} · {policy.desired} desired{policy.target_cpu != null ? ` · ${policy.target_cpu}% CPU target` : ''}{policy.predictive ? ' · predictive' : ''} · {g.paused ? 'paused' : 'active'}</span>
                  {g.last_error && <span role="alert">{g.last_error}</span>}
                  <button className={secondary} disabled={busy} onClick={() => void action(() => cloud.updateInstanceGroup(g.id, policy, !g.paused))}>{g.paused ? 'Resume' : 'Pause'} {g.name}</button>
                  <button className={secondary} disabled={busy || policy.desired >= policy.max} onClick={() => void action(() => cloud.updateInstanceGroup(g.id, { ...policy, desired: policy.desired + 1 }, g.paused))}>Add instance</button>
                  <button className={secondary} disabled={busy || policy.desired <= policy.min} onClick={() => void action(() => cloud.updateInstanceGroup(g.id, { ...policy, desired: policy.desired - 1 }, g.paused))}>{policy.scale_in === 'sleep' ? 'Sleep one instance' : 'Stop one instance'}</button>
                  <GroupAutoscale group={g} policy={policy} loadBalancers={lbs.filter((b) => !b.project_id || b.project_id === project)} busy={busy} onSave={(p) => action(() => cloud.updateInstanceGroup(g.id, p, g.paused))} />
                </li>
              )
            })}
          </ul>
        </section>
      </div>
      <FleetCloudFooter />
    </PageLayout>
  )
}

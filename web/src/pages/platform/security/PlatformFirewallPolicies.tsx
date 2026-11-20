// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { ArrowLeft, RefreshCw, Shield } from 'lucide-react'
import PageLayout from '../../../components/PageLayout'
import { MacGlassPanel, MacListRow, MacSheet } from '../../../components/platform/mac/PlatformMacUi'
import {
  createFirewallPolicy,
  getFirewallOverview,
  getMultisiteDrTemplates,
  getMultisiteExport,
  listFirewallPolicies,
  simulateFirewallPolicy,
  type FirewallPolicyRow,
  type FirewallTargetSummary,
} from '../../../api/zeusFirewall'
import JsonInspector from '../../../components/platform/JsonInspector'
import { useToastContext } from '../../../contexts/ToastContext'
import { formatUserError } from '../../../utils/apiError'
import { hubLinkClasses } from '../../../utils/semanticColors'

// `firewall_policies` rows only ever carry `id`/`name`/`spec_yaml` — there's no
// top-level `profile` column, it's embedded in the YAML text. Pull it out for
// display rather than reading a `p.profile` field that's always undefined.
function profileFromSpecYaml(specYaml: string | null | undefined): string | null {
  const match = specYaml?.match(/^\s*profile:\s*(\S+)/m)
  return match ? match[1] : null
}

export default function PlatformFirewallPolicies() {
  const toast = useToastContext()
  const [rows, setRows] = useState<FirewallPolicyRow[]>([])
  const [error, setError] = useState<string | null>(null)
  const [name, setName] = useState('production-default')
  const [profile, setProfile] = useState('ProductionServer')
  const [specYaml, setSpecYaml] = useState('profile: ProductionServer\n')
  const [simResult, setSimResult] = useState<Record<string, unknown> | null>(null)
  const [sheetOpen, setSheetOpen] = useState(false)
  const [drTemplates, setDrTemplates] = useState<Awaited<ReturnType<typeof getMultisiteDrTemplates>> | null>(null)
  const [multisiteExport, setMultisiteExport] = useState<Record<string, unknown> | null>(null)
  const [targets, setTargets] = useState<FirewallTargetSummary[]>([])
  const [targetId, setTargetId] = useState('')

  const load = useCallback(async () => {
    setError(null)
    try {
      setRows(await listFirewallPolicies())
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => { void load() }, [load])
  useEffect(() => {
    void getMultisiteDrTemplates().then(setDrTemplates).catch(() => setDrTemplates(null))
  }, [])
  useEffect(() => {
    void getFirewallOverview()
      .then((ov) => {
        setTargets(ov.targets)
        setTargetId((current) => current || ov.targets[0]?.id || '')
      })
      .catch(() => setTargets([]))
  }, [])

  const exportMultisite = async () => {
    try {
      const data = await getMultisiteExport()
      setMultisiteExport(data)
      toast.success('Multisite export ready')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const create = async () => {
    try {
      await createFirewallPolicy({ name, spec_yaml: specYaml })
      toast.success('Policy created')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const simulate = async () => {
    if (!targetId) {
      toast.error('Select a target to simulate against')
      return
    }
    try {
      // The simulate endpoint plans against a real firewall target (host), not
      // the policy name/spec_yaml being drafted — it previously sent `name` in
      // place of the `target_id` the backend actually requires, so every
      // simulate attempt failed with a raw deserialize error.
      const r = await simulateFirewallPolicy({ target_id: targetId, profile })
      setSimResult(r as Record<string, unknown>)
      setSheetOpen(true)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <PageLayout
      compact
      error={error}
      prepend={
        <Link to="/platform/zeus/security/firewall" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Firewall
        </Link>
      }
      title="Policy Studio"
      subtitle={`${rows.length} polic${rows.length === 1 ? 'y' : 'ies'} · create, simulate, and manage Zeus firewall policies`}
      icon={<Shield className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <div className="flex gap-2">
          <button type="button" className="btn-primary text-sm" onClick={() => void create()}>Create</button>
          <button type="button" className="btn-secondary text-xs" onClick={() => void load()} aria-label="Refresh">
            <RefreshCw className="w-4 h-4" />
          </button>
        </div>
      }
      contentClassName="space-y-4"
    >
      <MacGlassPanel title="New policy">
        <div className="grid gap-3 max-w-lg">
          <input aria-label="Policy name" className="input text-sm" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
          <input aria-label="Policy profile" className="input text-sm" placeholder="Profile" value={profile} onChange={(e) => setProfile(e.target.value)} />
          <textarea aria-label="Policy spec YAML" className="input text-sm font-mono min-h-[8rem]" value={specYaml} onChange={(e) => setSpecYaml(e.target.value)} />
          <select aria-label="Simulate against target" className="input text-sm" value={targetId} onChange={(e) => setTargetId(e.target.value)}>
            {targets.length === 0 && <option value="">No firewall targets available</option>}
            {targets.map((t) => (
              <option key={t.id} value={t.id}>{t.name}</option>
            ))}
          </select>
          <div className="flex gap-2">
            <button type="button" className="btn-secondary text-sm" onClick={() => void simulate()}>Simulate</button>
          </div>
        </div>
      </MacGlassPanel>
      <MacGlassPanel title="Policies">
        <ul className="divide-y divide-white/[0.04] -mx-1">
          {rows.map((p) => (
            <MacListRow key={p.id} title={p.name} subtitle={profileFromSpecYaml(p.spec_yaml) ?? 'policy'} />
          ))}
          {rows.length === 0 && <p className="text-sm text-[var(--text-muted)] px-1">No policies yet.</p>}
        </ul>
      </MacGlassPanel>
      <MacGlassPanel title="Multi-site DR">
        {drTemplates ? (
          <p className="text-sm text-[var(--text-muted)] mb-3">{drTemplates.summary}</p>
        ) : (
          <p className="text-sm text-[var(--text-muted)] mb-3">Loading DR templates…</p>
        )}
        {drTemplates?.profiles?.length ? (
          <ul className="text-xs space-y-1 mb-3">
            {drTemplates.profiles.map((p) => (
              <li key={p.primary_profile} className="text-[var(--text-secondary)]">{p.primary_profile} → {p.dr_profile}</li>
            ))}
          </ul>
        ) : null}
        <button type="button" className="btn-secondary text-sm" onClick={() => void exportMultisite()}>Export federation bundle</button>
        {multisiteExport ? <JsonInspector data={multisiteExport} className="mt-3" /> : null}
      </MacGlassPanel>
      <MacSheet open={sheetOpen} onClose={() => setSheetOpen(false)} title="Simulation result" wide>
        {simResult ? <JsonInspector data={simResult} /> : <p className="text-sm text-[var(--text-muted)]">—</p>}
      </MacSheet>
    </PageLayout>
  )
}

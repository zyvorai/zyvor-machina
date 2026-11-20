// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Shield } from 'lucide-react'
import FleetSettingsPane from '../../components/platform/FleetSettingsPane'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { listPolicyRules, listProjectQuotas, upsertProjectQuota, type PolicyRule } from '../../api/platform'
import { getAiPolicyExport } from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformPolicy({ embedded }: { embedded?: boolean } = {}) {
  const toast = useToastContext()
  const [rules, setRules] = useState<PolicyRule[]>([])
  const [quotas, setQuotas] = useState<Awaited<ReturnType<typeof listProjectQuotas>>>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [project, setProject] = useState('default')
  const [maxVms, setMaxVms] = useState(50)
  const [maxVcpu, setMaxVcpu] = useState(50 * 4)
  const [maxMemoryMib, setMaxMemoryMib] = useState(50 * 8192)
  const [maxStorageGib, setMaxStorageGib] = useState(50 * 100)

  const load = useCallback(async () => {
    setError(null)
    try {
      const [r, q] = await Promise.all([listPolicyRules(), listProjectQuotas()])
      setRules(r)
      setQuotas(q)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  // Keep the whole quota form in sync with the matching existing quota whenever Project changes to
  // an already-configured project, so Save doesn't silently clobber real vCPU/memory/storage values
  // with a fixed multiple of Max VMs — upsertProjectQuota is a full replace, not a partial patch, so
  // every field submitted has to reflect what the user actually intends.
  useEffect(() => {
    const match = quotas.find((q) => q.project === project)
    if (match) {
      setMaxVms(match.max_vms)
      setMaxVcpu(match.max_vcpu)
      setMaxMemoryMib(match.max_memory_mib)
      setMaxStorageGib(match.max_storage_gib)
    }
  }, [project, quotas])

  const downloadPolicyYaml = async () => {
    try {
      const r = await getAiPolicyExport()
      const blob = new Blob([r.yaml], { type: 'text/yaml' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = 'machina-policy.yaml'
      a.click()
      URL.revokeObjectURL(url)
      toast.success(`Downloaded ${r.rule_count} rules, ${r.quota_count} quotas`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const saveQuota = async () => {
    try {
      await upsertProjectQuota({
        project,
        max_vms: maxVms,
        max_vcpu: maxVcpu,
        max_memory_mib: maxMemoryMib,
        max_storage_gib: maxStorageGib,
      })
      toast.success('Quota saved')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      loading={loading && rules.length === 0 && quotas.length === 0}
      error={error}
      onErrorRetry={() => void load()}
      title={embedded ? undefined : 'Policy & Quotas'}
      subtitle={embedded ? undefined : 'Controller policy rules and per-project resource limits.'}
      icon={embedded ? undefined : <Shield className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <MacGlassPanel
        title="Policy rules"
        action={
          <button type="button" className="btn-secondary text-xs" onClick={() => void downloadPolicyYaml()}>
            Download policy YAML
          </button>
        }
      >
        {rules.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No policy rules configured.</p>
        ) : (
          <ul className="divide-y divide-white/[0.04] -mx-1">
            {rules.map((r) => (
              <MacListRow
                key={r.id}
                title={r.name}
                subtitle={r.enabled ? 'Enabled' : 'Disabled'}
                badge={<Shield className="w-4 h-4 text-orange-400" />}
              />
            ))}
          </ul>
        )}
      </MacGlassPanel>
      <MacGlassPanel title="Project quotas">
        <div className="grid gap-3 sm:grid-cols-3 max-w-2xl mb-4">
          <label className="block text-xs text-[var(--text-muted)]">
            Project
            <input className="input text-sm mt-1 w-full" value={project} onChange={(e) => setProject(e.target.value)} />
          </label>
          <label className="block text-xs text-[var(--text-muted)]">
            Max VMs
            <input type="number" className="input text-sm mt-1 w-full" value={maxVms} onChange={(e) => setMaxVms(Number(e.target.value))} />
          </label>
          <label className="block text-xs text-[var(--text-muted)]">
            Max vCPU
            <input type="number" className="input text-sm mt-1 w-full" value={maxVcpu} onChange={(e) => setMaxVcpu(Number(e.target.value))} />
          </label>
          <label className="block text-xs text-[var(--text-muted)]">
            Max memory (MiB)
            <input type="number" className="input text-sm mt-1 w-full" value={maxMemoryMib} onChange={(e) => setMaxMemoryMib(Number(e.target.value))} />
          </label>
          <label className="block text-xs text-[var(--text-muted)]">
            Max storage (GiB)
            <input type="number" className="input text-sm mt-1 w-full" value={maxStorageGib} onChange={(e) => setMaxStorageGib(Number(e.target.value))} />
          </label>
          <div className="flex items-end">
            <button type="button" className="btn-primary text-sm w-full" onClick={() => void saveQuota()}>Save quota</button>
          </div>
        </div>
        <ul className="divide-y divide-white/[0.04] -mx-1">
          {quotas.map((q) => (
            <MacListRow
              key={q.project}
              title={q.project}
              subtitle={`${q.max_vms} VMs · ${q.max_vcpu} vCPU · ${Math.round(q.max_memory_mib / 1024)} GiB RAM`}
            />
          ))}
        </ul>
      </MacGlassPanel>
      {!embedded && <FleetSettingsPane kind="general" />}
    </PlatformPageChrome>
  )
}

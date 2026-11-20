// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { Lock } from 'lucide-react'
import ErrorBanner from '../../components/ErrorBanner'
import DetailTabs from '../../components/platform/DetailTabs'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import {
  getEnterpriseSecurityOverview,
  getFipsMatrix,
  getFleetKeychain,
  getTenantIsolationOverview,
  upsertTenantPolicy,
  type EnterpriseSecurityOverview,
  type FipsMatrix,
  type FleetKeychainOverview,
  type TenantIsolationOverview,
} from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import {hostStateTone, httpStatusTone, migrationReadinessTone, riskTone, statusBadgeClasses, statusPillClasses, statusToneClass, taskStatusTone, webhookDeliveryTone, hubLinkClasses} from '../../utils/semanticColors'
import { useToastContext } from '../../contexts/ToastContext'

type TabId = 'keychain' | 'fips' | 'tenants'

const ENTERPRISE_TABS = [
  { id: 'keychain' as const, label: 'Keychain' },
  { id: 'fips' as const, label: 'FIPS matrix' },
  { id: 'tenants' as const, label: 'Tenant isolation' },
]

const KIND_LABELS: Record<string, string> = {
  air_gap: 'Air-gap',
  api_key: 'API key',
}

function entryHref(kind: string): string | undefined {
  if (kind === 'air_gap') return '/platform/settings?section=security'
  if (kind === 'api_key') return '/platform/api-keys'
  return undefined
}

export default function PlatformEnterprise({ embedded }: { embedded?: boolean } = {}) {
  const toast = useToastContext()
  const [tab, setTab] = usePlatformTabState<TabId>(ENTERPRISE_TABS.map((t) => t.id), { defaultTab: 'keychain' })
  const activeTab: TabId = embedded ? 'keychain' : tab

  const [keychain, setKeychain] = useState<FleetKeychainOverview | null>(null)
  const [overview, setOverview] = useState<EnterpriseSecurityOverview | null>(null)
  const [fips, setFips] = useState<FipsMatrix | null>(null)
  const [tenants, setTenants] = useState<TenantIsolationOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [policyProject, setPolicyProject] = useState('default')
  const [policyMaxVms, setPolicyMaxVms] = useState('50')
  const [policyIsolation, setPolicyIsolation] = useState('shared')
  // Last-response-wins guard: switching tabs quickly (each tab fetches a
  // different data set) could otherwise let a slow response for a tab the
  // user already left overwrite state committed by a newer fetch.
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    try {
      if (activeTab === 'keychain') {
        const k = await getFleetKeychain()
        if (alive()) setKeychain(k)
        return
      }
      const [ov, f, t] = await Promise.all([
        getEnterpriseSecurityOverview(),
        getFipsMatrix(),
        getTenantIsolationOverview(),
      ])
      if (!alive()) return
      setOverview(ov)
      setFips(f)
      setTenants(t)
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    }
  }, [activeTab])

  useEffect(() => { void load() }, [load])

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      error={error}
      onErrorRetry={() => void load()}
      title={embedded ? undefined : 'Enterprise Security'}
      subtitle={embedded ? undefined : 'Secrets inventory and link-out — API keys, air-gap bundles (no live secret export).'}
      icon={embedded ? undefined : <Lock className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      {actionError && <ErrorBanner message={actionError} />}
      {(keychain?.summary || overview?.summary) && activeTab !== 'keychain' && (
        <p className="text-sm text-[var(--text-muted)]">{overview?.summary}</p>
      )}
      {activeTab === 'keychain' && keychain && (
        <p className="text-sm text-[var(--text-muted)]">{keychain.summary}</p>
      )}
      {activeTab === 'keychain' && keychain && (
        <div className="apple-metric-band">
          {[
            { label: 'API keys', value: String(keychain.api_keys) },
            { label: 'Air-gap bundles', value: String(keychain.air_gap_bundles) },
          ].map((s) => (
            <div key={s.label} className="min-w-0">
              <div className="apple-metric-value">{s.value}</div>
              <div className="apple-metric-label">{s.label}</div>
            </div>
          ))}
        </div>
      )}
      {activeTab !== 'keychain' && overview && (
        <div className="apple-metric-band">
          {[
            { label: 'Tenant policies', value: String(overview.tenant_policies) },
            { label: 'FIPS profiles', value: String(overview.fips_profiles) },
          ].map((s) => (
            <div key={s.label} className="min-w-0">
              <div className="apple-metric-value">{s.value}</div>
              <div className="apple-metric-label">{s.label}</div>
            </div>
          ))}
        </div>
      )}
      {!embedded && <DetailTabs primary={ENTERPRISE_TABS} active={tab} onChange={setTab} />}

      {activeTab === 'keychain' && (
        <MacGlassPanel title="Secrets inventory" subtitle="Metadata only — manage credentials in linked panes.">
          {!keychain ? (
            <p className="text-sm text-[var(--text-muted)] py-6 text-center">Loading keychain inventory…</p>
          ) : keychain.entries.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No credentials registered — add API keys or export an air-gap bundle in Settings.</p>
          ) : (
            <div className="divide-y divide-white/[0.04] -mx-1">
              {keychain.entries.map((e) => (
                <MacListRow
                  key={`${e.kind}-${e.id}`}
                  title={e.name}
                  subtitle={e.summary}
                  href={entryHref(e.kind)}
                  badge={
                    <span className="text-[10px] uppercase px-2 py-0.5 rounded border border-white/[0.08] text-[var(--text-muted)]">
                      {KIND_LABELS[e.kind] ?? e.kind}
                    </span>
                  }
                  trailing={
                    <span className={`text-[10px] ${
                      e.status === 'active'
                        ? statusToneClass('ok')
                        : e.status === 'disconnected'
                          ? statusToneClass('warn')
                          : 'text-[var(--text-muted)]'
                    }`}>
                      {e.status}
                    </span>
                  }
                />
              ))}
            </div>
          )}
          <div className="flex flex-wrap gap-3 mt-4 pt-2 border-t border-white/[0.04]">
            <Link to="/platform/settings?section=security" className={`text-sm ${hubLinkClasses()}`}>System Settings → Security</Link>
            <Link to="/platform/api-keys" className={`text-sm ${hubLinkClasses()}`}>API keys</Link>
          </div>
        </MacGlassPanel>
      )}

      {activeTab === 'fips' && fips && (
        <MacGlassPanel title="FIPS crypto matrix">
          <p className="text-sm text-[var(--text-muted)] mb-2">{fips.summary}</p>
          <p className="text-xs text-[var(--text-muted)] mb-4">Runtime: {fips.openssl_version}</p>
          <ul className="space-y-3">
            {fips.profiles.map((p) => (
              <li key={p.id} className="rounded-lg border border-[var(--apple-hairline)] p-3">
                <p className="text-sm font-medium text-[var(--text-primary)]">{p.name}</p>
                <p className="text-xs text-[var(--text-muted)] mt-1">TLS {p.tls_min_version} · FIPS {p.fips_mode}</p>
                <p className="text-xs text-[var(--text-muted)]">{p.cipher_suites}</p>
                <p className="text-xs text-[var(--text-faint)] mt-1">{p.notes}</p>
              </li>
            ))}
          </ul>
        </MacGlassPanel>
      )}

      {activeTab === 'tenants' && tenants && (
        <MacGlassPanel title="Workspace isolation">
          <p className="text-sm text-[var(--text-muted)] mb-3">{tenants.summary}</p>
          <div className="grid gap-2 sm:grid-cols-4 mb-4 pb-4 border-b border-white/[0.04]">
            <input className="input text-sm" aria-label="Project" value={policyProject} onChange={(e) => setPolicyProject(e.target.value)} placeholder="Project" />
            <input className="input text-sm" type="number" min={1} aria-label="Max VMs" value={policyMaxVms} onChange={(e) => setPolicyMaxVms(e.target.value)} placeholder="Max VMs" />
            <select className="input text-sm" aria-label="Network isolation" value={policyIsolation} onChange={(e) => setPolicyIsolation(e.target.value)}>
              <option value="shared">shared</option>
              <option value="isolated">isolated</option>
              <option value="dedicated">dedicated</option>
            </select>
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={() => {
                setActionError(null)
                const maxVms = Number(policyMaxVms)
                void upsertTenantPolicy(policyProject.trim(), {
                  max_vms: Number.isFinite(maxVms) ? maxVms : undefined,
                  network_isolation: policyIsolation,
                  enforce_quotas: true,
                })
                  .then(() => {
                    toast.success('Tenant policy saved')
                    return load()
                  })
                  .catch((e: unknown) => setActionError(formatUserError(e)))
              }}
            >
              Save policy
            </button>
          </div>
          <div className="overflow-x-auto">
          <table className="w-full text-sm text-left" aria-label="Tenant isolation policies">
            <thead className="text-xs text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
              <tr>
                <th scope="col" className="py-2 pr-4">Project</th>
                <th scope="col" className="py-2 pr-4">VMs</th>
                <th scope="col" className="py-2 pr-4">Network</th>
                <th scope="col" className="py-2 pr-4">Quotas</th>
                <th scope="col" className="py-2">Status</th>
              </tr>
            </thead>
            <tbody>
              {(tenants.projects ?? []).map((p) => (
                <tr key={p.project_name} className="border-b border-[var(--apple-hairline)]/60">
                  <td className="py-2 pr-4 text-[var(--text-primary)]">{p.project_name}</td>
                  <td className="py-2 pr-4 text-[var(--text-muted)]">{p.vm_count}{p.max_vms > 0 ? ` / ${p.max_vms}` : ''}</td>
                  <td className="py-2 pr-4 text-[var(--text-muted)]">{p.network_isolation}</td>
                  <td className="py-2 pr-4 text-[var(--text-muted)]">{p.enforce_quotas ? 'enforced' : 'off'}</td>
                  <td className={`py-2 text-xs ${statusToneClass(p.quota_status.includes('exceeded') ? 'error' : 'ok')}`}>{p.quota_status}</td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        </MacGlassPanel>
      )}
    </PlatformPageChrome>
  )
}

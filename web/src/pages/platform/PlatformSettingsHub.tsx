// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { Settings, Shield, Users, HardDrive, Network, RefreshCw, Key, LayoutGrid, Lock, FileBarChart, Terminal, Plug, Workflow, Sparkles } from 'lucide-react'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'
import PlatformSettings from './PlatformSettings'
import PlatformAiProviders from './PlatformAiProviders'
import PlatformZyraSettings from './PlatformZyraSettings'
import PlatformAppearanceSettings from '../../components/platform/PlatformAppearanceSettings'
import PlatformUsers from './PlatformUsers'
import PlatformPolicy from './PlatformPolicy'
import PlatformApiKeys from './PlatformApiKeys'
import PlatformWebhooks from './PlatformWebhooks'
import PlatformReports from './PlatformReports'
import PlatformEvents from './PlatformEvents'
import PlatformProjects from './PlatformProjects'
import PlatformIntegrations from './PlatformIntegrations'
import PlatformEnterprise from './PlatformEnterprise'
import {
  MacSettingsPane,
  MacSettingsGroup,
  MacSettingsGroupBody,
  MacToggle,
  MacListRow,
} from '../../components/platform/mac/PlatformMacUi'
import { getClusterSettings, patchClusterSettings, getEnterpriseSecurityOverview, getFleetNetwork, listAirGapBundles, createAirGapBundle, getAirGapBundle, listPolicyRules, type EnterpriseSecurityOverview, type FleetNetworkOverview, type AirGapBundle, type PolicyRule } from '../../api/platform'
import JsonInspector, { asArray, asRecord } from '../../components/platform/JsonInspector'
import { getAiPolicyExport } from '../../api/ai'
import { getFirewallOverview, type FirewallOverview } from '../../api/zeusFirewall'
import { listAlertRules, listAlerts, listTokens } from '../../api/automation'
import { getSession, type AuthSession } from '../../api/auth'
import IdentitySsoPanel from '../../components/IdentitySsoPanel'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import {hostStateTone, httpStatusTone, migrationReadinessTone, riskTone, statusBadgeClasses, statusPillClasses, statusToneClass, taskStatusTone, webhookDeliveryTone, hubLinkClasses} from '../../utils/semanticColors'
import { dispatchOpenHelp } from '../../utils/openHelp'

function BrowserSessionInfo() {
  const [session, setSession] = useState<AuthSession | null>(null)
  useEffect(() => {
    void getSession().then(setSession)
  }, [])
  if (!session?.authenticated || session.active_sessions_for_user == null) return null
  const max = session.max_sessions_per_user ?? 0
  const concurrent = max === 0
  return (
    <p className="text-xs text-[var(--text-muted)] pb-2">
      Browser sessions for <span className="text-[var(--text-secondary)]">{session.username}</span>:{' '}
      <span className="text-[var(--text-primary)]">{session.active_sessions_for_user}</span>
      {concurrent ? ' (concurrent logins allowed)' : ` / ${max} max per user`}
    </p>
  )
}

type SettingsSection =
  | 'general'
  | 'identity'
  | 'zyra'
  | 'ai-providers'
  | 'security'
  | 'network'
  | 'updates'
  | 'about'
  | 'users'
  | 'stage-manager'
  | 'keychain'
  | 'policy'
  | 'api-keys'
  | 'webhooks'
  | 'reports'
  | 'console'
  | 'resources'
  | 'integrations'
  | 'support'

const SETTINGS_SECTIONS: SettingsSection[] = [
  'general',
  'identity',
  'zyra',
  'ai-providers',
  'security',
  'network',
  'users',
  'stage-manager',
  'keychain',
  'policy',
  'api-keys',
  'webhooks',
  'reports',
  'console',
  'resources',
  'updates',
  'integrations',
  'support',
  'about',
]

function parseSettingsSection(raw: string | null): SettingsSection {
  return SETTINGS_SECTIONS.includes(raw as SettingsSection) ? (raw as SettingsSection) : 'general'
}

function SettingsWorkspaceLink({ to, label }: { to: string; label: string }) {
  return (
    <p className="text-sm pb-2 border-b border-white/[0.06] mb-4">
      <Link to={to} className={hubLinkClasses('hover:underline')}>{label} →</Link>
    </p>
  )
}

const SECTIONS: Array<{ id: SettingsSection; label: string; icon: React.ReactNode; fullPath?: string }> = [
  { id: 'general', label: 'General', icon: <Settings className="w-4 h-4" /> },
  { id: 'identity', label: 'Identity & SSO', icon: <Shield className="w-4 h-4" /> },
  { id: 'zyra', label: 'Zyra', icon: <Sparkles className="w-4 h-4" /> },
  { id: 'ai-providers', label: 'AI Providers', icon: <Plug className="w-4 h-4" /> },
  { id: 'security', label: 'Security', icon: <Shield className="w-4 h-4" /> },
  { id: 'network', label: 'Network', icon: <Network className="w-4 h-4" /> },
  { id: 'users', label: 'Users & Groups', icon: <Users className="w-4 h-4" />, fullPath: '/platform/users' },
  { id: 'stage-manager', label: 'Stage Manager', icon: <LayoutGrid className="w-4 h-4" />, fullPath: '/platform/projects' },
  { id: 'keychain', label: 'Keychain', icon: <Lock className="w-4 h-4" />, fullPath: '/platform/enterprise' },
  { id: 'policy', label: 'Policy & Quotas', icon: <Shield className="w-4 h-4" />, fullPath: '/platform/policy' },
  { id: 'api-keys', label: 'API Keys', icon: <Key className="w-4 h-4" />, fullPath: '/platform/api-keys' },
  { id: 'webhooks', label: 'Webhooks', icon: <Workflow className="w-4 h-4" />, fullPath: '/platform/webhooks' },
  { id: 'reports', label: 'Reports', icon: <FileBarChart className="w-4 h-4" />, fullPath: '/platform/reports' },
  { id: 'console', label: 'Console', icon: <Terminal className="w-4 h-4" />, fullPath: '/platform/events' },
  { id: 'resources', label: 'Infrastructure', icon: <HardDrive className="w-4 h-4" />, fullPath: '/platform/infrastructure' },
  { id: 'updates', label: 'Updates', icon: <RefreshCw className="w-4 h-4" /> },
  { id: 'integrations', label: 'Apps & Integrations', icon: <Plug className="w-4 h-4" />, fullPath: '/platform/settings?section=integrations' },
  // Support / About are top menubar Help items — not Settings side-rail entries.
]

const SECTION_GROUPS: Record<string, string> = {"general": "Workspace", "zyra": "Workspace", "stage-manager": "Workspace", "console": "Workspace", "identity": "Access", "users": "Access", "keychain": "Access", "api-keys": "Access", "security": "Security", "policy": "Security", "network": "Security", "ai-providers": "Integrations", "webhooks": "Integrations", "integrations": "Integrations", "reports": "System", "resources": "System", "updates": "System"}
const SECTION_GROUP_ORDER = ['Workspace', 'Access', 'Security', 'Integrations', 'System']

export default function PlatformSettingsHub() {
  const toast = useToastContext()
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  const initialSection = searchParams.get('section')
  const [section, setSection] = useState<SettingsSection>(parseSettingsSection(initialSection))
  const [deleteApproval, setDeleteApproval] = useState(false)
  const [approvalSlaHours, setApprovalSlaHours] = useState(72)
  const [saving, setSaving] = useState(false)
  const [policyYaml, setPolicyYaml] = useState<string | null>(null)
  const [firewallOverview, setFirewallOverview] = useState<FirewallOverview | null>(null)
  const [enterprise, setEnterprise] = useState<EnterpriseSecurityOverview | null>(null)
  const [airGapBundles, setAirGapBundles] = useState<AirGapBundle[]>([])
  const [bundleName, setBundleName] = useState('sovereign-export')
  const [bundleCreating, setBundleCreating] = useState(false)
  const [fleetNetwork, setFleetNetwork] = useState<FleetNetworkOverview | null>(null)
  const [policyRules, setPolicyRules] = useState<PolicyRule[]>([])
  const [selectedBundle, setSelectedBundle] = useState<AirGapBundle | null>(null)
  const [daemonAlerts, setDaemonAlerts] = useState<number>(0)
  const [daemonTokens, setDaemonTokens] = useState<number>(0)

  const load = useCallback(async () => {
    // These four groups don't depend on each other — fire them concurrently instead
    // of one after another (used to serialize ~4 independent request waves).
    const [clusterRes, fwRes, enterpriseRes, alertsTokensRes] = await Promise.allSettled([
      getClusterSettings(),
      getFirewallOverview(),
      Promise.all([
        getEnterpriseSecurityOverview(),
        listAirGapBundles(),
        listPolicyRules().catch(() => []),
      ]),
      Promise.all([listAlerts().catch(() => []), listTokens().catch(() => [])]),
    ])
    if (clusterRes.status === 'fulfilled') {
      const s = clusterRes.value
      setDeleteApproval(Boolean(s.require_vm_delete_approval))
      setApprovalSlaHours(s.firewall_approval_sla_hours ?? 72)
    }
    if (fwRes.status === 'fulfilled') setFirewallOverview(fwRes.value)
    if (enterpriseRes.status === 'fulfilled') {
      const [ov, bundles, policy] = enterpriseRes.value
      setEnterprise(ov)
      setAirGapBundles(bundles)
      setPolicyRules(policy)
    }
    if (alertsTokensRes.status === 'fulfilled') {
      const [alerts, tokens] = alertsTokensRes.value
      setDaemonAlerts(alerts.length)
      setDaemonTokens(tokens.length)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    const raw = searchParams.get('section')
    // About / Support moved to the top menubar Help menu — bounce legacy deep links.
    if (raw === 'about') {
      dispatchOpenHelp('about')
      setSearchParams({}, { replace: true })
      setSection('general')
      return
    }
    if (raw === 'support') {
      navigate('/platform/support', { replace: true })
      return
    }
    setSection(parseSettingsSection(raw))
  }, [searchParams, setSearchParams, navigate])

  useEffect(() => {
    if (section !== 'network') return
    void getFleetNetwork().then(setFleetNetwork).catch(() => setFleetNetwork(null))
  }, [section])

  const selectSection = (id: string) => {
    const next = id as SettingsSection
    setSection(next)
    setSearchParams(next === 'general' ? {} : { section: next }, { replace: true })
  }

  const toggleDeleteApproval = async (checked: boolean) => {
    setSaving(true)
    try {
      await patchClusterSettings({ require_vm_delete_approval: checked })
      setDeleteApproval(checked)
      toast.success(checked ? 'VM deletion approval enabled' : 'VM deletion approval disabled')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const saveApprovalSla = async () => {
    setSaving(true)
    try {
      await patchClusterSettings({ firewall_approval_sla_hours: approvalSlaHours })
      toast.success('Firewall approval SLA saved')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const exportAirGapBundle = async () => {
    setBundleCreating(true)
    try {
      const bundle = await createAirGapBundle({ name: bundleName.trim() || 'sovereign-export' })
      setAirGapBundles((prev) => [bundle, ...prev])
      toast.success(`Air-gap bundle "${bundle.name}" manifest created`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBundleCreating(false)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Settings"
      subtitle="Cluster, security, integrations, and platform preferences"
      icon={<Settings className="w-6 h-6 text-[var(--text-muted)]" />}
      contentClassName="space-y-0"
    >
    <MacSettingsPane
      title="Settings"
      hideTitle
      sections={SECTION_GROUP_ORDER.flatMap((g) => SECTIONS.filter((s) => SECTION_GROUPS[s.id] === g).map((s) => ({ id: s.id, label: s.label, icon: s.icon, group: g })))}
      active={section}
      onSelect={selectSection}
    >
      {section === 'general' && (
        <div className="space-y-6">
          <PlatformAppearanceSettings />
          <PlatformSettings embedded />
          <MacSettingsGroup title="Classic daemon automation">
            <MacSettingsGroupBody>
              <p className="text-xs text-[var(--text-muted)] leading-relaxed">Single-host alerts and API tokens from the libvirt daemon — mirror of Settings → Automation in classic UI.</p>
              <p className="text-sm text-[var(--text-primary)]">{daemonAlerts} active alert(s) · {daemonTokens} API token(s)</p>
              <Link to="/settings?tab=automation" className={`text-sm inline-block ${hubLinkClasses()}`}>Open classic automation →</Link>
            </MacSettingsGroupBody>
          </MacSettingsGroup>
        </div>
      )}

      {section === 'identity' && (
        <div className="space-y-4">
          <BrowserSessionInfo />
          <IdentitySsoPanel compact />
        </div>
      )}

      {section === 'zyra' && <PlatformZyraSettings embedded />}

      {section === 'ai-providers' && <PlatformAiProviders embedded />}

      {section === 'security' && (
        <div className="space-y-6">
          <MacSettingsGroup title="Security">
            <MacSettingsGroupBody className="pb-0">
              <BrowserSessionInfo />
            </MacSettingsGroupBody>
            <MacToggle
              label="Require confirmation for production VM deletion"
              description="Deletes queue for approval when enabled cluster-wide."
              checked={deleteApproval}
              disabled={saving}
              onChange={(v) => void toggleDeleteApproval(v)}
            />
            <MacSettingsGroupBody className="pt-0">
              <p className="text-xs text-[var(--text-muted)]">Audit logging is always enabled for platform operations.</p>
            </MacSettingsGroupBody>
          </MacSettingsGroup>

          {enterprise && (
            <MacSettingsGroup title="Keychain">
              <p className="text-xs text-[var(--text-muted)] mb-2">Fleet secrets inventory — API keys, air-gap bundles.</p>
              <Link to="/platform/enterprise?tab=keychain" className={`text-sm ${hubLinkClasses()}`}>Open Keychain →</Link>
            </MacSettingsGroup>
          )}

          {enterprise && (
            <MacSettingsGroup title="Enterprise security">
              <p className="text-xs text-[var(--text-muted)] mb-2">{enterprise.summary}</p>
              <div className="grid gap-3 sm:grid-cols-2 text-sm mb-3">
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">Tenant policies</p>
                  <p className="text-lg font-semibold text-[var(--text-primary)]">{enterprise.tenant_policies}</p>
                </div>
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">Air-gap</p>
                  <p className="text-lg font-semibold text-[var(--text-primary)]">{enterprise.air_gap_bundles}</p>
                  <p className="text-[10px] text-[var(--text-muted)]">bundles</p>
                </div>
              </div>
            </MacSettingsGroup>
          )}

          <MacSettingsGroup title="Air-gap bundles">
            <p className="text-xs text-[var(--text-muted)] mb-2">Simulated sovereign export manifests — no live bundle runner.</p>
            <div className="flex gap-2 mb-3">
              <input
                aria-label="Air-gap bundle name"
                className="input flex-1 text-sm"
                value={bundleName}
                disabled={bundleCreating}
                onChange={(e) => setBundleName(e.target.value)}
                placeholder="Bundle name"
              />
              <button type="button" className="btn-secondary text-xs shrink-0" disabled={bundleCreating} onClick={() => void exportAirGapBundle()}>
                {bundleCreating ? 'Creating…' : 'Create manifest'}
              </button>
            </div>
            {airGapBundles.slice(0, 5).map((b) => (
              <MacListRow
                key={b.id}
                title={b.name}
                subtitle={`${b.checksum.slice(0, 24)}… · ${Math.round(b.size_bytes / 1024)} KB`}
                onClick={() => void getAirGapBundle(b.id).then(setSelectedBundle).catch(() => toast.error('Bundle not found'))}
              />
            ))}
            {selectedBundle && (
              <JsonInspector data={selectedBundle.manifest_json} className="mt-2">
                <dl className="grid gap-2 sm:grid-cols-2 text-sm">
                  <div className="tahoe-glass-card px-3 py-2">
                    <dt className="text-xs text-[var(--text-muted)]">Bundle</dt>
                    <dd className="text-[var(--text-primary)] mt-0.5">{selectedBundle.name}</dd>
                  </div>
                  <div className="tahoe-glass-card px-3 py-2">
                    <dt className="text-xs text-[var(--text-muted)]">Size</dt>
                    <dd className="text-[var(--text-primary)] mt-0.5">{Math.round(selectedBundle.size_bytes / 1024)} KB</dd>
                  </div>
                  <div className="tahoe-glass-card px-3 py-2 sm:col-span-2">
                    <dt className="text-xs text-[var(--text-muted)]">Checksum</dt>
                    <dd className="text-[var(--text-primary)] mt-0.5 font-mono text-xs break-all">{selectedBundle.checksum}</dd>
                  </div>
                  {asArray(asRecord(selectedBundle.manifest_json)?.artifacts).slice(0, 6).map((item, i) => {
                    const row = asRecord(item)
                    return (
                      <div key={String(row?.path ?? row?.name ?? i)} className="tahoe-glass-card px-3 py-2 sm:col-span-2">
                        <dt className="text-xs text-[var(--text-muted)]">Artifact</dt>
                        <dd className="text-[var(--text-primary)] mt-0.5 text-xs">{String(row?.path ?? row?.name ?? JSON.stringify(item))}</dd>
                      </div>
                    )
                  })}
                </dl>
              </JsonInspector>
            )}
          </MacSettingsGroup>

          <MacSettingsGroup title="Policy rules">
            {policyRules.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No rules — <Link to="/platform/policy" className={hubLinkClasses()}>open Policy & Quotas</Link></p>
            ) : (
              policyRules.slice(0, 5).map((r) => (
                <MacListRow key={r.id} title={r.name} subtitle={r.enabled ? 'Enabled' : 'Disabled'} />
              ))
            )}
          </MacSettingsGroup>

          <MacSettingsGroup title="Zeus Firewall">
            {firewallOverview ? (
              <MacListRow
                title="Zeus Firewall"
                subtitle={`${firewallOverview.summary} · ${firewallOverview.targets.length} targets · ${firewallOverview.critical_count} critical`}
                href="/platform/zeus/security/firewall"
              />
            ) : (
              <p className="text-sm text-[var(--text-muted)]">Firewall overview unavailable — agent may be offline.</p>
            )}
            {firewallOverview && firewallOverview.critical_count > 0 && (
              <p className={`text-xs px-1 mt-2 ${statusToneClass('warn')}`}>
                {firewallOverview.critical_count} machine(s) need attention — open Zeus Firewall for detail.
              </p>
            )}
          </MacSettingsGroup>

          <MacSettingsGroup title="Policy Generator">
            <p className="text-xs text-[var(--text-muted)] mb-2">Export cluster policy rules and quotas as YAML.</p>
            <button type="button" className="btn-secondary text-xs" onClick={async () => {
              try {
                const r = await getAiPolicyExport()
                setPolicyYaml(r.yaml)
                toast.success(`Exported ${r.rule_count} rules, ${r.quota_count} quotas`)
              } catch (e: unknown) { toast.error(formatUserError(e)) }
            }}>Generate YAML</button>
            {policyYaml && (
              <textarea className="input font-mono text-xs min-h-32 w-full mt-2" readOnly value={policyYaml} aria-label="Exported policy YAML" />
            )}
          </MacSettingsGroup>
        </div>
      )}

      {section === 'network' && (
        <div className="space-y-6">
          {fleetNetwork && (
            <>
              <p className="text-sm text-[var(--text-muted)]">{fleetNetwork.summary}</p>
              <div className="grid gap-3 sm:grid-cols-4 text-sm">
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">Networks</p>
                  <p className="text-lg font-semibold">{fleetNetwork.network_count}</p>
                </div>
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">Segments</p>
                  <p className="text-lg font-semibold">{fleetNetwork.segment_count}</p>
                </div>
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">IPAM pools</p>
                  <p className="text-lg font-semibold">{fleetNetwork.ipam_pool_count}</p>
                </div>
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">Deny east-west</p>
                  <p className={`text-lg font-semibold ${statusToneClass('warn')}`}>{fleetNetwork.deny_east_west_count}</p>
                </div>
              </div>
            </>
          )}

          <MacSettingsGroup title="Network Lens">
            <p className="text-sm text-[var(--text-muted)] mb-2">macOS-style reachability explain — why can&apos;t VM A reach VM B?</p>
            <Link to="/platform/networks?tab=lens" className={`text-sm block ${hubLinkClasses()}`}>Open Network Lens →</Link>
            <Link to="/platform/topology" className={`text-sm block mt-1 ${hubLinkClasses()}`}>Topology & digital twin →</Link>
          </MacSettingsGroup>

          <MacSettingsGroup title="Overlay segments">
            {(fleetNetwork?.segments ?? []).length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No overlay segments yet.</p>
            ) : (
              fleetNetwork!.segments.slice(0, 8).map((s) => (
                <MacListRow
                  key={s.id}
                  title={s.name}
                  subtitle={`${s.cidr} · ${s.vm_count} VM(s) · east-west ${s.east_west_default}`}
                  badge={<span className="text-[10px] text-[var(--link)]">{s.micro_seg_grade}</span>}
                  href={`/platform/networks?tab=segments`}
                />
              ))
            )}
            <Link to="/platform/networks" className={`text-sm inline-block mt-2 ${hubLinkClasses()}`}>Manage networks →</Link>
          </MacSettingsGroup>

          <MacSettingsGroup title="Hypervisor networking">
            <MacListRow title="Host systemd network" subtitle="networkd + resolved per hypervisor" href="/platform/hosts" />
          </MacSettingsGroup>

          <MacSettingsGroup title="Networks">
            <MacListRow title="Virtual networks" subtitle="Bridges, VLANs, and IP pools" href="/platform/networks" />
          </MacSettingsGroup>

          <MacSettingsGroup title="Firewall">
            {firewallOverview && (
              <div className="grid gap-3 sm:grid-cols-3 text-sm mb-3">
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">Targets</p>
                  <p className="text-lg font-semibold text-[var(--text-primary)]">{firewallOverview.targets.length}</p>
                </div>
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">Critical</p>
                  <p className={`text-lg font-semibold ${statusToneClass('error')}`}>{firewallOverview.critical_count}</p>
                </div>
                <div className="tahoe-glass-card p-3">
                  <p className="text-[10px] uppercase text-[var(--text-muted)]">Warnings</p>
                  <p className={`text-lg font-semibold ${statusToneClass('warn')}`}>{firewallOverview.warning_count}</p>
                </div>
              </div>
            )}
            <label className="block text-sm text-[var(--text-muted)] mb-2">
              Risky profile change approval SLA (hours)
              <input
                type="number"
                min={1}
                max={720}
                className="input mt-1 block w-32"
                value={approvalSlaHours}
                disabled={saving}
                onChange={(e) => setApprovalSlaHours(Number(e.target.value))}
              />
            </label>
            <button type="button" className="btn-secondary text-xs" disabled={saving} onClick={() => void saveApprovalSla()}>
              Save SLA
            </button>
            <Link to="/platform/zeus/security/firewall" className={`text-sm block mt-3 ${hubLinkClasses()}`}>
              Open Zeus Firewall hub →
            </Link>
          </MacSettingsGroup>
        </div>
      )}

      {section === 'updates' && (
        <MacSettingsGroup title="Updates">
          <p className="text-sm text-[var(--text-muted)]">Controller upgrade matrix and rollout planning.</p>
          <Link to="/platform/upgrade" className={`text-sm inline-block mt-2 ${hubLinkClasses()}`}>Open upgrade matrix →</Link>
        </MacSettingsGroup>
      )}

      {section === 'users' && (
        <div>
          <SettingsWorkspaceLink to="/platform/users" label="Open full Users workspace" />
          <PlatformUsers embedded />
        </div>
      )}

      {section === 'stage-manager' && (
        <div>
          <SettingsWorkspaceLink to="/platform/projects" label="Open full Stage Manager workspace" />
          <PlatformProjects embedded />
        </div>
      )}

      {section === 'keychain' && (
        <div>
          <SettingsWorkspaceLink to="/platform/enterprise" label="Open full Keychain workspace" />
          <PlatformEnterprise embedded />
        </div>
      )}

      {section === 'policy' && (
        <div>
          <SettingsWorkspaceLink to="/platform/policy" label="Open full Policy workspace" />
          <PlatformPolicy embedded />
        </div>
      )}

      {section === 'api-keys' && (
        <div>
          <SettingsWorkspaceLink to="/platform/api-keys" label="Open full API Keys workspace" />
          <PlatformApiKeys embedded />
        </div>
      )}

      {section === 'webhooks' && (
        <div>
          <SettingsWorkspaceLink to="/platform/webhooks" label="Open full Webhooks workspace" />
          <PlatformWebhooks embedded />
        </div>
      )}

      {section === 'reports' && (
        <div>
          <SettingsWorkspaceLink to="/platform/reports" label="Open full Reports workspace" />
          <PlatformReports embedded />
        </div>
      )}

      {section === 'console' && (
        <div>
          <SettingsWorkspaceLink to="/platform/events" label="Open full Console workspace" />
          <PlatformEvents embedded />
        </div>
      )}

      {section === 'resources' && (
        <div>
          <SettingsWorkspaceLink to="/platform/storage" label="Open Infrastructure" />
        </div>
      )}

      {section === 'integrations' && (
        <div>
          <PlatformIntegrations embedded />
        </div>
      )}
    </MacSettingsPane>
    </PlatformPageChrome>
  )
}

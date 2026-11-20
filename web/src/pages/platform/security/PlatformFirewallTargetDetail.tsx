// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft, ChevronRight, Lock, Shield } from 'lucide-react'
import {
  MacGlassPanel,
  MacListRow,
  MacSegmentedControl,
  MacSettingsGroup,
  MacSheet,
  MacToggle,
} from '../../../components/platform/mac/PlatformMacUi'
import DetailTabs from '../../../components/platform/DetailTabs'
import ConfirmDialog from '../../../components/ConfirmDialog'
import PageLayout from '../../../components/PageLayout'
import JsonInspector from '../../../components/platform/JsonInspector'
import { formatAllowedFrom } from '../../../utils/firewallDisplay'
import {
  applyFirewall,
  applyFirewallProfile,
  detectFirewallDrift,
  explainFirewall,
  getFirewallServices,
  getFirewallTarget,
  getFirewallTimeline,
  listFirewallCheckpoints,
  listFirewallProfiles,
  lockdownMachine,
  planFirewall,
  rollbackFirewall,
  scanBaremetalExposure,
  createBaremetalTemporaryRule,
  secureMachinePlan,
  type AllowedService,
  type FirewallTargetDetail,
} from '../../../api/zeusFirewall'
import { useBreadcrumbName } from '../../../contexts/BreadcrumbNameContext'
import { useToastContext } from '../../../contexts/ToastContext'
import { formatUserError } from '../../../utils/apiError'
import { hubLinkClasses, riskTone, statusBadgeClasses, statusPillClasses, statusSurfaceClasses, statusToneClass } from '../../../utils/semanticColors'
import { useExpandable } from '../../../hooks/useExpandable'
import { ExpandableToggle } from '../../../components/ui/ExpandableToggle'

type StealthLevel = 'off' | 'standard' | 'strict'
type PaneId = 'firewall' | 'connections' | 'advanced'

const STEALTH_OPTIONS: Array<{ value: StealthLevel; label: string }> = [
  { value: 'off', label: 'Off' },
  { value: 'standard', label: 'Standard' },
  { value: 'strict', label: 'Strict' },
]

const PRIMARY_PANES: Array<{ id: PaneId; label: string }> = [
  { id: 'firewall', label: 'Firewall' },
  { id: 'connections', label: 'Incoming' },
  { id: 'advanced', label: 'Advanced' },
]

export default function PlatformFirewallTargetDetail() {
  const { id } = useParams<{ id: string }>()
  const toast = useToastContext()
  const [pane, setPane] = useState<PaneId>('firewall')
  const [detail, setDetail] = useState<FirewallTargetDetail | null>(null)
  useBreadcrumbName(detail?.target.name ?? '')
  const [services, setServices] = useState<AllowedService[]>([])
  const [profiles, setProfiles] = useState<Array<{ name: string; display_name: string }>>([])
  const [selectedProfile, setSelectedProfile] = useState('ProductionServer')
  const [stealth, setStealth] = useState<StealthLevel>('off')
  const [drift, setDrift] = useState<string | null>(null)
  const [timeline, setTimeline] = useState<Array<Record<string, unknown>>>([])
  const [checkpoints, setCheckpoints] = useState<Array<{ id: string; label: string; created_at: string }>>([])
  const checkpointList = useExpandable(checkpoints, 20)
  const timelineList = useExpandable(timeline, 20)
  const [aiExplain, setAiExplain] = useState<string | null>(null)
  const [securePlan, setSecurePlan] = useState<string | null>(null)
  const [previewSheet, setPreviewSheet] = useState<{ open: boolean; body: string }>({ open: false, body: '' })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [pendingApply, setPendingApply] = useState<{ body: Record<string, unknown>; message: string } | null>(null)
  const [confirmLockdown, setConfirmLockdown] = useState(false)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a slow fetch for a
    // prior target can't overwrite the target the user navigated to.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    try {
      const [d, profs, svc, tl, cps, dr] = await Promise.all([
        getFirewallTarget(id),
        listFirewallProfiles(),
        getFirewallServices(id).catch(() => []),
        getFirewallTimeline(id).catch(() => []),
        listFirewallCheckpoints(id).catch(() => []),
        detectFirewallDrift(id).catch(() => null),
      ])
      if (!alive()) return
      setDetail(d)
      setProfiles(profs)
      setServices(svc)
      setTimeline(tl)
      setCheckpoints(cps)
      setSelectedProfile(d.target.profile || (d.target.kind === 'bare_metal' ? 'BareMetalBmc' : 'ProductionServer'))
      const sl = d.inventory.posture.stealth_level?.toLowerCase()
      if (sl === 'standard' || sl === 'strict') setStealth(sl)
      else setStealth('off')
      if (dr) {
        setDrift(dr.drift_detected ? `${dr.summary} — expected: ${dr.expected}` : dr.summary)
      }
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    }
  }, [id])

  useEffect(() => { void load() }, [load])

  // Clear the previous target's detail on navigation so its name/score/posture
  // don't flash under the new target (contentLoading gates on !detail).
  useEffect(() => { setDetail(null) }, [id])

  const applyWithPreview = async (body: Record<string, unknown>, confirmMsg: string) => {
    if (!id) return
    try {
      const preview = await planFirewall(id, { ...body, dry_run: true })
      setPreviewSheet({
        open: true,
        body: JSON.stringify({
          operations: preview.operations,
          warnings: preview.diff?.warnings,
        }, null, 2),
      })
      setPendingApply({ body, message: confirmMsg })
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const runPendingApply = async () => {
    if (!id || !pendingApply) return
    const { body } = pendingApply
    setPendingApply(null)
    try {
      setBusy(true)
      await applyFirewall(id, { ...body, dry_run: false })
      toast.success('Firewall updated')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  if (!id) return null
  const inv = detail?.inventory
  const isMetal = detail?.target.kind === 'bare_metal'
  const score = inv?.score.score ?? 0
  const scoreTone = score >= 80 ? 'ok' : score >= 50 ? 'warn' : 'error'

  return (
    <PageLayout
      compact
      contentLoading={!detail && !error}
      error={error}
      prepend={
        <Link to="/platform/zeus/security/firewall" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Firewall
        </Link>
      }
      title={detail?.target.name ?? 'Machine firewall'}
      subtitle={detail && inv ? (
        <span className="flex flex-wrap items-center gap-2 text-sm">
          <span className={statusPillClasses(scoreTone)}>Score {score}/100</span>
          <span className={statusPillClasses(inv.posture.enabled ? 'ok' : 'warn')}>{inv.posture.enabled ? 'Enabled' : 'Disabled'}</span>
          <span className="text-[var(--text-muted)]">{isMetal ? 'Bare metal BMC/PXE' : detail.target.backend}</span>
        </span>
      ) : undefined}
      icon={<Shield className="w-6 h-6 text-[var(--text-muted)]" />}
      contentClassName="space-y-4"
    >
      {detail && inv && (
        <>
          {isMetal && (
            <div className={`rounded-xl px-4 py-3 text-sm ${statusSurfaceClasses('warn')}`}>
              Policy-only — live BMC firewall apply is on the roadmap. Profiles and scans update desired posture in Zeus OS.
            </div>
          )}
          <DetailTabs primary={PRIMARY_PANES} active={pane} onChange={setPane} />
          <MacGlassPanel title={PRIMARY_PANES.find((p) => p.id === pane)?.label ?? 'Security'}>
            {pane === 'firewall' && (
              <div className="space-y-4 -mt-1">
                <MacSettingsGroup title="Firewall">
                  <div className="px-4 py-2">
                    <MacToggle
                      checked={inv.posture.enabled}
                      disabled={busy}
                      label="Firewall"
                      description="Block incoming connections to this machine"
                      onChange={(on) => {
                        void applyWithPreview(
                          { enable: on, profile: selectedProfile, stealth_level: stealth === 'off' ? undefined : stealth },
                          on ? 'Turn firewall on?' : 'Turn firewall off? All incoming may be allowed.',
                        )
                      }}
                    />
                  </div>
                </MacSettingsGroup>
                <MacSettingsGroup title="Stealth Mode">
                  <div className="px-4 py-3">
                    <MacSegmentedControl
                      label="Reduce probe visibility"
                      options={STEALTH_OPTIONS}
                      value={stealth}
                      onChange={(v) => {
                        setStealth(v)
                        void applyWithPreview(
                          {
                            enable: true,
                            profile: selectedProfile,
                            stealth_level: v === 'off' ? undefined : v,
                          },
                          `Apply stealth mode: ${v}?`,
                        )
                      }}
                    />
                  </div>
                </MacSettingsGroup>
                <MacSettingsGroup title="Profile">
                  <div className="px-4 py-3 space-y-3">
                    <select
                      aria-label="Firewall profile"
                      className="input text-sm w-full max-w-md"
                      value={selectedProfile}
                      onChange={(e) => setSelectedProfile(e.target.value)}
                    >
                      {profiles.map((p) => (
                        <option key={p.name} value={p.name}>{p.display_name}</option>
                      ))}
                    </select>
                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        className="btn-secondary text-xs"
                        disabled={busy}
                        onClick={async () => {
                          try {
                            const r = await applyFirewallProfile(id, selectedProfile, true)
                            setPreviewSheet({
                              open: true,
                              body: JSON.stringify({ operations: r.operations }, null, 2),
                            })
                          } catch (e: unknown) {
                            toast.error(formatUserError(e))
                          }
                        }}
                      >
                        Preview changes
                      </button>
                      <button
                        type="button"
                        className="btn-primary text-xs"
                        disabled={busy}
                        onClick={() => void applyWithPreview(
                          { enable: true, profile: selectedProfile },
                          `Apply profile ${selectedProfile}?`,
                        )}
                      >
                        Apply profile
                      </button>
                    </div>
                  </div>
                </MacSettingsGroup>
                {drift && (
                  <MacGlassPanel title="Drift detected" subtitle="Changed outside Zeus OS">
                    <p className={`text-sm ${statusToneClass('warn')}`}>{drift}</p>
                  </MacGlassPanel>
                )}
                <div className="mt-4 flex flex-wrap gap-2 text-sm">
                  <Link to="/platform/zeus/security/ports" className={hubLinkClasses()}>Open Ports</Link>
                  <Link to="/platform/zeus/security/activity" className={hubLinkClasses()}>Activity</Link>
                  <Link to="/platform/zeus/security/compliance" className={hubLinkClasses()}>Compliance</Link>
                </div>
              </div>
            )}
            {pane === 'connections' && (
              <div className="space-y-4 -mt-1">
                <MacSettingsGroup title="Allowed incoming connections">
                  {services.length === 0 && inv.open_ports.length === 0 ? (
                    <p className="px-4 py-3 text-sm text-[var(--text-muted)]">No mapped services — scan exposure on Advanced tab.</p>
                  ) : (
                    (services.length > 0 ? services.map((s) => (
                      <MacListRow
                        key={`${s.name}-${s.port}`}
                        title={s.name}
                        subtitle={`${s.protocol}/${s.port} · Allowed from ${formatAllowedFrom(s.allowed_from)}`}
                        badge={
                          <span className="text-xs px-2 py-0.5 rounded-full bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]">
                            {String(s.status)}
                          </span>
                        }
                      />
                    )) : inv.open_ports.map((p) => (
                      <MacListRow
                        key={`${p.port}-${p.protocol}`}
                        title={`${p.service_name} (${p.port}/${p.protocol})`}
                        subtitle={`Bind ${p.bind_address} · ${formatAllowedFrom(p.allowed_from)}`}
                        badge={
                          <span className={`text-xs px-2 py-0.5 rounded-full ${
                            riskTone(String(p.risk)) === 'neutral'
                              ? 'bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]'
                              : statusBadgeClasses(riskTone(String(p.risk)))
                          }`}>
                            {String(p.risk)}
                          </span>
                        }
                      />
                    )))
                  )}
                </MacSettingsGroup>
                <MacSettingsGroup title="Quick rules">
                  <MacListRow
                    title="Allow SSH (admin subnet only)"
                    subtitle="Dry-run preset"
                    trailing={<ChevronRight className="w-4 h-4 text-[var(--text-faint)]" />}
                    onClick={async () => {
                      try {
                        const r = await planFirewall(id, { preset: 'allow_ssh', dry_run: true })
                        setPreviewSheet({ open: true, body: JSON.stringify(r, null, 2) })
                      } catch (e: unknown) {
                        toast.error(formatUserError(e))
                      }
                    }}
                  />
                </MacSettingsGroup>
              </div>
            )}
            {pane === 'advanced' && (
              <div className="space-y-4 -mt-1">
                {isMetal && (
                  <MacSettingsGroup title="BMC / PXE exposure">
                    <MacListRow
                      title="Run exposure scan"
                      subtitle="IPMI 623 / Redfish 443 heuristic"
                      onClick={async () => {
                        try {
                          const r = await scanBaremetalExposure(id)
                          setPreviewSheet({ open: true, body: JSON.stringify(r, null, 2) })
                          toast.success('Exposure scan complete')
                          void load()
                        } catch (e: unknown) {
                          toast.error(formatUserError(e))
                        }
                      }}
                    />
                    <MacListRow
                      title="Temporary PXE allow (1h) — record only"
                      subtitle="Audit record of ports 67/69 from admin subnet — does not touch the host firewall"
                      onClick={async () => {
                        try {
                          await createBaremetalTemporaryRule(id, { preset: 'pxe', reason: 'Provisioning window' })
                          toast.success('Recorded for audit — no host firewall change was made')
                          void load()
                        } catch (e: unknown) {
                          toast.error(formatUserError(e))
                        }
                      }}
                    />
                    <MacListRow
                      title="Temporary BMC access (4h) — record only"
                      subtitle="Audit record of IPMI/Redfish from admin subnet — does not touch the host firewall"
                      onClick={async () => {
                        try {
                          await createBaremetalTemporaryRule(id, { preset: 'bmc', reason: 'Break-glass BMC' })
                          toast.success('Recorded for audit — no host firewall change was made')
                          void load()
                        } catch (e: unknown) {
                          toast.error(formatUserError(e))
                        }
                      }}
                    />
                  </MacSettingsGroup>
                )}
                <MacSettingsGroup title="Emergency">
                  <MacListRow
                    title="Lock Down Machine"
                    subtitle="Emergency Isolation — blocks all traffic except management"
                    trailing={<Lock className={`w-4 h-4 ${statusToneClass('error')}`} />}
                    onClick={() => setConfirmLockdown(true)}
                  />
                  <MacListRow
                    title="Secure This Machine (AI plan)"
                    subtitle="Review steps before apply"
                    onClick={async () => {
                      try {
                        const r = await secureMachinePlan(id)
                        setSecurePlan(r.steps.map((s) => `${s.step}. ${s.action}`).join('\n'))
                      } catch (e: unknown) {
                        toast.error(formatUserError(e))
                      }
                    }}
                  />
                </MacSettingsGroup>
                {securePlan && (
                  <MacGlassPanel title="AI Secure Plan">
                    <p className="text-sm text-[var(--text-secondary)] whitespace-pre-wrap leading-relaxed">{securePlan}</p>
                  </MacGlassPanel>
                )}
                <MacSettingsGroup title="AI Explain">
                  <MacListRow
                    title="Why is this machine exposed?"
                    onClick={async () => {
                      try {
                        const r = await explainFirewall(id, 'Why is this machine exposed?')
                        setAiExplain(`${r.summary}\n\n${r.evidence.join('\n')}\n\nRecommended: ${r.recommendation}`)
                      } catch (e: unknown) {
                        toast.error(formatUserError(e))
                      }
                    }}
                  />
                </MacSettingsGroup>
                {aiExplain && (
                  <MacGlassPanel title="Exposure analysis">
                    <p className="text-sm text-[var(--text-secondary)] whitespace-pre-wrap leading-relaxed">{aiExplain}</p>
                  </MacGlassPanel>
                )}
                {checkpoints.length > 0 && (
                  <MacSettingsGroup title="Rollback checkpoints">
                    <div id={checkpointList.listId}>
                    {checkpointList.shown.map((c) => (
                      <MacListRow
                        key={c.id}
                        title={c.label}
                        subtitle={new Date(c.created_at).toLocaleString()}
                        trailing={
                          <button
                            type="button"
                            className={`text-xs ${hubLinkClasses()}`}
                            onClick={async (ev) => {
                              ev.stopPropagation()
                              try {
                                await rollbackFirewall(id, c.id)
                                toast.success('Rollback recorded — host firewall was NOT changed; re-apply manually via agent plan')
                                void load()
                              } catch (e: unknown) {
                                toast.error(formatUserError(e))
                              }
                            }}
                          >
                            Rollback
                          </button>
                        }
                      />
                    ))}
                    </div>
                    {checkpointList.showToggle && (
                      <ExpandableToggle expanded={checkpointList.expanded} hidden={checkpointList.hidden} listId={checkpointList.listId} onToggle={checkpointList.toggle} noun="checkpoints" className="btn-secondary text-sm mt-2" />
                    )}
                  </MacSettingsGroup>
                )}
                {timeline.length > 0 && (
                  <MacGlassPanel title="Timeline">
                    <ul className="space-y-2 text-xs text-[var(--text-muted)]" id={timelineList.listId}>
                      {timelineList.shown.map((e, i) => (
                        <li key={`${String(e.created_at)}-${i}`}>
                          {String(e.created_at || '')} — {String(e.summary || e.kind || '')}
                        </li>
                      ))}
                    </ul>
                    {timelineList.showToggle && (
                      <ExpandableToggle expanded={timelineList.expanded} hidden={timelineList.hidden} listId={timelineList.listId} onToggle={timelineList.toggle} noun="events" className="btn-secondary text-sm mt-2" />
                    )}
                  </MacGlassPanel>
                )}
                <MacGlassPanel title="Score">
                  <ul className="space-y-2 text-sm">
                    {inv.score.breakdown.map((b) => (
                      <li key={b.category} className="flex justify-between text-[var(--text-secondary)]">
                        <span>{b.detail}</span>
                        <span className={b.points < 0 ? statusToneClass('warn') : 'text-[var(--text-muted)]'}>{b.points}</span>
                      </li>
                    ))}
                  </ul>
                </MacGlassPanel>
              </div>
            )}
          </MacGlassPanel>
        </>
      )}
      <MacSheet
        open={previewSheet.open}
        onClose={() => setPreviewSheet({ open: false, body: '' })}
        title="Preview changes"
        subtitle="Review before applying on the host"
        wide
      >
        {(() => {
          try {
            const parsed = JSON.parse(previewSheet.body) as unknown
            return <JsonInspector data={parsed} />
          } catch {
            return <p className="text-sm text-[var(--text-secondary)] whitespace-pre-wrap font-mono">{previewSheet.body}</p>
          }
        })()}
      </MacSheet>
      <ConfirmDialog
        open={!!pendingApply}
        title="Apply firewall change"
        message={pendingApply?.message ?? ''}
        confirmLabel="Apply"
        variant="warning"
        onCancel={() => setPendingApply(null)}
        onConfirm={() => void runPendingApply()}
      />
      <ConfirmDialog
        open={confirmLockdown}
        title="Emergency Isolation"
        message="Enable Emergency Isolation lockdown?"
        confirmLabel="Enable"
        variant="warning"
        onCancel={() => setConfirmLockdown(false)}
        onConfirm={async () => {
          setConfirmLockdown(false)
          if (!id) return
          try {
            await lockdownMachine(id, true)
            toast.success('Lockdown initiated')
            void load()
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          }
        }}
      />
    </PageLayout>
  )
}

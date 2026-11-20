// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import {
  createSecurityGroup,
  createSecurityGroupRule,
  deleteSecurityGroup,
  deleteSecurityGroupRule,
  getSecurityGroup,
  listSecurityGroupRules,
  listSecurityGroups,
  previewSecurityGroupEnforcement,
  setSecurityGroupMode,
  type EnforcePreview,
  type NativeSecurityGroup,
  type NativeSecurityGroupRule,
} from '../api/securityGroups'
import ConfirmDialog from '../components/ConfirmDialog'
import { useToastContext } from '../contexts/ToastContext'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { Loader2, RefreshCw, Shield } from 'lucide-react'

// Native security groups — this feature is libvirt/SQLite-native and does not
// depend on a wired external cloud (there's no old external-cloud gate
// component to gate them behind either: the daemon's external-cloud-client
// integration has since been fully removed).
export default function FleetCloudSecurityGroupsPage() {
  return <FleetCloudSecurityGroupsContent />
}

function FleetCloudSecurityGroupsContent() {
  const toast = useToastContext()
  const [groups, setGroups] = useState<NativeSecurityGroup[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<NativeSecurityGroup | null>(null)
  const [rules, setRules] = useState<NativeSecurityGroupRule[]>([])
  const [loading, setLoading] = useState(true)
  const [detailLoading, setDetailLoading] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deleteGroupTarget, setDeleteGroupTarget] = useState<NativeSecurityGroup | null>(null)
  const [deletingGroup, setDeletingGroup] = useState(false)
  const [newSgName, setNewSgName] = useState('')
  const [creatingSg, setCreatingSg] = useState(false)
  const [search, setSearch] = useState('')
  const [armEnforce, setArmEnforce] = useState(false)
  const [enforcePreview, setEnforcePreview] = useState<EnforcePreview | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setLoadError(null)
    try {
      const security_groups = await listSecurityGroups()
      setGroups(security_groups)
      if (security_groups.length > 0) {
        setSelectedId((prev) => prev ?? security_groups[0].id)
      }
    } catch (e: unknown) {
      const msg = formatUserError(e)
      setLoadError(msg)
      toast.error(`Failed to load security groups: ${msg}`)
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => {
    void load()
  }, [load])

  const loadDetail = useCallback(async (id: string) => {
    const [g, r] = await Promise.all([getSecurityGroup(id), listSecurityGroupRules(id)])
    setDetail(g)
    setRules(r)
  }, [])

  useEffect(() => {
    if (!selectedId) {
      setDetail(null)
      setRules([])
      return
    }
    // Clear the previous group's rules immediately and guard against an
    // out-of-order response committing stale detail after a fast re-selection.
    let cancelled = false
    setDetail(null)
    setRules([])
    setDetailLoading(true)
    loadDetail(selectedId)
      .catch((e: unknown) => {
        if (cancelled) return
        toast.error(formatUserError(e))
        setDetail(null)
      })
      .finally(() => { if (!cancelled) setDetailLoading(false) })
    return () => { cancelled = true }
  }, [selectedId, loadDetail, toast])

  const active = detail ?? groups.find((g) => g.id === selectedId) ?? null

  const filteredGroups = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return groups
    return groups.filter(
      (g) =>
        g.name.toLowerCase().includes(q) ||
        g.id.toLowerCase().includes(q) ||
        (g.description?.toLowerCase().includes(q) ?? false),
    )
  }, [groups, search])

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
      error={loadError}
      errorTitle="Failed to load"
      technicalDetail={loadError}
      errorTone="red"
      onErrorRetry={() => void load()}
      onErrorDismiss={() => setLoadError(null)}
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
          <Shield className="w-7 h-7 text-[var(--accent)]" />
          Security groups
        </h1>
        <button
          type="button"
          onClick={() => void load()}
          className="btn-secondary text-sm inline-flex items-center gap-2"
        >
          <RefreshCw className="w-4 h-4" />
          Refresh
        </button>
      </div>
      <p className="text-xs text-[var(--text-muted)] -mt-2">
        Groups are advisory until you switch them to Enforce; the badge shows what each host&apos;s VM edge reports.
      </p>

      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 flex flex-wrap gap-2 items-end text-sm">
        <input id="new-sg-name" placeholder="New group name" aria-label="New security group name"
          value={newSgName} onChange={(e) => setNewSgName(e.target.value)}
          className="input-field" />
        <button type="button" disabled={creatingSg || !newSgName.trim()}
          className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed"
          onClick={async () => {
            const name = newSgName.trim()
            if (!name || creatingSg) return
            setCreatingSg(true)
            try {
              await createSecurityGroup({ name })
              toast.success('Security group created')
              setNewSgName('')
              void load()
            } catch (e: unknown) {
              toast.error(formatUserError(e))
            } finally {
              setCreatingSg(false)
            }
          }}>{creatingSg ? 'Creating…' : 'Create group'}</button>
        <Link to="/fleet-cloud/instances" className="text-[var(--accent)] hover:underline ml-auto text-xs">
          Attach on instance detail
        </Link>
      </div>

      {loading ? (
        <div className="py-12 text-center text-[var(--text-muted)] flex flex-col items-center gap-3">
          <Loader2 className="w-8 h-8 animate-spin text-[var(--accent)]" />
          Loading…
        </div>
      ) : groups.length === 0 ? (
        <p className="text-[var(--text-muted)] text-sm">No security groups in this project.</p>
      ) : (
        <div className="flex flex-col gap-4 w-full">
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search name or ID…"
          />

          <TahoeTableWrap>
            <table className="apple-table" aria-label="Security groups">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Description</th>
                  <th scope="col">ID</th>
                </tr>
              </thead>
              <tbody>
                {filteredGroups.length === 0 && (
                  <tr>
                    <td colSpan={3} className="text-center text-[var(--text-muted)]">
                      {search.trim() ? 'No security groups match your search.' : 'No security groups.'}
                    </td>
                  </tr>
                )}
                {filteredGroups.map((g) => (
                  <tr
                    key={g.id}
                    onClick={() => setSelectedId(g.id)}
                    className={`cursor-pointer ${selectedId === g.id ? 'bg-[var(--accent-soft)]' : ''}`}
                  >
                    <td>
                      <Link
                        to={`/fleet-cloud/security-groups/${g.id}`}
                        className="apple-link font-medium"
                        onClick={(e) => e.stopPropagation()}
                      >
                        {g.name}
                      </Link>
                    </td>
                    <td className="text-[var(--text-muted)] text-sm">{g.description || '—'}</td>
                    <td className="text-xs font-mono text-[var(--text-faint)] truncate max-w-[220px]">{g.id}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>

          <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 min-h-[12rem]">
            {detailLoading && (
              <Loader2 className="w-5 h-5 animate-spin text-[var(--accent)] mb-2" />
            )}
            {active ? (
              <>
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <h2 className="text-lg font-medium text-[var(--text-primary)]">
                    <Link to={`/fleet-cloud/security-groups/${active.id}`} className="text-[var(--link)] hover:underline">{active.name}</Link>
                  </h2>
                  <button
                    type="button"
                    className={statusActionLinkClasses('error', 'text-xs')}
                    onClick={() => setDeleteGroupTarget(active)}
                  >
                    Delete group
                  </button>
                </div>
                {active.description && (
                  <p className="text-sm text-[var(--text-muted)] mt-1">{active.description}</p>
                )}
                <p className="text-xs font-mono text-[var(--text-faint)] mt-2 break-all">{active.id}</p>
                <div className="mt-3 flex flex-wrap items-center gap-2 text-xs" data-testid="sg-enforcement">
                  <span className="rounded-full border border-[var(--apple-hairline)] px-2 py-0.5 font-medium">
                    {active.enforcement?.state ?? 'advisory'}
                  </span>
                  <span className="text-[var(--text-muted)]">{active.enforcement?.reason}</span>
                  <button type="button" className="px-2 py-1 rounded border border-[var(--apple-hairline)]"
                    onClick={async () => {
                      const next = active.mode === 'enforce' ? 'audit' : 'enforce'
                      if (next === 'enforce' && !armEnforce) {
                        try { setEnforcePreview(await previewSecurityGroupEnforcement(active.id)) } catch (e: unknown) { toast.error(formatUserError(e)); return }
                        setArmEnforce(true)
                        return
                      }
                      setArmEnforce(false)
                      setEnforcePreview(null)
                      try {
                        await setSecurityGroupMode(active.id, next)
                        toast.success(next === 'enforce' ? 'Enforcing' : 'Back to advisory')
                        void loadDetail(active.id)
                      } catch (e: unknown) {
                        toast.error(formatUserError(e))
                      }
                    }}>
                    {active.mode === 'enforce' ? 'Switch to audit' : armEnforce ? 'Confirm: enforce on attached instances' : 'Enforce'}
                  </button>
                  {armEnforce && (
                    <button type="button" className="px-2 py-1 rounded border border-[var(--apple-hairline)]"
                      onClick={() => { setArmEnforce(false); setEnforcePreview(null) }}>Cancel</button>
                  )}
                </div>
                {armEnforce && enforcePreview && (
                  <div className="mt-2 text-xs space-y-1" data-testid="sg-enforce-preview">
                    {enforcePreview.instances.length === 0 && (
                      <p className="text-[var(--text-muted)]">No instance has this group attached, so nothing changes yet.</p>
                    )}
                    {enforcePreview.instances.map((i) => (
                      <div key={i.vm}>
                        <span className="font-medium">{i.vm}</span>: {i.ingress_rules} inbound and {i.egress_rules} outbound rule(s) would apply
                        {i.warnings.map((w) => <p key={w} className="text-amber-500">Warning: {w}</p>)}
                      </div>
                    ))}
                  </div>
                )}
                <h3 className="text-sm font-medium text-[var(--text-muted)] mt-4 mb-2">
                  Rules ({rules.length})
                </h3>
                {selectedId && (
                  <div className="mb-4 flex flex-wrap gap-2 items-end text-xs">
                    <button type="button" className="px-2 py-1 rounded border border-[var(--apple-hairline)]"
                      onClick={async () => {
                        try {
                          await createSecurityGroupRule(selectedId, {
                            direction: 'ingress',
                            protocol: 'tcp',
                            port_min: 22,
                            port_max: 22,
                            remote_cidr: '0.0.0.0/0',
                          })
                          toast.success('SSH rule added')
                          void loadDetail(selectedId)
                        } catch (e: unknown) {
                          toast.error(formatUserError(e))
                        }
                      }}>+ SSH (22)</button>
                  </div>
                )}
                {rules.length === 0 ? (
                  <p className="text-sm text-[var(--text-muted)]">No rules defined.</p>
                ) : (
                  <TahoeTableWrap>
                    <table className="apple-table" aria-label="Security group rules">
                      <thead>
                        <tr>
                          <th scope="col">Direction</th>
                          <th scope="col">Protocol</th>
                          <th scope="col">Ports</th>
                          <th scope="col">Remote</th>
                          <th scope="col" />
                        </tr>
                      </thead>
                      <tbody className="font-mono text-xs">
                        {rules.map((r) => (
                          <tr key={r.id}>
                            <td className="text-[var(--text-secondary)]">{r.direction}</td>
                            <td>{r.protocol || '—'}</td>
                            <td>
                              {r.port_min != null
                                ? r.port_min === r.port_max
                                  ? String(r.port_min)
                                  : `${r.port_min}–${r.port_max}`
                                : '—'}
                            </td>
                            <td className="text-[var(--text-muted)]">
                              {r.remote_sg_id ? `sg:${r.remote_sg_id.slice(0, 8)}` : r.remote_cidr || '—'}
                            </td>
                            <td>
                              <button type="button" className={statusActionLinkClasses('error')}
                                onClick={async () => {
                                  try {
                                    await deleteSecurityGroupRule(r.id)
                                    toast.success('Rule deleted')
                                    void loadDetail(selectedId!)
                                  } catch (e: unknown) {
                                    toast.error(formatUserError(e))
                                  }
                                }}>Del</button>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </TahoeTableWrap>
                )}
              </>
            ) : (
              <p className="text-[var(--text-muted)] text-sm">Select a security group.</p>
            )}
          </div>
        </div>
      )}

      <FleetCloudFooter />

      <ConfirmDialog
        open={!!deleteGroupTarget}
        title="Delete security group"
        message={`Delete security group "${deleteGroupTarget?.name}" and all its rules?`}
        confirmLabel={deletingGroup ? 'Deleting…' : 'Delete'}
        variant="danger"
        onCancel={() => setDeleteGroupTarget(null)}
        onConfirm={async () => {
          if (!deleteGroupTarget) return
          setDeletingGroup(true)
          try {
            await deleteSecurityGroup(deleteGroupTarget.id)
            toast.success('Security group deleted')
            setDeleteGroupTarget(null)
            setSelectedId(null)
            setDetail(null)
            void load()
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          } finally {
            setDeletingGroup(false)
          }
        }}
      />
    </PageLayout>
  )
}

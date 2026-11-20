// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft, Loader2, Shield } from 'lucide-react'
import {
  createSecurityGroupRule,
  deleteSecurityGroup,
  deleteSecurityGroupRule,
  getSecurityGroup,
  listSecurityGroupRules,
  type NativeSecurityGroup,
  type NativeSecurityGroupRule,
} from '../api/securityGroups'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import ConfirmDialog from '../components/ConfirmDialog'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native security groups — no old external-cloud gate component in the way
// any more (the daemon's external-cloud-client integration has since been
// fully removed); see FleetCloudSecurityGroups.tsx.
export default function FleetCloudSecurityGroupDetailPage() {
  return <FleetCloudSecurityGroupDetailContent />
}

function FleetCloudSecurityGroupDetailContent() {
  const { id } = useParams<{ id: string }>()
  const toast = useToastContext()
  const [group, setGroup] = useState<NativeSecurityGroup | null>(null)
  const [rules, setRules] = useState<NativeSecurityGroupRule[]>([])
  const [loading, setLoading] = useState(true)
  const [confirmDelete, setConfirmDelete] = useState(false)
  useBreadcrumbName(group?.name)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior security group can't overwrite the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const [g, r] = await Promise.all([getSecurityGroup(id), listSecurityGroupRules(id)])
      if (!alive()) return
      setGroup(g)
      setRules(r)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setGroup(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void load() }, [load])

  if (loading) return <PageSkeleton />
  if (!group) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/security-groups" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/security-groups" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Security groups
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Shield className="w-7 h-7 text-[var(--accent)]" />
        {group.name}
      </h1>
      {group.description && <p className="text-sm text-[var(--text-muted)]">{group.description}</p>}
      <p className="text-xs text-amber-400/90">
        Advisory until switched to Enforce on the Security groups page; the badge there shows what the hosts report.
      </p>
      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">ID</dt><dd className="font-mono text-[var(--text-primary)] mt-1 break-all">{group.id}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Rules</dt><dd className="text-[var(--text-primary)] mt-1">{rules.length}</dd></div>
      </dl>
      <div className="flex flex-wrap gap-2">
        <button type="button" className="btn-secondary text-sm"
          onClick={async () => {
            try {
              await createSecurityGroupRule(group.id, {
                direction: 'ingress',
                protocol: 'tcp',
                port_min: 22,
                port_max: 22,
                remote_cidr: '0.0.0.0/0',
              })
              toast.success('Added SSH rule')
              void load()
            } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}>Add SSH ingress</button>
        <button type="button" className="px-3 py-1.5 rounded-lg border border-red-500/50 text-red-600 text-sm"
          onClick={() => setConfirmDelete(true)}>Delete group</button>
      </div>
      <ConfirmDialog
        open={confirmDelete}
        title="Delete security group"
        message={`Delete security group ${group.name}?`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          setConfirmDelete(false)
          try {
            await deleteSecurityGroup(group.id)
            toast.success('Deleted')
            window.location.href = '/fleet-cloud/security-groups'
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <h2 className="text-sm font-medium text-[var(--text-secondary)] mb-3">Rules</h2>
        {rules.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No rules.</p>
        ) : (
          <ul className="space-y-2 text-xs font-mono">
            {rules.map((r) => (
              <li key={r.id} className="flex flex-wrap items-center gap-2 rounded-lg border border-[var(--apple-hairline)] px-3 py-2 text-[var(--text-secondary)]">
                <span>{r.direction}</span>
                <span>{r.protocol || 'any'}</span>
                {(r.port_min != null || r.port_max != null) && (
                  <span>{r.port_min ?? '—'}–{r.port_max ?? '—'}</span>
                )}
                {r.remote_cidr && <span>{r.remote_cidr}</span>}
                <button type="button" className={statusActionLinkClasses('error', 'ml-auto')}
                  onClick={async () => {
                    try {
                      await deleteSecurityGroupRule(r.id)
                      toast.success('Rule deleted')
                      void load()
                    } catch (e: unknown) { toast.error(formatUserError(e)) }
                  }}>Delete</button>
              </li>
            ))}
          </ul>
        )}
      </section>
      <FleetCloudFooter />
    </PageLayout>
  )
}

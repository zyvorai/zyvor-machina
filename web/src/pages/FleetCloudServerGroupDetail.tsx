// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { ArrowLeft, Layers } from 'lucide-react'
import { deleteServerGroup, getServerGroup, removeVmFromServerGroup, type DerivedServerGroup } from '../api/nativeServerGroups'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import ConfirmDialog from '../components/ConfirmDialog'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native anti-affinity group — no old external-cloud gate component in the way
// any more (the daemon's external-cloud-client integration has since been
// fully removed). `id` in the route is the group name (derived from tags, not
// a stored UUID) — see api/nativeServerGroups.ts.
export default function FleetCloudServerGroupDetailPage() {
  return <FleetCloudServerGroupDetailContent />
}

function FleetCloudServerGroupDetailContent() {
  const { id: name } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const toast = useToastContext()
  const [group, setGroup] = useState<DerivedServerGroup | null>(null)
  const [loading, setLoading] = useState(true)
  const [confirmDelete, setConfirmDelete] = useState(false)
  useBreadcrumbName(group?.name)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!name) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior group can't overwrite the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const g = await getServerGroup(name)
      if (!alive()) return
      setGroup(g)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setGroup(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [name, toast])

  useEffect(() => { void load() }, [load])

  if (loading) return <PageSkeleton />
  if (!group) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/server-groups" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/server-groups" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Server groups
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Layers className="w-7 h-7 text-[var(--accent)]" />
        {group.name}
      </h1>
      <dl className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm space-y-3">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Policy</dt><dd className="text-[var(--text-primary)] mt-1">anti-affinity</dd></div>
        <div>
          <dt className="text-xs text-[var(--text-muted)] uppercase">Members ({group.members.length})</dt>
          <ul className="mt-1 font-mono text-xs text-[var(--text-muted)] space-y-1">
            {group.members.map((m) => (
              <li key={m.id} className="flex items-center gap-2">
                <Link to={`/fleet-cloud/instances/${m.id}`} className="text-[var(--accent)] hover:underline">{m.name}</Link>
                <button type="button" className={statusActionLinkClasses('error', 'text-xs ml-auto')}
                  onClick={async () => {
                    try {
                      await removeVmFromServerGroup(m, group.name)
                      toast.success('Removed')
                      void load()
                    } catch (e: unknown) { toast.error(formatUserError(e)) }
                  }}>Remove</button>
              </li>
            ))}
            {group.members.length === 0 && <li>None</li>}
          </ul>
        </div>
      </dl>
      <button type="button" className={`px-3 py-1.5 rounded-lg border text-sm ${statusActionLinkClasses('error')}`}
        onClick={() => setConfirmDelete(true)}>Delete group</button>
      <ConfirmDialog
        open={confirmDelete}
        title="Delete group"
        message={`Delete group ${group.name}?`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          setConfirmDelete(false)
          try {
            await deleteServerGroup(group)
            toast.success('Deleted')
            navigate('/fleet-cloud/server-groups')
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}

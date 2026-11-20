// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { usePlatformTabState } from '../hooks/usePlatformTabState'
import { Link, useNavigate, useParams } from 'react-router'
import { ArrowLeft, Layers, Trash2 } from 'lucide-react'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import { deleteStack, getStack, type NativeStack } from '../api/stacks'
import { StackDriftPanel, StackTemplateEditor } from '../components/flow/StackDrift'
import { DriftBadge } from '../components/flow/StackPlan'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native stacks have no Heat-style events log or live outputs. Templates are
// updated in place (instance groups and policies; v1 resources stay as they are).
const HEAT_TABS = ['overview', 'resources', 'drift', 'template'] as const
type Tab = (typeof HEAT_TABS)[number]

// Native stacks — there's no old external-cloud gate component in the way
// (the daemon's external-cloud-client integration has since been fully
// removed); see FleetCloudHeat.tsx.
export default function FleetCloudHeatDetailPage() {
  return <FleetCloudHeatDetailContent />
}

function FleetCloudHeatDetailContent() {
  const { id } = useParams<{ name: string; id: string }>()
  const toast = useToastContext()
  const navigate = useNavigate()
  const [stack, setStack] = useState<NativeStack | null>(null)
  const [tab, setTab] = usePlatformTabState(HEAT_TABS, { defaultTab: 'overview' })
  const [loading, setLoading] = useState(true)
  const [confirmDelete, setConfirmDelete] = useState(false)
  useBreadcrumbName(stack?.name)
  const loadStackSeq = useRef(0)
  const stackRef = useRef<NativeStack | null>(null)
  stackRef.current = stack

  const loadStack = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior stack can't overwrite the one now shown.
    const seq = ++loadStackSeq.current
    const alive = () => seq === loadStackSeq.current
    if (!stackRef.current) setLoading(true)
    try {
      const s = await getStack(id)
      if (!alive()) return
      setStack(s)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setStack(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void loadStack() }, [loadStack])

  useEffect(() => {
    if (stack?.status !== 'creating' && stack?.status !== 'updating') return
    const t = window.setTimeout(() => void loadStack(), 5000)
    return () => window.clearTimeout(t)
  }, [stack, loadStack])

  if (loading) return <PageSkeleton />
  if (!stack) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/heat" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  const tabs: { id: Tab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'resources', label: 'Resources' },
    { id: 'drift', label: 'Drift' },
    { id: 'template', label: 'Template' },
  ]

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/heat" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Stacks
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Layers className="w-7 h-7 text-[var(--accent)]" /> {stack.name}
        <DriftBadge stack={stack} />
      </h1>

      <div className="flex flex-wrap gap-2 border-b border-[var(--apple-hairline)] pb-2">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            className={`px-3 py-1.5 rounded-lg text-sm ${tab === t.id ? 'bg-[var(--accent)]/30 text-[var(--link)]' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]'}`}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === 'overview' && (
        <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
          <div><dt className="text-xs text-[var(--text-muted)] uppercase">ID</dt><dd className="font-mono mt-1 break-all">{stack.id}</dd></div>
          <div><dt className="text-xs text-[var(--text-muted)] uppercase">Status</dt><dd className="mt-1">{stack.status}</dd></div>
          <div className="sm:col-span-2"><dt className="text-xs text-[var(--text-muted)] uppercase">Last error</dt><dd className="mt-1 text-[var(--text-muted)]">{stack.last_error || '—'}</dd></div>
        </dl>
      )}

      {tab === 'resources' ? (
        <div className="overflow-x-auto apple-surface rounded-2xl">
          <table className="apple-table" aria-label="Stack resources">
            <thead>
              <tr>
                <th scope="col" className="px-3 py-2">Name</th>
                <th scope="col" className="px-3 py-2">Kind</th>
                <th scope="col" className="px-3 py-2">ID</th>
              </tr>
            </thead>
            <tbody>
              {stack.resources_json.map((r) => (
                <tr key={`${r.kind}:${r.name}`} className="border-t border-[var(--apple-hairline)]">
                  <td className="px-3 py-2">{r.name}</td>
                  <td className="px-3 py-2 text-[var(--text-muted)]">{r.kind}</td>
                  <td className="px-3 py-2 font-mono text-xs">{r.id}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {stack.resources_json.length === 0 && <p className="p-4 text-[var(--text-muted)] text-sm">No resources created yet.</p>}
        </div>
      ) : tab === 'drift' ? (
        <StackDriftPanel key={stack.checked_at ?? ''} stack={stack} onChanged={() => void loadStack()} />
      ) : tab === 'template' ? (
        <StackTemplateEditor key={stack.updated_at ?? ''} stack={stack} onChanged={() => void loadStack()} />
      ) : null}

      <button type="button" className="px-3 py-1.5 rounded-lg border border-red-600/50 text-red-600 text-sm inline-flex items-center gap-1"
        onClick={() => setConfirmDelete(true)}>
        <Trash2 className="w-4 h-4" /> Delete stack
      </button>
      <ConfirmDialog
        open={confirmDelete}
        title="Delete stack"
        message={`Delete stack ${stack.name}?`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          setConfirmDelete(false)
          try {
            await deleteStack(stack.id)
            toast.success('Deleted')
            navigate('/fleet-cloud/heat')
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}

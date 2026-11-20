// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft, KeyRound } from 'lucide-react'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import {
  addProjectMember,
  listProjectMembers,
  listProjectRegistry,
  listUsers,
  removeProjectMember,
  type NativeProject,
  type NativeProjectMember,
  type NativeUser,
} from '../api/nativeProjects'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses, statusToneClass } from '../utils/semanticColors'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

const ROLES = ['admin', 'operator', 'viewer'] as const

// Native project registry — there's no old external-cloud gate component to gate
// it behind (the daemon's external-cloud-client integration has since been
// fully removed); see FleetCloudIdentity.tsx.
export default function FleetCloudIdentityProjectDetailPage() {
  return <FleetCloudIdentityProjectDetailContent />
}

function FleetCloudIdentityProjectDetailContent() {
  const { id } = useParams<{ id: string }>()
  const toast = useToastContext()
  const [project, setProject] = useState<NativeProject | null>(null)
  const [members, setMembers] = useState<NativeProjectMember[]>([])
  const [users, setUsers] = useState<NativeUser[]>([])
  const [grantUserId, setGrantUserId] = useState('')
  const [grantRole, setGrantRole] = useState<string>(ROLES[1])
  const [loading, setLoading] = useState(true)
  useBreadcrumbName(project?.name)

  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior project can't interleave into the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const [all, m, u] = await Promise.all([
        listProjectRegistry(),
        listProjectMembers(id),
        listUsers().catch(() => []),
      ])
      if (!alive()) return
      setProject(all.find((p) => p.id === id) ?? null)
      setMembers(m)
      setUsers(u)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setProject(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void load() }, [load])

  if (loading) return <PageSkeleton />
  if (!project) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/identity" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/identity" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Projects
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <KeyRound className={`w-7 h-7 ${statusToneClass('warn')}`} /> {project.name}
      </h1>
      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">ID</dt><dd className="font-mono mt-1 break-all">{project.id}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Enabled</dt><dd className="mt-1">{project.enabled ? 'yes' : 'no'}</dd></div>
        <div className="sm:col-span-2"><dt className="text-xs text-[var(--text-muted)] uppercase">Description</dt><dd className="mt-1">{project.description || '—'}</dd></div>
      </dl>

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Members</h2>
        <div className="flex flex-wrap gap-2">
          <select aria-label="User" value={grantUserId} onChange={(e) => setGrantUserId(e.target.value)}
            className="input-field text-sm">
            <option value="">User…</option>
            {users.map((u) => <option key={u.id} value={u.id}>{u.username}</option>)}
          </select>
          <select aria-label="Role" value={grantRole} onChange={(e) => setGrantRole(e.target.value)}
            className="input-field text-sm">
            {ROLES.map((r) => <option key={r} value={r}>{r}</option>)}
          </select>
          <button type="button" className="px-2 py-1 rounded bg-amber-700 text-white text-sm"
            disabled={!grantUserId}
            onClick={async () => {
              try {
                await addProjectMember(project.id, { user_id: grantUserId, role: grantRole })
                toast.success('Member added')
                void load()
              } catch (e: unknown) { toast.error(formatUserError(e)) }
            }}>Add</button>
        </div>
        <div className="overflow-x-auto">
          <table className="apple-table" aria-label="Project members">
            <thead className="text-[var(--text-muted)] text-left">
              <tr><th scope="col" className="py-1">User</th><th scope="col" className="py-1">Role</th><th scope="col" /></tr>
            </thead>
            <tbody>
              {members.map((m) => (
                <tr key={m.user_id} className="border-t border-[var(--apple-hairline)]">
                  <td className="py-2">{m.username}</td>
                  <td className="py-2">{m.role}</td>
                  <td className="py-2 text-right">
                    <button type="button" className={statusActionLinkClasses('error', 'text-xs')} onClick={async () => {
                      try {
                        await removeProjectMember(project.id, m.user_id)
                        toast.success('Removed')
                        void load()
                      } catch (e: unknown) { toast.error(formatUserError(e)) }
                    }}>Remove</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {members.length === 0 && <p className="text-[var(--text-muted)] text-sm py-2">No members.</p>}
        </div>
      </section>
      <FleetCloudFooter />
    </PageLayout>
  )
}

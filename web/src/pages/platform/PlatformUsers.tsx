// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Link, useNavigate } from 'react-router'
import { Boxes, Plus, Users } from 'lucide-react'
import {
  MacGlassPanel,
} from '../../components/platform/mac/PlatformMacUi'
import DetailTabs from '../../components/platform/DetailTabs'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformPageChrome, { PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { TahoeListEmpty, TahoeTableWrap, TahoeToolbar } from '../../components/platform/tahoe/TahoeListKit'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import {
  createUser,
  deleteUser,
  getCurrentUser,
  getFleetUsers,
  listUsers,
  pruneInvalidUsers,
  patchUser,
  type FleetUsersOverview,
  type PlatformUser,
} from '../../api/platform'
import { useActiveWorkspace } from '../../hooks/useActiveWorkspace'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

type TabId = 'users' | 'workspaces'

const USER_TABS = [
  { id: 'users' as const, label: 'Users' },
  { id: 'workspaces' as const, label: 'Groups' },
]

export default function PlatformUsers({ embedded }: { embedded?: boolean } = {}) {
  const toast = useToastContext()
  const navigate = useNavigate()
  const [tab, setTab] = usePlatformTabState<TabId>(USER_TABS.map((t) => t.id), { defaultTab: 'users' })
  const { workspace, setWorkspace } = useActiveWorkspace()

  const [fleet, setFleet] = useState<FleetUsersOverview | null>(null)
  const [rows, setRows] = useState<PlatformUser[]>([])
  const [me, setMe] = useState<PlatformUser | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [formError, setFormError] = useState<string | null>(null)
  const [deleteUserId, setDeleteUserId] = useState<string | null>(null)
  const [newUsername, setNewUsername] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [role, setRole] = useState('operator')
  const [addBusy, setAddBusy] = useState(false)
  const [search, setSearch] = useState('')

  const filteredRows = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return rows
    return rows.filter((u) => u.username.toLowerCase().includes(q) || u.role.toLowerCase().includes(q))
  }, [rows, search])

  const load = useCallback(async () => {
    setError(null)
    try {
      await pruneInvalidUsers().catch(() => null)
      const [f, users, current] = await Promise.all([getFleetUsers(), listUsers(), getCurrentUser().catch(() => null)])
      setFleet(f)
      setRows(users.filter((u) => u.username?.trim()))
      setMe(current)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  const validateNewUser = () => {
    const username = newUsername.trim()
    if (!username) return 'Username is required.'
    if (!/^[a-zA-Z0-9._-]+$/.test(username)) {
      return 'Username may only contain letters, numbers, dot, dash, and underscore.'
    }
    if (newPassword.length < 8) return 'Password must be at least 8 characters.'
    return null
  }

  const handleAddUser = async () => {
    const validation = validateNewUser()
    if (validation) {
      setFormError(validation)
      return
    }
    setFormError(null)
    setAddBusy(true)
    try {
      await createUser({ username: newUsername.trim(), password: newPassword, role })
      toast.success(`User ${newUsername.trim()} created`)
      setNewUsername('')
      setNewPassword('')
      await load()
    } catch (e: unknown) {
      const message = formatUserError(e)
      setFormError(message)
      toast.error(message)
    } finally {
      setAddBusy(false)
    }
  }

  useEffect(() => { void load() }, [load])

  const switchWorkspace = (name: string) => {
    setWorkspace(name)
    navigate(name ? `/platform/vms?project=${encodeURIComponent(name)}` : '/platform/vms')
  }

  const scrollToAddUser = () => {
    document.getElementById('users-add-form')?.scrollIntoView({ behavior: 'smooth' })
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      loading={loading && !fleet}
      error={error}
      onErrorRetry={() => void load()}
      title={embedded ? undefined : 'Access & Workspaces'}
      subtitle={embedded ? undefined : (
        fleet
          ? (
            <span className="flex flex-col gap-1">
              <span className="text-[var(--text-muted)]">Platform RBAC accounts and tenant workspaces — switch active workspace from the menu bar.</span>
              {platformStatSubtitle([
                { label: 'Users', value: fleet.user_count },
                { label: 'Admins', value: fleet.admin_count },
                { label: 'Workspaces', value: fleet.workspace_count },
                { label: 'Quotas enforced', value: fleet.workspaces_enforced },
              ])}
            </span>
          )
          : 'Platform RBAC accounts and tenant workspaces — switch active workspace from the menu bar.'
      )}
      icon={embedded ? undefined : <Users className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : (
        <>
          <button
            type="button"
            className="btn-primary text-sm flex items-center gap-2"
            onClick={scrollToAddUser}
          >
            <Plus className="w-4 h-4" /> Add user
          </button>
          <PlatformRefreshButton onClick={() => void load()} />
        </>
      )}
      contentClassName="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4"
    >
      <OperatingSurfaceLayout testId="platform-users-page">
        {me && <p className="text-sm text-[var(--text-muted)]">Signed in as <strong className="text-[var(--text-primary)]">{me.username}</strong> ({me.role})</p>}
        {fleet && <p className="text-sm text-[var(--text-muted)]">{fleet.summary}</p>}

        <DetailTabs primary={USER_TABS} active={tab} onChange={setTab} />

        {tab === 'users' && (
          <>
            <MacGlassPanel
              title="Add platform user"
              subtitle="Controller database accounts (separate from OS/PAM users)."
            >
              <form id="users-add-form" className="grid gap-3 md:grid-cols-4 md:items-end" onSubmit={(e) => { e.preventDefault(); void handleAddUser() }}>
                <label className="block space-y-1">
                  <span className="text-xs text-[var(--text-muted)]">Username</span>
                  <input
                    className="input w-full"
                    placeholder="jane.ops"
                    value={newUsername}
                    autoComplete="off"
                    onChange={(e) => { setNewUsername(e.target.value); setFormError(null) }}
                  />
                </label>
                <label className="block space-y-1">
                  <span className="text-xs text-[var(--text-muted)]">Password</span>
                  <input
                    className="input w-full"
                    type="password"
                    placeholder="min. 8 characters"
                    value={newPassword}
                    autoComplete="new-password"
                    onChange={(e) => { setNewPassword(e.target.value); setFormError(null) }}
                  />
                </label>
                <label className="block space-y-1">
                  <span className="text-xs text-[var(--text-muted)]">Role</span>
                  <select className="input w-full" value={role} onChange={(e) => setRole(e.target.value)}>
                    <option value="admin">admin</option>
                    <option value="operator">operator</option>
                    <option value="viewer">viewer</option>
                  </select>
                </label>
                <button
                  type="submit"
                  className="btn-primary text-sm w-fit flex items-center gap-2"
                  disabled={addBusy}
                >
                  <Plus className="w-4 h-4" /> {addBusy ? 'Adding…' : 'Add user'}
                </button>
              </form>
              {formError && (
                <p className="text-sm text-red-600 mt-3" role="alert">{formError}</p>
              )}
            </MacGlassPanel>

            <TahoeToolbar search={search} onSearchChange={setSearch} placeholder="Search users…" />

            {filteredRows.length === 0 ? (
              <TahoeListEmpty
                icon={Users}
                title={search ? 'No users match' : 'No platform users yet'}
                description={search ? 'Try a different search term.' : 'Create the first RBAC account to grant fleet access.'}
                primaryAction={search ? undefined : { label: 'Add user', onClick: scrollToAddUser }}
              />
            ) : (
              <TahoeTableWrap>
                <table className="apple-table w-full text-sm" aria-label="Platform users">
                  <thead>
                    <tr>
                      <th scope="col">User</th>
                      <th scope="col">Role</th>
                      <th scope="col" className="text-right">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {filteredRows.map((u) => (
                      <tr key={u.id}>
                        <td className="font-medium text-[var(--text-primary)]">{u.username}</td>
                        <td>
                          <select
                            aria-label="User role"
                            className="input text-xs capitalize w-full max-w-[160px]"
                            value={u.role}
                            onChange={async (e) => {
                              try {
                                await patchUser(u.id, { role: e.target.value })
                                toast.success('Role updated')
                                await load()
                              } catch (err: unknown) { toast.error(formatUserError(err)) }
                            }}
                          >
                            <option value="admin">admin</option>
                            <option value="operator">operator</option>
                            <option value="viewer">viewer</option>
                          </select>
                        </td>
                        <td className="text-right">
                          <button
                            type="button"
                            className="btn-secondary text-xs"
                            disabled={me?.username === u.username}
                            title={me?.username === u.username ? 'Cannot delete your own account' : undefined}
                            onClick={() => setDeleteUserId(u.id)}
                          >
                            Delete
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </TahoeTableWrap>
            )}
          </>
        )}

        {tab === 'workspaces' && (
          <>
            <TahoeToolbar search={search} onSearchChange={setSearch} placeholder="Search workspaces…" />
            {!fleet ? (
              <p className="text-sm text-[var(--text-muted)] py-6 text-center">Loading workspaces…</p>
            ) : fleet.workspaces.length === 0 ? (
              <TahoeListEmpty
                icon={Boxes}
                title="No workspaces yet"
                description="Assign VMs to a project label in Machine Finder to create tenant groups."
                primaryAction={{ label: 'Open Machine Finder', onClick: () => navigate('/platform/vms') }}
              />
            ) : (
              <TahoeTableWrap>
                <table className="apple-table w-full text-sm" aria-label="Workspace groups">
                  <thead>
                    <tr>
                      <th scope="col">Workspace</th>
                      <th scope="col">VMs</th>
                      <th scope="col">Isolation</th>
                      <th scope="col">Quota</th>
                      <th scope="col" className="text-right">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {fleet.workspaces
                      .filter((w) => {
                        const q = search.trim().toLowerCase()
                        return !q || w.name.toLowerCase().includes(q)
                      })
                      .map((w) => (
                        <tr
                          key={w.name}
                          className="cursor-pointer hover:bg-[var(--surface-hover)]"
                          onClick={() => switchWorkspace(w.name)}
                        >
                          <td className="font-medium text-[var(--text-primary)]">
                            {w.name}
                            {workspace === w.name && (
                              <span className="ml-2 text-xs text-[var(--link)] border border-[var(--apple-hairline)] px-2 py-0.5 rounded">active</span>
                            )}
                          </td>
                          <td>{w.vm_count}</td>
                          <td className="text-xs uppercase text-[var(--text-muted)]">{w.network_isolation}</td>
                          <td className="text-xs">
                            {w.enforce_quotas ? (
                              <span className={statusToneClass('ok')}>{w.quota_status}</span>
                            ) : (
                              <span className="text-[var(--text-muted)]">{w.quota_status}</span>
                            )}
                          </td>
                          <td className="text-right">
                            <Link
                              to={`/platform/vms?project=${encodeURIComponent(w.name)}`}
                              className={`text-xs ${hubLinkClasses()}`}
                              onClick={(e) => e.stopPropagation()}
                            >
                              VMs →
                            </Link>
                          </td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </TahoeTableWrap>
            )}
            <div className="flex flex-wrap gap-3 pt-2 border-t border-white/[0.04]">
              <Link to="/platform/projects" className={`text-sm ${hubLinkClasses()}`}>Projects list</Link>
              <Link to="/platform/enterprise?tab=tenants" className={`text-sm ${hubLinkClasses()}`}>Tenant isolation</Link>
            </div>
          </>
        )}
      </OperatingSurfaceLayout>
      <ConfirmDialog
        open={deleteUserId !== null}
        title="Delete User"
        message={`Delete user "${rows.find((u) => u.id === deleteUserId)?.username}"? This cannot be undone.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setDeleteUserId(null)}
        onConfirm={async () => {
          try { await deleteUser(deleteUserId!); toast.success('User deleted'); await load() }
          catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setDeleteUserId(null) }
        }}
      />
    </PlatformPageChrome>
  )
}

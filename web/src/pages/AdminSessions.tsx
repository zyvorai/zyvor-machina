// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { RefreshCw, Trash2, Users } from 'lucide-react'
import ConfirmDialog from '../components/ConfirmDialog'
import { useAuth } from '../contexts/AuthContext'
import { useToastContext } from '../contexts/ToastContext'
import { listAdminSessions, revokeAdminSession, AdminSessionsResponse } from '../api/adminSessions'
import { logout as apiLogout } from '../api/auth'
import { formatUserError } from '../utils/apiError'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { statusActionLinkClasses, statusBadgeClasses, statusToneClass } from '../utils/semanticColors'
import { useExpandable } from '../hooks/useExpandable'
import { ExpandableToggle } from '../components/ui/ExpandableToggle'

export default function AdminSessionsPage() {
  const { username, isRoot } = useAuth()
  const toast = useToastContext()
  const [data, setData] = useState<AdminSessionsResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [revokeTarget, setRevokeTarget] = useState<{ sessionId: string; username: string; isCurrent: boolean } | null>(null)
  const sessionList = useExpandable(data?.sessions ?? [], 50)

  const load = useCallback(async () => {
    if (!isRoot) return
    setLoading(true)
    try {
      setData(await listAdminSessions())
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setData(null)
    } finally {
      setLoading(false)
    }
  }, [isRoot, toast])

  useEffect(() => { void load() }, [load])

  if (!isRoot) {
    return (
      <div className="max-w-lg mx-auto text-center space-y-4 py-16">
        <h1 className="text-xl font-semibold text-[var(--text-primary)]">Web sessions</h1>
        <p className="text-[var(--text-muted)] text-sm">
          Only the UNIX <strong className="text-[var(--text-secondary)]">root</strong> user may list or revoke browser sessions.
          You are signed in as <code className="bg-[var(--apple-fill-tertiary)] px-1 rounded text-[var(--text-secondary)]">{username || '?'}</code>.
        </p>
        <Link to="/" className={`inline-block min-h-9 text-sm ${statusActionLinkClasses('info')}`}>Back to dashboard</Link>
      </div>
    )
  }

  return (
    <PageLayout
      eyebrow="Security"
      className="max-w-5xl"
      title="Web sessions"
      icon={<Users className={`w-7 h-7 ${statusToneClass('info')}`} />}
      subtitle="In-memory browser logins for this machina daemon (not API bearer tokens). Revoking a session invalidates that cookie; the user must sign in again."
      actions={
        <button type="button" onClick={() => void load()} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" aria-label="Refresh" title="Refresh">
          <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
        </button>
      }
      contentLoading={loading && !data}
    >
      {data && (
        <div className="grid gap-4 sm:grid-cols-3">
          <div className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-xl p-4">
            <div className="text-[10px] font-bold text-[var(--text-muted)] uppercase tracking-wider">Sessions</div>
            <div className="text-2xl font-semibold text-[var(--text-primary)] mt-1">{data.total_sessions}</div>
          </div>
          <div className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-xl p-4">
            <div className="text-[10px] font-bold text-[var(--text-muted)] uppercase tracking-wider">Distinct users</div>
            <div className="text-2xl font-semibold text-[var(--text-primary)] mt-1">{data.users_logged_in}</div>
          </div>
          <div className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-xl p-4 sm:col-span-1">
            <div className="text-[10px] font-bold text-[var(--text-muted)] uppercase tracking-wider">Per user</div>
            <div className="text-xs text-[var(--text-secondary)] mt-2 font-mono space-y-0.5">
              {Object.entries(data.sessions_per_username).map(([u, n]) => (
                <div key={u}>{u}: {n}</div>
              ))}
            </div>
          </div>
        </div>
      )}

      {!loading && data && data.sessions.length === 0 ? (
        <EmptyState title="No active sessions" description="No browser sessions are currently tracked by the daemon." />
      ) : (
      <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-x-auto">
        <table className="w-full text-sm" aria-label="Active sessions">
          <thead>
            <tr className="border-b border-[var(--apple-hairline)] text-left text-[var(--text-muted)]">
              <th scope="col" className="px-4 py-3">Session id</th>
              <th scope="col" className="px-4 py-3">User</th>
              <th scope="col" className="px-4 py-3">Age</th>
              <th scope="col" className="px-4 py-3">TTL left</th>
              <th scope="col" className="px-4 py-3 text-right">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--apple-hairline)]/30" id={sessionList.listId}>
            {sessionList.shown.map((s) => (
              <tr key={s.session_id} className="table-row-hover">
                <td className="px-4 py-3 font-mono text-xs text-[var(--text-muted)] break-all max-w-[200px]">{s.session_id}</td>
                <td className="px-4 py-3 font-medium">
                  {s.username}
                  {s.is_current && <span className={`ml-2 text-[10px] uppercase ${statusBadgeClasses('info')}`}>This browser</span>}
                </td>
                <td className="px-4 py-3 text-[var(--text-muted)]">{formatDuration(s.age_secs)}</td>
                <td className="px-4 py-3 text-[var(--text-muted)]">{formatDuration(s.expires_in_secs)}</td>
                <td className="px-4 py-3 text-right">
                  <button
                    type="button"
                    className={`p-1.5 rounded-lg hover:bg-red-600/20 disabled:opacity-40 disabled:pointer-events-none ${statusToneClass('error')}`}
                    title={s.is_current ? 'Ends this browser session (you will need to sign in again)' : 'Revoke session'}
                    aria-label="Revoke session"
                    onClick={async () => {
                      setRevokeTarget({ sessionId: s.session_id, username: s.username, isCurrent: Boolean(s.is_current) })
                    }}
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {sessionList.showToggle && (
          <div className="p-3 border-t border-[var(--apple-hairline)]">
            <ExpandableToggle expanded={sessionList.expanded} hidden={sessionList.hidden} listId={sessionList.listId} onToggle={sessionList.toggle} noun="sessions" />
          </div>
        )}
      </div>
      )}
      <ConfirmDialog
        open={revokeTarget !== null}
        title={revokeTarget?.isCurrent ? 'End this session' : 'Revoke session'}
        message={
          revokeTarget?.isCurrent
            ? 'End your own browser session? You will be signed out immediately.'
            : `Revoke the session for "${revokeTarget?.username}"? They will be signed out immediately.`
        }
        confirmLabel="Revoke"
        variant="danger"
        onCancel={() => setRevokeTarget(null)}
        onConfirm={async () => {
          const t = revokeTarget
          setRevokeTarget(null)
          if (!t) return
          try {
            await revokeAdminSession(t.sessionId)
            if (t.isCurrent) {
              await apiLogout()
              window.location.href = '/login'
              return
            }
            toast.success(`Session for "${t.username}" revoked`)
            void load()
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          }
        }}
      />
    </PageLayout>
  )
}

function formatDuration(secs: number): string {
  // An already-expired session has negative expires_in_secs — show "expired"
  // rather than "-42s". (age_secs is never negative, so this only affects the
  // remaining-time column.)
  if (secs < 0) return 'expired'
  if (secs < 60) return `${secs}s`
  if (secs < 3600) return `${Math.floor(secs / 60)}m`
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`
}

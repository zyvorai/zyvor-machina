// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Copy, Key, Plus, RotateCcw, Trash2 } from 'lucide-react'
import GlassDataTable from '../../components/platform/GlassDataTable'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformPageChrome, { PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import { createApiKey, deleteApiKey, listApiKeys, type ApiKeyRow } from '../../api/platform'
import { rotateApiKey } from '../../api/day2'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'

export default function PlatformApiKeys({ embedded }: { embedded?: boolean } = {}) {
  const toast = useToastContext()
  const [rows, setRows] = useState<ApiKeyRow[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [role, setRole] = useState('operator')
  const [projectScope, setProjectScope] = useState('')
  const [newToken, setNewToken] = useState<string | null>(null)
  const [deleteKeyId, setDeleteKeyId] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [rotateKeyId, setRotateKeyId] = useState<string | null>(null)
  const [rotatingId, setRotatingId] = useState<string | null>(null)

  const load = useCallback(async () => {
    setError(null)
    try { setRows(await listApiKeys()) } catch (e: unknown) { setError(formatUserError(e)) }
    finally { setLoading(false) }
  }, [])

  useEffect(() => { void load() }, [load])

  const activeCount = rows.length

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      loading={loading && rows.length === 0}
      error={error}
      onErrorRetry={() => void load()}
      title={embedded ? undefined : 'API keys'}
      subtitle={embedded ? undefined : (
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">Bearer tokens for automation (machina_*)</span>
          {platformStatSubtitle([
            { label: 'Active keys', value: activeCount },
            { label: 'Total keys', value: rows.length },
          ])}
        </span>
      )}
      icon={embedded ? undefined : <Key className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <OperatingSurfaceLayout testId="platform-api-keys-page">
        {newToken && (
          <div className={`rounded-xl p-4 text-sm ${statusSurfaceClasses('warn')}`}>
            <p className={`mb-2 ${statusToneClass('warn')}`}>Copy this token now — it will not be shown again:</p>
            <div className="flex items-start gap-2">
              <code className="flex-1 break-all text-xs text-[var(--text-secondary)]">{newToken}</code>
              <button
                type="button"
                className="btn-secondary text-xs shrink-0 inline-flex items-center gap-1"
                onClick={async () => {
                  try { await navigator.clipboard.writeText(newToken); toast.success('Token copied') }
                  catch { toast.error('Copy failed — select the token and copy it manually') }
                }}
                aria-label="Copy API token"
              >
                <Copy className="w-3.5 h-3.5" /> Copy
              </button>
            </div>
          </div>
        )}

        <MacGlassPanel title="Create API key" subtitle="Keys inherit RBAC role and appear as machina_* bearer tokens.">
          <form className="grid gap-3 md:grid-cols-4 md:items-end" onSubmit={async (e) => {
            e.preventDefault()
            if (creating || !name.trim()) return
            setCreating(true)
            try {
              const scope = projectScope.split(',').map((p) => p.trim()).filter(Boolean)
              const res = await createApiKey({ name: name.trim(), role, projects: scope.length ? scope : undefined })
              setNewToken(res.token)
              setName('')
              toast.success('API key created')
              await load()
            } catch (err: unknown) { toast.error(formatUserError(err)) }
            finally { setCreating(false) }
          }}>
            <label className="block space-y-1">
              <span className="text-xs text-[var(--text-muted)]">Name</span>
              <input className="input w-full" placeholder="automation" value={name} onChange={(e) => setName(e.target.value)} />
            </label>
            <label className="block space-y-1">
              <span className="text-xs text-[var(--text-muted)]">Role</span>
              <select className="input w-full" value={role} onChange={(e) => setRole(e.target.value)}>
                <option value="admin">admin</option>
                <option value="operator">operator</option>
                <option value="viewer">viewer</option>
              </select>
            </label>
            <label className="block space-y-1 md:col-span-2">
              <span className="text-xs text-[var(--text-muted)]">Limit to projects (optional, comma separated; not for admin keys)</span>
              <input className="input w-full" aria-label="Limit to projects" placeholder="all projects" value={projectScope} onChange={(e) => setProjectScope(e.target.value)} />
            </label>
            <button type="submit" disabled={creating || !name.trim()} className="btn-primary text-sm w-fit flex items-center gap-2 md:col-span-2 disabled:opacity-40 disabled:cursor-not-allowed"><Plus className="w-4 h-4" /> {creating ? 'Creating…' : 'Create key'}</button>
          </form>
        </MacGlassPanel>

        <GlassDataTable
          title="API keys"
          columns={
            <>
              <th scope="col" className="p-3 text-left">Name</th>
              <th scope="col" className="p-3 text-left">Role</th>
              <th scope="col" className="p-3 text-left">Projects</th>
              <th scope="col" className="p-3 text-left">Last used</th>
              <th scope="col" className="p-3 text-right" />
            </>
          }
          isEmpty={rows.length === 0}
          empty={{
            icon: Key,
            title: 'No API keys yet',
            subtitle: 'Create a bearer token for CI/CD or automation scripts.',
          }}
        >
          {rows.map((k) => (
            <tr key={k.id} className="border-b border-white/[0.04]">
              <td className="p-3 text-[var(--text-primary)]">{k.name}</td>
              <td className="p-3 capitalize">{k.role}</td>
              <td className="p-3 text-[var(--text-muted)]">{k.projects && k.projects.length ? k.projects.join(', ') : 'all'}</td>
              <td className="p-3 text-[var(--text-muted)]">{k.last_used_at ? new Date(k.last_used_at).toLocaleString() : '—'}</td>
              <td className="p-3 text-right">
                <div className="inline-flex items-center gap-2">
                  <button
                    type="button"
                    className="btn-secondary text-xs inline-flex items-center gap-1"
                    aria-label="Rotate API key"
                    disabled={rotatingId === k.id}
                    onClick={() => setRotateKeyId(k.id)}
                  >
                    <RotateCcw className="w-3 h-3" /> {rotatingId === k.id ? 'Rotating…' : 'Rotate'}
                  </button>
                  <button type="button" className="btn-secondary text-xs" aria-label="Delete API key" onClick={() => setDeleteKeyId(k.id)}><Trash2 className="w-3 h-3 inline" /></button>
                </div>
              </td>
            </tr>
          ))}
        </GlassDataTable>
      </OperatingSurfaceLayout>
      <ConfirmDialog
        open={rotateKeyId !== null}
        title="Rotate API Key"
        message={`Rotate API key "${rows.find((k) => k.id === rotateKeyId)?.name}"? A new token is issued and the current token stops working immediately. Update any integrations using it.`}
        confirmLabel="Rotate"
        variant="warning"
        onCancel={() => setRotateKeyId(null)}
        onConfirm={async () => {
          const id = rotateKeyId
          setRotateKeyId(null)
          if (!id) return
          setRotatingId(id)
          try {
            const res = await rotateApiKey(id)
            setNewToken(res.token)
            toast.success('API key rotated — copy the new token now')
            await load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setRotatingId(null) }
        }}
      />
      <ConfirmDialog
        open={deleteKeyId !== null}
        title="Delete API Key"
        message={`Delete API key "${rows.find((k) => k.id === deleteKeyId)?.name}"? Any integrations using this key will stop working immediately.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setDeleteKeyId(null)}
        onConfirm={async () => {
          try { await deleteApiKey(deleteKeyId!); toast.success('Deleted'); await load() }
          catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setDeleteKeyId(null) }
        }}
      />
    </PlatformPageChrome>
  )
}

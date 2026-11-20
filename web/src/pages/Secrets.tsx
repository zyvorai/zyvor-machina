// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback, useRef } from 'react'
import { listSecrets, deleteSecret, getSecretXml, defineSecret, SecretInfo } from '../api/advanced'
import { useFocusTrap } from '../hooks/useFocusTrap'
import { useToastContext } from '../contexts/ToastContext'
import ConfirmDialog from '../components/ConfirmDialog'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { Shield, Trash2, RefreshCw, Search, Code, X, Plus } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import { statusToneClass } from '../utils/semanticColors'

export default function SecretsPage() {
  const [secrets, setSecrets] = useState<SecretInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [search, setSearch] = useState('')
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [xmlContent, setXmlContent] = useState<string | null>(null)
  const [xmlUuid, setXmlUuid] = useState('')
  const [defineOpen, setDefineOpen] = useState(false)
  const [defXml, setDefXml] = useState('')
  const [defValueB64, setDefValueB64] = useState('')
  const [defValidate, setDefValidate] = useState(false)
  const [defSaving, setDefSaving] = useState(false)
  const defineRef = useRef<HTMLDivElement>(null)
  const xmlRef = useRef<HTMLDivElement>(null)
  useFocusTrap(defineRef, defineOpen, () => { if (!defSaving) setDefineOpen(false) })
  useFocusTrap(xmlRef, xmlContent !== null, () => setXmlContent(null))
  const toast = useToastContext()

  const load = useCallback(async () => {
    try {
      setLoading(true)
      setSecrets(await listSecrets())
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { load() }, [load])

  const handleDelete = async () => {
    if (!deleteTarget) return
    try { await deleteSecret(deleteTarget); toast.success(`Deleted secret '${deleteTarget}'`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setDeleteTarget(null)
  }

  const showXml = async (uuid: string) => {
    try { const xml = await getSecretXml(uuid); setXmlContent(xml); setXmlUuid(uuid) } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const handleDefineSecret = async () => {
    if (!defXml.trim()) {
      toast.warning('Secret XML is required')
      return
    }
    setDefSaving(true)
    try {
      const r = await defineSecret({
        xml: defXml.trim(),
        value_base64: defValueB64.trim() || undefined,
        validate_xml: defValidate,
        set_value_flags: 0,
      })
      toast.success(`Secret defined: ${r.uuid}`)
      setDefineOpen(false)
      setDefXml('')
      setDefValueB64('')
      setDefValidate(false)
      load()
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setDefSaving(false)
    }
  }

  const filtered = secrets.filter(s =>
    search === '' ||
    s.uuid.toLowerCase().includes(search.toLowerCase()) ||
    s.usage_type.toLowerCase().includes(search.toLowerCase()) ||
    s.usage_id.toLowerCase().includes(search.toLowerCase())
  )

  return (
    <PageLayout
      eyebrow="Hypervisor"
      title="Secrets"
      icon={<Shield className="w-6 h-6" />}
      subtitle={`${secrets.length} libvirt secrets`}
      actions={
        <>
          <button type="button" onClick={() => setDefineOpen(true)} className="px-3 py-1.5 btn-primary rounded-lg text-sm font-medium transition flex items-center gap-1"><Plus className="w-4 h-4" /> Define secret</button>
          <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" title="Refresh" aria-label="Refresh"><RefreshCw className="w-4 h-4" /></button>
        </>
      }
    >
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
        <input type="text" aria-label="Search secrets" placeholder="Search secrets..." value={search} onChange={(e) => setSearch(e.target.value)}
          className={`w-full pl-10 py-2 bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)] ${search ? 'pr-8' : 'pr-4'}`} />
        {search && (
          <button type="button" aria-label="Clear search" onClick={() => setSearch('')}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
            <X className="w-4 h-4" />
          </button>
        )}
      </div>

      {loading ? (
        <div className="flex items-center justify-center h-32" aria-busy="true" aria-label="Loading secrets">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-[var(--accent)]" />
        </div>
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={<Shield className="w-6 h-6" />}
          title="No secrets found"
          description={search ? 'Try a different search term.' : 'Define a libvirt secret for encrypted storage pools (Ceph, iSCSI, etc.).'}
          primaryAction={
            !search ? (
              <button type="button" onClick={() => setDefineOpen(true)} className="btn-primary text-sm">
                Define secret
              </button>
            ) : undefined
          }
        />
      ) : (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
          <table className="w-full" aria-label="libvirt secrets">
            <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]">
              <th scope="col" className="px-6 py-3">UUID</th><th scope="col" className="px-6 py-3 hidden md:table-cell">Usage Type</th><th scope="col" className="px-6 py-3 hidden md:table-cell">Usage ID</th><th scope="col" className="px-6 py-3 text-right">Actions</th>
            </tr></thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/30">
              {filtered.map(s => (
                <tr key={s.uuid} className="table-row-hover">
                  <td className="px-6 py-3 font-mono text-sm">{s.uuid}</td>
                  <td className="px-6 py-3 text-sm text-[var(--text-secondary)] hidden md:table-cell">{s.usage_type}</td>
                  <td className="px-6 py-3 text-sm text-[var(--text-muted)] hidden md:table-cell">{s.usage_id}</td>
                  <td className="px-6 py-3 text-right">
                    <div className="flex items-center justify-end gap-1">
                      <button onClick={() => showXml(s.uuid)} className="p-1.5 hover:bg-white/10 rounded transition" title="View XML" aria-label="View XML"><Code className={`w-4 h-4 ${statusToneClass('info')}`} /></button>
                      <button onClick={() => setDeleteTarget(s.uuid)} className="p-1.5 hover:bg-red-600/20 rounded transition" title="Delete" aria-label="Delete"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <ConfirmDialog open={!!deleteTarget} title="Delete Secret" message={`Delete secret '${deleteTarget}'?`} confirmLabel="Delete" onConfirm={handleDelete} onCancel={() => setDeleteTarget(null)} />

      {defineOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" role="dialog" aria-modal="true" onClick={() => !defSaving && setDefineOpen(false)}>
          <div ref={defineRef} className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-2xl mx-4 max-h-[90vh] flex flex-col" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center justify-between p-5 border-b border-[var(--apple-hairline)]">
              <span className="text-lg font-semibold">Define libvirt secret</span>
              <button type="button" aria-label="Close" disabled={defSaving} onClick={() => setDefineOpen(false)} className="text-[var(--text-muted)] hover:text-[var(--text-primary)] p-1 hover:bg-[var(--surface-hover)] rounded-lg transition"><X className="w-4 h-4" /></button>
            </div>
            <div className="p-5 space-y-3 overflow-y-auto flex-1">
              <label htmlFor="define-secret-xml" className="block text-sm text-[var(--text-muted)]">Secret XML (<code className="text-[var(--text-muted)]">virSecretDefineXML</code>)</label>
              <textarea id="define-secret-xml" value={defXml} onChange={(e) => setDefXml(e.target.value)} rows={8} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm font-mono text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)]" placeholder={'<secret ephemeral=\'no\'>\n  <description>…</description>\n</secret>'} />
              <label htmlFor="define-secret-value" className="block text-sm text-[var(--text-muted)]">Value (base64, optional)</label>
              <textarea id="define-secret-value" value={defValueB64} onChange={(e) => setDefValueB64(e.target.value)} rows={2} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm font-mono text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)]" placeholder="Base64-encoded secret bytes (optional)" />
              <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)] cursor-pointer">
                <input type="checkbox" checked={defValidate} onChange={(e) => setDefValidate(e.target.checked)} />
                Validate XML against schema
              </label>
              <p className="text-xs text-[var(--text-muted)]">After define, the secret UUID is returned. Use the XML usage section expected by your storage pool (Ceph, iSCSI, etc.).</p>
            </div>
            <div className="flex justify-end gap-3 px-5 pb-5 border-t border-[var(--apple-hairline)] pt-4">
              <button type="button" disabled={defSaving} onClick={() => setDefineOpen(false)} className="btn-secondary text-sm font-medium transition">Cancel</button>
              <button type="button" disabled={defSaving} onClick={handleDefineSecret} className="btn-primary text-sm disabled:opacity-50">{defSaving ? 'Saving…' : 'Define'}</button>
            </div>
          </div>
        </div>
      )}

      {xmlContent !== null && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" role="dialog" aria-modal="true" onClick={() => setXmlContent(null)}>
          <div ref={xmlRef} className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-3xl mx-4 max-h-[80vh] flex flex-col" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center justify-between p-5 border-b border-[var(--apple-hairline)]">
              <span className="text-lg font-semibold font-mono">{xmlUuid}</span>
              <button aria-label="Close" onClick={() => setXmlContent(null)} className="text-[var(--text-muted)] hover:text-[var(--text-primary)] p-1 hover:bg-[var(--surface-hover)] rounded-lg transition"><X className="w-4 h-4" /></button>
            </div>
            <pre className="p-5 text-sm text-[var(--text-secondary)] overflow-auto whitespace-pre-wrap font-mono flex-1">{xmlContent}</pre>
          </div>
        </div>
      )}
    </PageLayout>
  )
}

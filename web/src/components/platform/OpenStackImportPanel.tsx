// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { MacGlassPanel } from './mac/PlatformMacUi'
import {
  createOsConnection,
  getOsImportJob,
  listOsConnections,
  listOsInventory,
  submitOsImport,
  syncOsInventory,
  type OsConnection,
  type OsInventoryItem,
} from '../../api/openstackImport'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function OpenStackImportPanel() {
  const toast = useToastContext()
  const [connections, setConnections] = useState<OsConnection[]>([])
  const [selected, setSelected] = useState<string>('')
  const [items, setItems] = useState<OsInventoryItem[]>([])
  const [busy, setBusy] = useState(false)
  const [name, setName] = useState('openstack')
  const [authUrl, setAuthUrl] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [project, setProject] = useState('admin')
  const [jobNote, setJobNote] = useState<string | null>(null)

  const refresh = async () => {
    const rows = await listOsConnections()
    setConnections(Array.isArray(rows) ? rows : [])
    if (!selected && rows?.[0]?.id) setSelected(rows[0].id)
  }

  useEffect(() => {
    void refresh().catch((e) => toast.error(formatUserError(e)))
  }, [])

  useEffect(() => {
    if (!selected) return
    void listOsInventory(selected)
      .then((r) => setItems(r.items ?? []))
      .catch(() => setItems([]))
  }, [selected])

  const onCreate = async () => {
    setBusy(true)
    try {
      const c = await createOsConnection({
        name,
        auth_url: authUrl,
        username,
        password,
        project_name: project,
        insecure_tls: true,
      })
      toast.success(`Connection ${c.name} saved`)
      await refresh()
      setSelected(c.id)
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const onSync = async () => {
    if (!selected) return
    setBusy(true)
    try {
      const r = await syncOsInventory(selected)
      setItems(r.items ?? [])
      toast.success(`Synced ${r.count} resources`)
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const onImport = async (item: OsInventoryItem) => {
    if (!selected || item.kind !== 'image') {
      toast.warning('Only Glance images can be imported in v1')
      return
    }
    setBusy(true)
    try {
      const job = await submitOsImport(selected, {
        kind: 'image',
        remote_id: item.remote_id,
        dest_name: item.name || undefined,
      })
      setJobNote(`Job ${job.job_id} ${job.status}`)
      const poll = window.setInterval(() => {
        void getOsImportJob(job.job_id)
          .then((st) => {
            setJobNote(`Job ${st.id}: ${st.status} (${st.progress}%)`)
            if (st.status === 'done' || st.status === 'failed') {
              window.clearInterval(poll)
              if (st.status === 'done') toast.success('Image imported as template')
              else toast.error(st.error || 'Import failed')
            }
          })
          .catch(() => window.clearInterval(poll))
      }, 2000)
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-4">
      <MacGlassPanel title="OpenStack connection">
        <p className="text-sm text-[var(--text-secondary)] mb-3">
          Connect to Keystone, sync Glance/Nova/Cinder inventory, and import Glance images as Fleet Cloud templates.
        </p>
        <div className="grid gap-2 sm:grid-cols-2">
          <input className="rounded-lg border border-[var(--apple-hairline)] bg-transparent px-3 py-2 text-sm" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
          <input className="rounded-lg border border-[var(--apple-hairline)] bg-transparent px-3 py-2 text-sm" placeholder="Auth URL (…/v3)" value={authUrl} onChange={(e) => setAuthUrl(e.target.value)} />
          <input className="rounded-lg border border-[var(--apple-hairline)] bg-transparent px-3 py-2 text-sm" placeholder="Username" value={username} onChange={(e) => setUsername(e.target.value)} />
          <input className="rounded-lg border border-[var(--apple-hairline)] bg-transparent px-3 py-2 text-sm" placeholder="Password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
          <input className="rounded-lg border border-[var(--apple-hairline)] bg-transparent px-3 py-2 text-sm" placeholder="Project" value={project} onChange={(e) => setProject(e.target.value)} />
        </div>
        <div className="mt-3 flex flex-wrap gap-2">
          <button type="button" disabled={busy || !authUrl} onClick={() => void onCreate()} className="rounded-lg bg-[#0071e3] px-3 py-2 text-sm text-white disabled:opacity-50">
            {busy ? <Loader2 className="inline h-4 w-4 animate-spin" /> : null} Save connection
          </button>
          <select className="rounded-lg border border-[var(--apple-hairline)] bg-transparent px-3 py-2 text-sm" value={selected} onChange={(e) => setSelected(e.target.value)}>
            <option value="">Select connection…</option>
            {connections.map((c) => (
              <option key={c.id} value={c.id}>{c.name}</option>
            ))}
          </select>
          <button type="button" disabled={busy || !selected} onClick={() => void onSync()} className="rounded-lg border border-[var(--apple-hairline)] px-3 py-2 text-sm disabled:opacity-50">
            Sync inventory
          </button>
        </div>
        {jobNote && <p className="mt-2 text-xs text-[var(--text-secondary)]">{jobNote}</p>}
      </MacGlassPanel>

      <MacGlassPanel title="Inventory">
        {items.length === 0 ? (
          <p className="text-sm text-[var(--text-secondary)]">No inventory yet — sync a connection.</p>
        ) : (
          <ul className="divide-y divide-[var(--apple-hairline)]">
            {items.map((it) => (
              <li key={`${it.kind}:${it.remote_id}`} className="flex items-center justify-between gap-3 py-2 text-sm">
                <div>
                  <span className="font-medium">{it.name || it.remote_id}</span>
                  <span className="ml-2 text-[var(--text-muted)]">{it.kind} · {it.status}</span>
                </div>
                {it.kind === 'image' && (
                  <button type="button" disabled={busy} onClick={() => void onImport(it)} className="rounded-lg border border-[var(--apple-hairline)] px-2 py-1 text-xs disabled:opacity-50">
                    Import
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </MacGlassPanel>
    </div>
  )
}

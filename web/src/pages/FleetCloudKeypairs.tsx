// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { createKeypair, deleteKeypair, listKeypairs, type NativeKeypair } from '../api/nativeKeypairs'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import ConfirmDialog from '../components/ConfirmDialog'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { Key, RefreshCw } from 'lucide-react'
import { platformStatSubtitle } from '../components/platform/PlatformPageChrome'

// Native SSH keypair catalog — no old external-cloud gate component in the way
// any more (the daemon's external-cloud-client integration has since been
// fully removed). Import-only: no server-side keypair generation (Machina
// never hands out private keys over an API) — see api/nativeKeypairs.ts.
export default function FleetCloudKeypairsPage() {
  return <FleetCloudKeypairsContent />
}

function FleetCloudKeypairsContent() {
  const toast = useToastContext()
  const [keys, setKeys] = useState<NativeKeypair[]>([])
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [publicKey, setPublicKey] = useState('')
  const [creating, setCreating] = useState(false)
  const [search, setSearch] = useState('')
  const [pendingDelete, setPendingDelete] = useState<NativeKeypair | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const list = await listKeypairs()
      setKeys(list)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => {
    void load()
  }, [load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return keys
    return keys.filter(
      (k) =>
        k.name.toLowerCase().includes(q) ||
        k.fingerprint.toLowerCase().includes(q) ||
        k.id.toLowerCase().includes(q),
    )
  }, [keys, search])

  return (
    <PageLayout
      className="w-full max-w-none"
      prepend={<FleetCloudSubNav />}
      eyebrow="Fleet Cloud"
      title="SSH keypairs"
      subtitle={platformStatSubtitle([{ label: 'Keypairs', value: keys.length }])}
      icon={<Key className="w-7 h-7 text-[var(--accent)]" />}
      contentLoading={loading && keys.length === 0}
      actions={
        <button type="button" onClick={() => void load()}
          className="btn-secondary text-sm inline-flex items-center gap-1">
          <RefreshCw className="w-4 h-4" /> Refresh
        </button>
      }
    >
      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3 mb-4">
        <div className="flex flex-wrap gap-2">
          <input aria-label="Keypair name" value={name} onChange={(e) => setName(e.target.value)} placeholder="name"
            className="input-field text-sm" />
          <button type="button" disabled={creating || !name.trim() || !publicKey.trim()}
            className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed"
            onClick={async () => {
              if (!name.trim() || !publicKey.trim() || creating) return
              setCreating(true)
              try {
                await createKeypair({ name: name.trim(), public_key: publicKey.trim() })
                toast.success(`Keypair '${name.trim()}' imported`)
                setName('')
                setPublicKey('')
                void load()
              } catch (e: unknown) {
                toast.error(formatUserError(e))
              } finally {
                setCreating(false)
              }
            }}>
            {creating ? 'Importing…' : 'Import'}
          </button>
        </div>
        <textarea aria-label="SSH public key" value={publicKey} onChange={(e) => setPublicKey(e.target.value)} rows={3}
          placeholder="Paste public key (ssh-rsa AAAA... or ssh-ed25519 AAAA...)"
          className="w-full input-field text-xs font-mono" />
      </div>

      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search name or fingerprint…"
      />

      <TahoeTableWrap>
        <table className="apple-table" aria-label="SSH keypairs">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Fingerprint</th>
              <th scope="col" className="text-right">Actions</th>
            </tr>
          </thead>
          <tbody>
            {loading && filtered.length === 0 && (
              <tr>
                <td colSpan={3} className="text-center text-[var(--text-muted)]">Loading…</td>
              </tr>
            )}
            {!loading && filtered.length === 0 && (
              <tr>
                <td colSpan={3} className="text-center text-[var(--text-muted)]">
                  {search.trim() ? 'No keypairs match your search.' : 'No keypairs imported yet.'}
                </td>
              </tr>
            )}
            {filtered.map((k) => (
              <tr key={k.id}>
                <td className="font-mono text-[var(--text-primary)]">{k.name}</td>
                <td className="text-[var(--text-muted)] text-xs">{k.fingerprint}</td>
                <td className="text-right">
                  <button type="button" className={statusActionLinkClasses('error', 'text-xs')}
                    onClick={() => setPendingDelete(k)}>Delete</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TahoeTableWrap>

      <ConfirmDialog
        open={!!pendingDelete}
        title="Delete keypair"
        message={pendingDelete ? `Delete keypair ${pendingDelete.name}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDelete(null)}
        onConfirm={async () => {
          if (!pendingDelete) return
          const target = pendingDelete
          setPendingDelete(null)
          try {
            await deleteKeypair(target.id)
            toast.success('Deleted')
            void load()
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          }
        }}
      />

      <FleetCloudFooter />
    </PageLayout>
  )
}

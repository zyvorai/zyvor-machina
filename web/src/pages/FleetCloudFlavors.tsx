// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { createFlavor, deleteFlavor, listFlavors, type NativeFlavor } from '../api/flavors'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { Cpu, Loader2, Plus, RefreshCw, Trash2 } from 'lucide-react'
import { platformStatSubtitle } from '../components/platform/PlatformPageChrome'

// Native flavor catalog — unlike the rest of the /fleet-cloud/* pages, this one no
// longer depends on a wired external cloud (see api/flavors.ts). The
// the old external-cloud gate component it once needed to skip is gone too: the daemon's
// external-cloud-client integration has since been fully removed, so this
// page always renders.
export default function FleetCloudFlavorsPage() {
  return <FleetCloudFlavorsContent />
}

function FleetCloudFlavorsContent() {
  const toast = useToastContext()
  const [flavors, setFlavors] = useState<NativeFlavor[]>([])
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [vcpus, setVcpus] = useState('1')
  const [ram, setRam] = useState('2048')
  const [disk, setDisk] = useState('20')
  const [creating, setCreating] = useState(false)
  const [search, setSearch] = useState('')
  const [pendingDelete, setPendingDelete] = useState<NativeFlavor | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const list = await listFlavors()
      setFlavors(list)
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
    if (!q) return flavors
    return flavors.filter(
      (f) => f.name.toLowerCase().includes(q) || f.id.toLowerCase().includes(q),
    )
  }, [flavors, search])

  const handleCreate = async () => {
    if (!name.trim()) {
      toast.error('Name is required')
      return
    }
    // Empty → sensible default; but a typed negative previously passed straight
    // through (Number('-4') is truthy) and 0 was silently coerced to the default.
    const vcpusN = vcpus.trim() ? Number(vcpus) : 1
    const ramN = ram.trim() ? Number(ram) : 512
    const diskN = disk.trim() ? Number(disk) : 0
    if (!Number.isFinite(vcpusN) || vcpusN < 1 || !Number.isFinite(ramN) || ramN < 1 || !Number.isFinite(diskN) || diskN < 0) {
      toast.error('vCPUs and RAM must be at least 1; disk must be 0 or more')
      return
    }
    setCreating(true)
    try {
      await createFlavor({
        name: name.trim(),
        vcpus: vcpusN,
        memory_mib: ramN,
        disk_gib: diskN,
        is_public: true,
      })
      toast.success('Flavor created')
      setName('')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setCreating(false)
    }
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Cpu className="w-7 h-7 text-[var(--accent)]" />
        Compute flavors
      </h1>
      {platformStatSubtitle([{ label: 'Flavors', value: flavors.length }])}
      <p className="text-sm text-[var(--text-muted)]">Flavor catalog — create and delete require admin role.</p>
      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)] flex items-center gap-2"><Plus className="w-4 h-4" /> Create flavor</h2>
        <div className="grid sm:grid-cols-2 gap-3">
          <input aria-label="Flavor name" value={name} onChange={(e) => setName(e.target.value)} placeholder="Name"
            className="input-field text-sm" />
          <input aria-label="vCPUs" value={vcpus} onChange={(e) => setVcpus(e.target.value)} placeholder="vCPUs" type="number" min={1}
            className="input-field text-sm" />
          <input aria-label="RAM in MB" value={ram} onChange={(e) => setRam(e.target.value)} placeholder="RAM (MB)" type="number" min={512}
            className="input-field text-sm" />
          <input aria-label="Disk in GB" value={disk} onChange={(e) => setDisk(e.target.value)} placeholder="Disk (GB)" type="number" min={0}
            className="input-field text-sm" />
        </div>
        <button type="button" disabled={creating} onClick={() => void handleCreate()}
          className="btn-primary text-sm disabled:opacity-50">
          Create
        </button>
      </section>
      <button type="button" onClick={() => void load()}
        className="btn-secondary text-sm inline-flex items-center gap-1">
        <RefreshCw className="w-4 h-4" /> Refresh
      </button>
      {loading ? (
        <Loader2 className="w-8 h-8 animate-spin text-[var(--accent)]" />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search name or ID…"
          />
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Flavors">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">vCPU</th>
                  <th scope="col">RAM</th>
                  <th scope="col">Disk</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={5} className="text-center text-[var(--text-muted)]">
                      {search.trim() ? 'No flavors match your search.' : 'No flavors returned from Compute.'}
                    </td>
                  </tr>
                )}
                {filtered.map((f) => (
                  <tr key={f.id}>
                    <td>
                      <Link to={`/fleet-cloud/flavors/${f.id}`} className="apple-link font-mono">{f.name}</Link>
                      <span className="block text-xs text-[var(--text-muted)] font-mono">{f.id}</span>
                    </td>
                    <td>{f.vcpus}</td>
                    <td>{f.memory_mib} MiB</td>
                    <td>{f.disk_gib} GiB</td>
                    <td className="flex gap-2">
                      <Link to={`/fleet-cloud/flavors/${f.id}`} className="text-xs text-[var(--accent)] hover:underline">Open</Link>
                      <button type="button" className={statusActionLinkClasses('error', 'text-xs inline-flex items-center gap-0.5')}
                        onClick={() => setPendingDelete(f)}>
                        <Trash2 className="w-3 h-3" /> Del
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </>
      )}
      <ConfirmDialog
        open={!!pendingDelete}
        title="Delete flavor"
        message={pendingDelete ? `Delete flavor ${pendingDelete.name}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDelete(null)}
        onConfirm={async () => {
          if (!pendingDelete) return
          const target = pendingDelete
          setPendingDelete(null)
          try {
            await deleteFlavor(target.id)
            toast.success('Deleted')
            void load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}

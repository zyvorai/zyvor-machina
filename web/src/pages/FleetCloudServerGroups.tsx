// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { listVms, type NativeVm } from '../api/nativeVms'
import { addVmToServerGroup, deleteServerGroup, listServerGroups, type DerivedServerGroup } from '../api/nativeServerGroups'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import ConfirmDialog from '../components/ConfirmDialog'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { Layers, Loader2, RefreshCw } from 'lucide-react'

// Native anti-affinity groups — no old external-cloud gate component in the
// way any more (the daemon's external-cloud-client integration has
// since been fully removed). A "group" is derived from VM tags (see
// api/nativeServerGroups.ts), not a stored resource — only anti-affinity is
// supported (Machina's placement engine only enforces that policy).
export default function FleetCloudServerGroupsPage() {
  return <FleetCloudServerGroupsContent />
}

function FleetCloudServerGroupsContent() {
  const toast = useToastContext()
  const [groups, setGroups] = useState<DerivedServerGroup[]>([])
  const [vms, setVms] = useState<NativeVm[]>([])
  const [loading, setLoading] = useState(true)
  const [groupName, setGroupName] = useState('')
  const [vmId, setVmId] = useState('')
  const [search, setSearch] = useState('')
  const [pendingDelete, setPendingDelete] = useState<DerivedServerGroup | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [g, v] = await Promise.all([listServerGroups(), listVms()])
      setGroups(g)
      setVms(v)
      if (!vmId && v.length > 0) setVmId(v[0].id)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setLoading(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [toast])

  useEffect(() => {
    void load()
  }, [load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return groups
    return groups.filter((g) => g.name.toLowerCase().includes(q))
  }, [groups, search])

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Layers className="w-7 h-7 text-[var(--accent)]" />
        Anti-affinity groups
      </h1>
      <p className="text-sm text-[var(--text-muted)]">
        {groups.length} group{groups.length === 1 ? '' : 's'} — Machina's placement engine avoids co-locating VMs that share a group on the same host.
      </p>
      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 flex flex-wrap gap-3 items-end">
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">Group name</label>
          <input value={groupName} onChange={(e) => setGroupName(e.target.value)}
            aria-label="Group name"
            className="input-field text-sm" />
        </div>
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">VM</label>
          <select value={vmId} onChange={(e) => setVmId(e.target.value)}
            aria-label="VM"
            className="input-field text-sm">
            {vms.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
          </select>
        </div>
        <button type="button" disabled={!groupName.trim() || !vmId}
          className="btn-primary text-sm disabled:opacity-40"
          onClick={async () => {
            const vm = vms.find((v) => v.id === vmId)
            if (!vm) return
            try {
              await addVmToServerGroup(vm, groupName.trim())
              toast.success(`Added '${vm.name}' to group '${groupName.trim()}'`)
              setGroupName('')
              void load()
            } catch (e: unknown) {
              toast.error(formatUserError(e))
            }
          }}>
          Add to group
        </button>
        <button type="button" onClick={() => void load()}
          className="ml-auto btn-secondary text-sm inline-flex items-center gap-1">
          <RefreshCw className="w-4 h-4" /> Refresh
        </button>
      </div>

      {loading ? (
        <Loader2 className="w-8 h-8 animate-spin text-[var(--accent)]" />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search group name…"
          />
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Anti-affinity groups">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Policy</th>
                  <th scope="col">Members</th>
                  <th scope="col" className="text-right">Actions</th>
                </tr>
              </thead>
              <tbody>
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={4} className="text-center text-[var(--text-muted)]">
                      {search.trim() ? 'No groups match your search.' : 'No anti-affinity groups yet.'}
                    </td>
                  </tr>
                )}
                {filtered.map((g) => (
                  <tr key={g.name}>
                    <td>
                      <Link
                        to={`/fleet-cloud/server-groups/${encodeURIComponent(g.name)}`}
                        className="apple-link font-mono"
                      >
                        {g.name}
                      </Link>
                    </td>
                    <td className="text-xs text-[var(--text-muted)]">anti-affinity</td>
                    <td className="text-xs text-[var(--text-muted)]">{g.members.length}</td>
                    <td className="text-right">
                      <button type="button" className={statusActionLinkClasses('error', 'text-xs')}
                        onClick={() => setPendingDelete(g)}>Delete</button>
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
        title="Delete group"
        message={pendingDelete ? `Delete group ${pendingDelete.name}? This removes the tag from all ${pendingDelete.members.length} member VM(s).` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDelete(null)}
        onConfirm={async () => {
          if (!pendingDelete) return
          const target = pendingDelete
          setPendingDelete(null)
          try {
            await deleteServerGroup(target)
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

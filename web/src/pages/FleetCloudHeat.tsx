// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { Layers, Loader2, Plus, Trash2 } from 'lucide-react'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import StackComposer from '../components/flow/StackComposer'
import { DriftBadge } from '../components/flow/StackPlan'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { createStack, deleteStack, listStacks, type NativeStack, type StackTemplate } from '../api/stacks'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'

const MINIMAL_TEMPLATE: StackTemplate = {
  security_groups: [],
  volumes: [],
  vms: [],
}

// Native stacks — this feature is libvirt-native and does not depend on a wired
// external cloud (there's no old external-cloud gate component in the way
// either: the daemon's external-cloud-client integration has since
// been fully removed). Unlike Heat, a template here is a fixed JSON shape
// (security_groups/volumes/vms), not an arbitrary resource-type graph — see
// api/stacks.ts.
export default function FleetCloudHeatPage() {
  return <FleetCloudHeatContent />
}

function FleetCloudHeatContent() {
  const toast = useToastContext()
  const [stacks, setStacks] = useState<NativeStack[]>([])
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [templateJson, setTemplateJson] = useState(JSON.stringify(MINIMAL_TEMPLATE, null, 2))
  const [search, setSearch] = useState('')
  const [pendingDelete, setPendingDelete] = useState<NativeStack | null>(null)

  const load = useCallback(async (quiet = false) => {
    if (!quiet) setLoading(true)
    try {
      const s = await listStacks()
      setStacks(s)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setStacks([])
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    if (!stacks.some((s) => s.status === 'creating' || s.status === 'updating')) return
    const t = window.setTimeout(() => void load(true), 5000)
    return () => window.clearTimeout(t)
  }, [stacks, load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return stacks
    return stacks.filter(
      (s) => s.name.toLowerCase().includes(q) || s.id.toLowerCase().includes(q) || s.status.toLowerCase().includes(q),
    )
  }, [stacks, search])

  return (
    <PageLayout
      hideHeader
      prepend={<><FleetCloudSubNav /></>}
      ><p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Layers className="w-7 h-7 text-[var(--accent)]" /> Stacks
      </h1>
      <p className="text-[var(--text-muted)] text-sm">
        Describe an application in plain English. Zyvor drafts the instance groups and
        who-talks-to-whom policies, checks quota, placement and monthly cost, and replays
        the policies against recorded traffic. After approval the stack is deployed and
        kept matching its template.
      </p>

      <StackComposer onDeployed={() => void load(true)} />

      <details className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <summary className="text-sm font-medium text-[var(--text-secondary)] cursor-pointer inline-flex items-center gap-2"><Plus className="w-4 h-4" /> Create from JSON</summary>
        <input aria-label="Stack name" value={name} onChange={(e) => setName(e.target.value)} placeholder="Stack name"
          className="w-full max-w-md input-field text-sm" />
        <label className="block text-xs text-[var(--text-muted)]">
          Template — {'{'}security_groups: [{'{'}name, rules[]{'}'}], volumes: [{'{'}name, size_gib{'}'}], vms: [{'{'}name, memory, cpu_cores, disk_gib, network, attach_volumes[]{'}'}]{'}'}
        </label>
        <textarea aria-label="Stack template (JSON)" value={templateJson} onChange={(e) => setTemplateJson(e.target.value)} rows={8}
          className="w-full font-mono text-xs input-field" />
        <button type="button" className="btn-primary text-sm"
          onClick={async () => {
            if (!name.trim()) { toast.warning('Stack name required'); return }
            let template: StackTemplate
            try {
              template = JSON.parse(templateJson) as StackTemplate
            } catch {
              toast.error('Template must be valid JSON')
              return
            }
            try {
              await createStack({ name: name.trim(), template })
              toast.success('Stack created')
              setName('')
              void load()
            } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}>Create</button>
      </details>

      {loading ? (
        <Loader2 className="w-8 h-8 animate-spin text-[var(--accent)] mx-auto" />
      ) : stacks.length === 0 ? (
        <EmptyState title="No stacks" description="No stacks in this project yet." />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search name or status…"
          />
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Stacks">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Status</th>
                  <th scope="col">Resources</th>
                  <th scope="col">Drift</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={5} className="text-center text-[var(--text-muted)]">No stacks match your search.</td>
                  </tr>
                )}
                {filtered.map((s) => (
                  <tr key={s.id}>
                    <td>
                      <Link to={`/fleet-cloud/heat/${encodeURIComponent(s.name)}/${encodeURIComponent(s.id)}`}
                        className="apple-link">{s.name}</Link>
                    </td>
                    <td>{s.status}</td>
                    <td className="text-[var(--text-muted)]">{s.resources_json.length}</td>
                    <td><DriftBadge stack={s} /></td>
                    <td className="text-right">
                      <button type="button" className={statusActionLinkClasses('error', 'inline-flex items-center gap-1')}
                        onClick={() => setPendingDelete(s)}>
                        <Trash2 className="w-3.5 h-3.5" /> Delete
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
        title="Delete stack"
        message={pendingDelete ? `Delete stack ${pendingDelete.name}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDelete(null)}
        onConfirm={async () => {
          if (!pendingDelete) return
          const target = pendingDelete
          setPendingDelete(null)
          try {
            await deleteStack(target.id)
            toast.success('Deleted')
            void load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}

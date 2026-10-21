// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Boxes, Plus, Trash2 } from 'lucide-react'
import ConfirmDialog from '../../components/ConfirmDialog'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import { listPlatformHosts, type PlatformHost } from '../../api/platform'
import {
  addK8sNode,
  createK8sCluster,
  deleteK8sCluster,
  downloadK8sKubeconfig,
  getK8sCluster,
  listK8sClusters,
  removeK8sNode,
  upgradeK8sCluster,
  type K8sCluster,
  type K8sClusterDetail,
} from '../../api/platformFleetFeatures'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

const BUSY = new Set(['provisioning', 'upgrading', 'deleting'])

export default function PlatformK8sClusters() {
  const toast = useToastContext()
  const [clusters, setClusters] = useState<K8sCluster[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [selected, setSelected] = useState<K8sClusterDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [version, setVersion] = useState('v1.31.4+k3s1')
  const [serverHost, setServerHost] = useState('')
  const [agentHosts, setAgentHosts] = useState<string[]>([])
  const [newVersion, setNewVersion] = useState('')
  const [addHost, setAddHost] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)

  const hostName = (id: string) => hosts.find((h) => h.id === id)?.hostname ?? id

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [c, h] = await Promise.all([listK8sClusters(), listPlatformHosts()])
      setClusters(c)
      setHosts(h)
      setSelected((cur) => cur)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  // While anything is in flight, refresh the open cluster and the list.
  useEffect(() => {
    const busy = clusters.some((c) => BUSY.has(c.status)) || (selected ? BUSY.has(selected.status) : false)
    if (!busy) return undefined
    const t = setInterval(() => {
      void (async () => {
        try {
          setClusters(await listK8sClusters())
          if (selected) setSelected(await getK8sCluster(selected.id))
        } catch { /* keep the last view */ }
      })()
    }, 5000)
    return () => clearInterval(t)
  }, [clusters, selected])

  const act = async (label: string, fn: () => Promise<K8sClusterDetail | unknown>) => {
    try {
      const res = await fn()
      toast.success(label)
      if (res && typeof res === 'object' && 'nodes' in res) setSelected(res as K8sClusterDetail)
      setClusters(await listK8sClusters())
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const open = async (id: string) => {
    try { setSelected(await getK8sCluster(id)) } catch (e: unknown) { toast.error(formatUserError(e)) }
  }

  const create = () => act('Cluster creation started', () =>
    createK8sCluster({ name: name.trim(), version: version.trim(), server_host_id: serverHost, agent_host_ids: agentHosts }))

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Kubernetes clusters (k3s)"
      subtitle="One server node plus agent nodes on enrolled hosts. Needs MACHINA_K3S_FLEET=1 on the controller and MACHINA_AGENT_K3S_ENABLED=1 on each host."
      icon={<Boxes className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && clusters.length === 0}
    >
      <OperatingSurfaceLayout testId="platform-k8s-clusters-page">
        <MacGlassPanel title="New cluster">
          <div className="grid gap-3 md:grid-cols-2 max-w-3xl">
            <input className="input text-sm" aria-label="Cluster name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
            <input className="input text-sm" aria-label="k3s version" placeholder="v1.31.4+k3s1" value={version} onChange={(e) => setVersion(e.target.value)} />
            <select className="input text-sm" aria-label="Server host" value={serverHost} onChange={(e) => setServerHost(e.target.value)}>
              <option value="">Server host…</option>
              {hosts.map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
            </select>
            <select
              className="input text-sm"
              aria-label="Agent hosts"
              multiple
              value={agentHosts}
              onChange={(e) => setAgentHosts(Array.from(e.target.selectedOptions).map((o) => o.value))}
            >
              {hosts.filter((h) => h.id !== serverHost).map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
            </select>
          </div>
          <button type="button" className="btn-primary text-sm mt-3 flex items-center gap-1.5" disabled={!name.trim() || !serverHost} onClick={() => void create()}>
            <Plus className="w-4 h-4" /> Create cluster
          </button>
        </MacGlassPanel>

        <MacGlassPanel title="Clusters" subtitle={`${clusters.length} cluster(s)`}>
          {clusters.length === 0 && !loading ? (
            <PlatformEmptyState icon={Boxes} title="No clusters yet" subtitle="Create one from enrolled hosts." />
          ) : (
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {clusters.map((c) => (
                <MacListRow
                  key={c.id}
                  title={c.name}
                  subtitle={`${c.version} · ${c.status}${c.message ? ` · ${c.message}` : ''}`}
                  onClick={() => void open(c.id)}
                />
              ))}
            </ul>
          )}
        </MacGlassPanel>

        {selected && (
          <MacGlassPanel
            title={selected.name}
            subtitle={`${selected.status} · ${selected.api_url || 'no API URL yet'}`}
            action={
              <div className="flex gap-2">
                <button type="button" className="btn-secondary text-xs" disabled={!selected.api_url} onClick={() => void downloadK8sKubeconfig(selected.id, selected.name).catch((e: unknown) => toast.error(formatUserError(e)))}>
                  Kubeconfig
                </button>
                <button type="button" className="btn-secondary text-xs p-1.5" aria-label="Delete cluster" disabled={BUSY.has(selected.status)} onClick={() => setConfirmDelete(true)}>
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            }
          >
            {selected.message && <p className="text-xs text-amber-400 mb-2">{selected.message}</p>}
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {selected.nodes.map((n) => (
                <MacListRow
                  key={n.id}
                  title={`${hostName(n.host_id)} (${n.role})`}
                  subtitle={`${n.status}${n.version ? ` · ${n.version}` : ''}${n.message ? ` · ${n.message}` : ''}`}
                  badge={
                    n.role === 'agent' ? (
                      <button type="button" className="btn-secondary text-xs" disabled={BUSY.has(selected.status)} onClick={() => void act('Node removal started', () => removeK8sNode(selected.id, n.id))}>
                        Remove
                      </button>
                    ) : undefined
                  }
                />
              ))}
            </ul>
            <div className="flex flex-wrap gap-3 mt-3">
              <input className="input text-sm" aria-label="Upgrade to version" placeholder="Upgrade to v1.31.5+k3s1" value={newVersion} onChange={(e) => setNewVersion(e.target.value)} />
              <button type="button" className="btn-secondary text-sm" disabled={!newVersion.trim() || selected.status !== 'ready'} onClick={() => void act('Upgrade started', () => upgradeK8sCluster(selected.id, newVersion.trim()))}>
                Upgrade
              </button>
              <select className="input text-sm" aria-label="Add a node" value={addHost} onChange={(e) => setAddHost(e.target.value)}>
                <option value="">Add a node…</option>
                {hosts.filter((h) => !selected.nodes.some((n) => n.host_id === h.id)).map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
              </select>
              <button type="button" className="btn-secondary text-sm" disabled={!addHost || selected.status !== 'ready'} onClick={() => void act('Node join started', () => addK8sNode(selected.id, addHost))}>
                Add node
              </button>
            </div>
            <p className="text-xs text-[var(--text-muted)] mt-2">Upgrades go server first, then one agent at a time. There is no node drain yet.</p>
          </MacGlassPanel>
        )}
      </OperatingSurfaceLayout>
      <ConfirmDialog
        open={confirmDelete}
        title="Delete cluster"
        message={`Remove k3s from every node of "${selected?.name}" and delete the cluster? Workloads on it are lost.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          if (selected) await act('Cluster deletion started', () => deleteK8sCluster(selected.id))
          setConfirmDelete(false)
        }}
      />
    </PlatformPageChrome>
  )
}

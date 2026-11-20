// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import ConfirmDialog from '../components/ConfirmDialog'
import { Link } from 'react-router'
import { Boxes, Copy, ExternalLink, Monitor, Network, Play, RefreshCw, RotateCw, Square, Terminal, Trash2 } from 'lucide-react'
import VNCViewer from '../components/VNCViewer'
import { fillViewportVncProps } from '../utils/embeddedVnc'
import KubeVirtSerialConsole from '../components/KubeVirtSerialConsole'
import KubeVirtExposeServiceModal from '../components/KubeVirtExposeServiceModal'
import K8sConnectionErrorBanner from '../components/K8sConnectionErrorBanner'
import {
  getK8sCronJobs,
  getK8sDaemonSets,
  getK8sDeployments,
  getK8sEvents,
  getK8sHelmReleases,
  getK8sIngresses,
  getK8sJobs,
  getK8sKubevirtVirtualMachines,
  getK8sKubevirtVmSummary,
  deleteK8sKubevirtVm,
  postK8sKubevirtVmLifecycle,
  type K8sKubeVirtVM,
  getK8sNamespaces,
  getK8sPersistentVolumeClaims,
  getK8sPersistentVolumes,
  getK8sPodLogs,
  getK8sPods,
  getK8sServices,
  getK8sStatefulSets,
  getK8sStorageClasses,
  postK8sApply,
  postK8sAuthCanI,
  K8sDeployment,
  K8sMetadataName,
  KubeVirtVmSummaryRow,
  K8sPod,
  K8sService,
  runK8sAction,
} from '../api/k8s'
import { useK8sContext } from '../hooks/useK8sContext'
import { useToastContext } from '../contexts/ToastContext'
import { usePlatformInfo } from '../contexts/PlatformInfoContext'
import { summarizeK8sClientError } from '../utils/k8sErrors'
import Hero from '../components/Hero'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import PageSkeleton from '../components/PageSkeleton'
import JsonInspector, { asArray, asRecord } from '../components/platform/JsonInspector'
import { formatUserError } from '../utils/apiError'
import { statusBadgeClasses, statusPillClasses, statusToneClass } from '../utils/semanticColors'
import { useExpandable } from '../hooks/useExpandable'
import { ExpandableToggle } from '../components/ui/ExpandableToggle'

export default function K8sWorkloadsPage() {
  const toast = useToastContext()
  const { context, setContext, choices: contextChoices, refreshChoices, ctxTrim } = useK8sContext()
  const { lastEvent, refreshKey } = usePlatformInfo()
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [namespace, setNamespace] = useState<string>('all')
  const [namespaces, setNamespaces] = useState<string[]>([])
  const [deployments, setDeployments] = useState<K8sDeployment[]>([])
  const [pods, setPods] = useState<K8sPod[]>([])
  const [services, setServices] = useState<K8sService[]>([])
  const [kubevirtRows, setKubevirtRows] = useState<KubeVirtVmSummaryRow[]>([])
  const [kubevirtListError, setKubevirtListError] = useState<string | null>(null)
  const [kubevirtVmCrs, setKubevirtVmCrs] = useState<K8sKubeVirtVM[]>([])
  const [kubevirtCrBusy, setKubevirtCrBusy] = useState(false)
  const [kubevirtVmBusy, setKubevirtVmBusy] = useState<string | null>(null)
  const [deleteKubevirtTarget, setDeleteKubevirtTarget] = useState<{ namespace: string; name: string } | null>(null)
  const [confirmDeleteResource, setConfirmDeleteResource] = useState<{
    payload: Parameters<typeof runK8sAction>[0]
    label: string
  } | null>(null)
  const [showKubevirtCreate, setShowKubevirtCreate] = useState(false)
  const [kubevirtCreateYaml, setKubevirtCreateYaml] = useState(`apiVersion: kubevirt.io/v1
kind: VirtualMachine
metadata:
  name: example-vm
  namespace: default
spec:
  running: false
  template:
    metadata:
      labels:
        kubevirt.io/vm: example-vm
    spec:
      domain:
        devices:
          disks:
            - name: containerdisk
              disk:
                bus: virtio
            - name: cloudinitdisk
              disk:
                bus: virtio
          interfaces:
            - name: default
              masquerade: {}
              ports:
                - port: 22
                  protocol: TCP
        resources:
          requests:
            memory: 1Gi
      networks:
        - name: default
          pod: {}
      volumes:
        - name: containerdisk
          containerDisk:
            image: quay.io/kubevirt/cirros-container-disk-demo
        - name: cloudinitdisk
          cloudInitNoCloud:
            userData: |
              #cloud-config
              password: cirros
              chpasswd: { expire: False }
`)
  const [liveKubeVirt, setLiveKubeVirt] = useState<null | { kind: 'vnc' | 'console'; namespace: string; name: string }>(null)
  const [exposeVm, setExposeVm] = useState<null | { name: string; namespace: string; nodeInternalIp?: string | null }>(null)
  const [acting, setActing] = useState<string | null>(null)
  const [scaleValue, setScaleValue] = useState<Record<string, number>>({})
  const [connectionError, setConnectionError] = useState<string | null>(null)
  const [statefulsets, setStatefulsets] = useState<K8sDeployment[]>([])
  const [daemonsets, setDaemonsets] = useState<K8sDeployment[]>([])
  const [jobs, setJobs] = useState<K8sMetadataName[]>([])
  const deploymentList = useExpandable(deployments, 40)
  const podList = useExpandable(pods, 50)
  const statefulsetList = useExpandable(statefulsets, 40)
  const daemonsetList = useExpandable(daemonsets, 40)
  const jobList = useExpandable(jobs, 40)
  const kubevirtRowList = useExpandable(kubevirtRows, 40)
  const [eventsText, setEventsText] = useState('')
  const [eventsItems, setEventsItems] = useState<unknown[]>([])
  const [logPod, setLogPod] = useState('')
  const [logNs, setLogNs] = useState('default')
  const [logContainer, setLogContainer] = useState('')
  const [logOut, setLogOut] = useState('')
  const [applyYaml, setApplyYaml] = useState('')
  const [applyDry, setApplyDry] = useState(true)
  const [applyOut, setApplyOut] = useState<unknown>(null)
  const [caniVerb, setCaniVerb] = useState('get')
  const [caniRes, setCaniRes] = useState('pods')
  const [caniNs, setCaniNs] = useState('')
  const [caniOut, setCaniOut] = useState('')
  const [helmJson, setHelmJson] = useState<unknown>(null)
  const [explorerKind, setExplorerKind] = useState('ingresses')
  const [explorerJson, setExplorerJson] = useState<unknown>(null)

  const nsValue = namespace === 'all' ? undefined : namespace
  // Monotonic request id: switching namespace/context fires a new load while the
  // previous one may still be in flight. Without this guard a slow older response
  // could resolve last and render namespace A's workloads under namespace B.
  const reqRef = useRef(0)

  const load = useCallback(async (background = false) => {
    const myReq = ++reqRef.current
    if (background) setRefreshing(true)
    try {
      const [ns, dep, pod, svc, sts, ds, jb] = await Promise.all([
        getK8sNamespaces(ctxTrim),
        getK8sDeployments(nsValue, ctxTrim),
        getK8sPods(nsValue, ctxTrim),
        getK8sServices(nsValue, ctxTrim),
        getK8sStatefulSets(nsValue, ctxTrim),
        getK8sDaemonSets(nsValue, ctxTrim),
        getK8sJobs(nsValue, ctxTrim),
      ])
      if (reqRef.current !== myReq) return
      setConnectionError(null)
      setNamespaces(
        (ns.items ?? [])
          .map((n) => n.metadata?.name)
          .filter((x): x is string => Boolean(x)),
      )
      setDeployments(dep.items ?? [])
      setPods(pod.items ?? [])
      setServices(svc.items ?? [])
      setStatefulsets(sts.items ?? [])
      setDaemonsets(ds.items ?? [])
      setJobs(jb.items ?? [])
      setKubevirtListError(null)
      try {
        const rows = await getK8sKubevirtVmSummary(nsValue, ctxTrim)
        if (reqRef.current !== myReq) return
        setKubevirtRows(Array.isArray(rows) ? rows : [])
      } catch (e: unknown) {
        if (reqRef.current !== myReq) return
        setKubevirtRows([])
        setKubevirtListError(formatUserError(e))
      }
    } catch (e: unknown) {
      if (reqRef.current !== myReq) return
      setConnectionError(formatUserError(e))
    } finally {
      if (reqRef.current === myReq) {
        setLoading(false)
        setRefreshing(false)
      }
    }
  }, [nsValue, ctxTrim])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    if (!lastEvent) return
    if (lastEvent.kind.startsWith('kubevirt.')) void load(true)
  }, [refreshKey, lastEvent, load])

  useEffect(() => {
    const t = window.setInterval(() => void load(true), 30_000)
    return () => window.clearInterval(t)
  }, [load])

  const loadKubevirtCrs = useCallback(async () => {
    setKubevirtCrBusy(true)
    try {
      const rows = await getK8sKubevirtVirtualMachines(nsValue, ctxTrim)
      const items = rows.items ?? []
      setKubevirtVmCrs(items)
      toast.success(`Loaded ${items.length} VirtualMachine CR(s)`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setKubevirtVmCrs([])
    } finally {
      setKubevirtCrBusy(false)
    }
  }, [ctxTrim, nsValue, toast])

  const runKubevirtLifecycle = useCallback(async (namespace: string, name: string, action: 'start' | 'stop' | 'restart') => {
    const key = `${action}:${namespace}/${name}`
    setKubevirtVmBusy(key)
    try {
      await postK8sKubevirtVmLifecycle(namespace, name, action, ctxTrim || undefined)
      toast.success(`KubeVirt VM ${action} requested for ${namespace}/${name}`)
      await load(true)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setKubevirtVmBusy(null)
    }
  }, [ctxTrim, load, toast])

  const deleteKubevirtVm = useCallback((namespace: string, name: string) => {
    setDeleteKubevirtTarget({ namespace, name })
  }, [])

  const applyKubevirtYaml = useCallback(async () => {
    setKubevirtVmBusy('create')
    try {
      const res = await postK8sApply(kubevirtCreateYaml, false, ctxTrim || undefined)
      toast.success(res.stdout?.trim() || 'VirtualMachine manifest applied')
      setShowKubevirtCreate(false)
      await load(true)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setKubevirtVmBusy(null)
    }
  }, [ctxTrim, kubevirtCreateYaml, load, toast])

  const copyText = useCallback((label: string, text: string) => {
    void navigator.clipboard.writeText(text).then(() => {
      toast.success(`${label} copied`)
    }).catch(() => {
      toast.error('Could not copy to clipboard')
    })
  }, [toast])

  const runAction = useCallback(async (payload: Parameters<typeof runK8sAction>[0]) => {
    setActing(`${payload.action}:${payload.name}`)
    try {
      const res = await runK8sAction({
        ...payload,
        ...(ctxTrim ? { context: ctxTrim } : {}),
      })
      toast.success(res.stdout.trim() || `Action ${payload.action} succeeded`)
      await load(true)
    } catch (e: unknown) {
      const raw = formatUserError(e)
      toast.error(`Action failed: ${summarizeK8sClientError(raw).headline}`)
    } finally {
      setActing(null)
    }
  }, [load, toast, ctxTrim])

  const byNs = useMemo(() => {
    const m: Record<string, { pods: number; deployments: number; services: number; kubevirtVms: number }> = {}
    const touch = (ns: string) => {
      if (!m[ns]) m[ns] = { pods: 0, deployments: 0, services: 0, kubevirtVms: 0 }
    }
    for (const d of deployments) {
      const ns = d.metadata.namespace || 'default'
      touch(ns)
      m[ns].deployments += 1
    }
    for (const p of pods) {
      const ns = p.metadata.namespace || 'default'
      touch(ns)
      m[ns].pods += 1
    }
    for (const s of services) {
      const ns = s.metadata?.namespace || 'default'
      touch(ns)
      m[ns].services += 1
    }
    for (const v of kubevirtRows) {
      const ns = v.namespace || 'default'
      touch(ns)
      m[ns].kubevirtVms += 1
    }
    return m
  }, [deployments, pods, services, kubevirtRows])

  if (loading) {
    return <PageSkeleton />
  }

  return (
    <PageLayout
      hideHeader
      className="relative"
    >
      <Hero
        eyebrow="Kubernetes"
        title="Kubernetes Workloads"
        subtitle="Pods show node + host IP; KubeVirt VMs merge VMI guest/pod IP & node InternalIP. Use Console / VNC to copy virtctl commands."
        icon={<Boxes className="w-6 h-6" />}
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <select
              aria-label="kubectl context"
              value={context}
              onChange={(e) => setContext(e.target.value)}
              className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm text-[var(--text-primary)] max-w-[18rem]"
              title="kubectl --context"
            >
              <option value="">Default kubeconfig context</option>
              {contextChoices.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
            <button
              type="button"
              className="px-2 py-2 text-xs rounded-lg bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] text-[var(--text-primary)] border border-[var(--apple-hairline)]"
              onClick={() => {
                refreshChoices()
                toast.success('Refreshing context list')
              }}
            >
              Refresh contexts
            </button>
            <select
              aria-label="Namespace"
              value={namespace}
              onChange={(e) => setNamespace(e.target.value)}
              className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm"
            >
              <option value="all">All namespaces</option>
              {namespaces.map((ns) => (
                <option key={ns} value={ns}>{ns}</option>
              ))}
            </select>
            <button onClick={() => void load(true)} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" aria-label="Refresh">
              <RefreshCw className={`w-4 h-4 ${refreshing ? 'animate-spin' : ''}`} />
            </button>
          </div>
        }
      />

      {connectionError && (
        <div className="sticky top-2 z-30">
          <K8sConnectionErrorBanner
            title="Could not load workloads from the API"
            message={connectionError}
            onDismiss={() => setConnectionError(null)}
          />
        </div>
      )}

      {!connectionError && deployments.length === 0 && pods.length === 0 && services.length === 0 && (
        <EmptyState
          icon={<Boxes className="w-6 h-6" />}
          title="No workloads in this scope"
          description="The API is reachable but there are no deployments, pods, or services in the selected namespace(s). Install a cluster from Kubernetes overview or switch context."
          primaryAction={
            <Link to="/k8s" className="btn-primary text-sm">
              Kubernetes overview
            </Link>
          }
          secondaryAction={
            <Link to="/settings" className="px-4 py-2 rounded-lg bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] text-sm text-[var(--text-primary)] border border-[var(--apple-hairline)] transition">
              Settings
            </Link>
          }
        />
      )}

      <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
        {Object.entries(byNs).slice(0, 9).map(([ns, counts]) => (
          <div key={ns} className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)] px-4 py-3">
            <div className="text-sm font-medium text-[var(--text-primary)]">{ns}</div>
            <div className="text-xs text-[var(--text-muted)] mt-1">
              {counts.deployments} deployments, {counts.pods} pods, {counts.services} services
              {counts.kubevirtVms > 0 ? `, ${counts.kubevirtVms} KubeVirt VMs` : ''}
            </div>
          </div>
        ))}
      </div>

      <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
        <div className="px-6 py-4 border-b border-[var(--apple-hairline)]">
          <h2 className="text-lg font-semibold">Deployments</h2>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-sm" aria-label="Deployments">
            <thead>
              <tr className="border-b border-[var(--apple-hairline)] text-[var(--text-muted)] text-xs uppercase tracking-wider">
                <th scope="col" className="text-left px-4 py-3">Name</th>
                <th scope="col" className="text-left px-4 py-3">Namespace</th>
                <th scope="col" className="text-left px-4 py-3">Ready</th>
                <th scope="col" className="text-left px-4 py-3">Replicas</th>
                <th scope="col" className="text-center px-4 py-3">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/30" id={deploymentList.listId}>
              {deployments.length === 0 && (
                <tr>
                  <td colSpan={5} className="px-4 py-8">
                    <EmptyState
                      className="py-6 border-0 bg-transparent"
                      title="No deployments"
                      description={namespace === 'all' ? 'No Deployment objects in any namespace.' : `No deployments in ${namespace}.`}
                    />
                  </td>
                </tr>
              )}
              {deploymentList.shown.map((d) => {
                const key = `${d.metadata.namespace || 'default'}/${d.metadata.name}`
                const replicaCurrent = d.spec?.replicas ?? 1
                const scale = scaleValue[key] ?? replicaCurrent
                return (
                  <tr key={key} className="hover:bg-[var(--surface-hover)]/30">
                    <td className="px-4 py-3 text-[var(--text-primary)] font-medium">{d.metadata.name}</td>
                    <td className="px-4 py-3 text-[var(--text-secondary)]">{d.metadata.namespace || 'default'}</td>
                    <td className="px-4 py-3 text-[var(--text-secondary)]">{d.status?.readyReplicas ?? 0}/{d.status?.replicas ?? replicaCurrent}</td>
                    <td className="px-4 py-3">
                      <input
                        type="number"
                        aria-label="Replica count"
                        min={0}
                        value={scale}
                        onChange={(e) => setScaleValue((prev) => ({ ...prev, [key]: Number(e.target.value) }))}
                        className="w-20 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1"
                      />
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-center justify-center gap-2">
                        <button
                          className="px-2 py-1 rounded-md text-xs bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent)]/30 hover:bg-[var(--accent-soft)] disabled:opacity-50"
                          disabled={acting !== null}
                          onClick={() => void runAction({
                            action: 'rollout_restart_deployment',
                            name: d.metadata.name ?? '',
                            namespace: d.metadata.namespace || 'default',
                          })}
                        >
                          Restart
                        </button>
                        <button
                          className="px-2 py-1 rounded-md text-xs bg-purple-500/20 text-purple-600 border border-[var(--apple-hairline)] hover:bg-purple-500/30 disabled:opacity-50"
                          disabled={acting !== null}
                          onClick={() => void runAction({
                            action: 'scale_deployment',
                            name: d.metadata.name ?? '',
                            namespace: d.metadata.namespace || 'default',
                            replicas: Math.max(0, Number.isFinite(scale) ? scale : replicaCurrent),
                          })}
                        >
                          Scale
                        </button>
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
          {deploymentList.showToggle && (
            <div className="p-3 border-t border-[var(--apple-hairline)]">
              <ExpandableToggle expanded={deploymentList.expanded} hidden={deploymentList.hidden} listId={deploymentList.listId} onToggle={deploymentList.toggle} noun="deployments" />
            </div>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-4">
        <div className="tahoe-glass-card overflow-hidden">
          <div className="px-4 py-3 border-b border-[var(--apple-hairline)] text-lg font-semibold">StatefulSets</div>
          <div className="overflow-x-auto max-h-72 overflow-y-auto">
            <table className="w-full text-sm" aria-label="StatefulSets">
              <thead><tr className="text-[var(--text-muted)] text-xs border-b border-[var(--apple-hairline)]"><th scope="col" className="text-left px-3 py-2">Name</th><th scope="col" className="text-left px-3 py-2">NS</th><th scope="col" className="text-right px-3 py-2">Action</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30" id={statefulsetList.listId}>
                {statefulsetList.shown.map((d) => {
                  const key = `${d.metadata?.namespace || 'default'}/${d.metadata?.name}`
                  return (
                    <tr key={key} className="hover:bg-[var(--surface-hover)]/20">
                      <td className="px-3 py-2 text-[var(--text-primary)]">{d.metadata?.name}</td>
                      <td className="px-3 py-2 text-[var(--text-muted)]">{d.metadata?.namespace || 'default'}</td>
                      <td className="px-3 py-2 text-right">
                        <button type="button" className="text-xs px-2 py-1 rounded bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent)]/30 disabled:opacity-50" disabled={acting !== null} onClick={() => void runAction({ action: 'rollout_restart_stateful_set', name: d.metadata?.name ?? '', namespace: d.metadata?.namespace || 'default' })}>Restart</button>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
            {statefulsetList.showToggle && (
              <div className="p-3 border-t border-[var(--apple-hairline)]">
                <ExpandableToggle expanded={statefulsetList.expanded} hidden={statefulsetList.hidden} listId={statefulsetList.listId} onToggle={statefulsetList.toggle} noun="statefulsets" />
              </div>
            )}
          </div>
        </div>
        <div className="tahoe-glass-card overflow-hidden">
          <div className="px-4 py-3 border-b border-[var(--apple-hairline)] text-lg font-semibold">DaemonSets</div>
          <div className="overflow-x-auto max-h-72 overflow-y-auto">
            <table className="w-full text-sm" aria-label="DaemonSets">
              <thead><tr className="text-[var(--text-muted)] text-xs border-b border-[var(--apple-hairline)]"><th scope="col" className="text-left px-3 py-2">Name</th><th scope="col" className="text-left px-3 py-2">NS</th><th scope="col" className="text-right px-3 py-2">Action</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30" id={daemonsetList.listId}>
                {daemonsetList.shown.map((d) => {
                  const key = `${d.metadata?.namespace || 'default'}/${d.metadata?.name}`
                  return (
                    <tr key={key} className="hover:bg-[var(--surface-hover)]/20">
                      <td className="px-3 py-2 text-[var(--text-primary)]">{d.metadata?.name}</td>
                      <td className="px-3 py-2 text-[var(--text-muted)]">{d.metadata?.namespace || 'default'}</td>
                      <td className="px-3 py-2 text-right">
                        <button type="button" className="text-xs px-2 py-1 rounded bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent)]/30 disabled:opacity-50" disabled={acting !== null} onClick={() => void runAction({ action: 'rollout_restart_daemon_set', name: d.metadata?.name ?? '', namespace: d.metadata?.namespace || 'default' })}>Restart</button>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
            {daemonsetList.showToggle && (
              <div className="p-3 border-t border-[var(--apple-hairline)]">
                <ExpandableToggle expanded={daemonsetList.expanded} hidden={daemonsetList.hidden} listId={daemonsetList.listId} onToggle={daemonsetList.toggle} noun="daemonsets" />
              </div>
            )}
          </div>
        </div>
      </div>

      <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
        <div className="px-4 py-3 border-b border-[var(--apple-hairline)] text-lg font-semibold">Jobs</div>
        <div className="overflow-x-auto max-h-56 overflow-y-auto">
          <table className="w-full text-sm" aria-label="Jobs">
            <thead><tr className="text-[var(--text-muted)] text-xs border-b border-[var(--apple-hairline)]"><th scope="col" className="text-left px-3 py-2">Name</th><th scope="col" className="text-left px-3 py-2">NS</th><th scope="col" className="text-right px-3 py-2">Action</th></tr></thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/30" id={jobList.listId}>
              {jobList.shown.map((j) => {
                const n = j.metadata?.name ?? ''
                const ns = j.metadata?.namespace || 'default'
                const key = `${ns}/${n}`
                return (
                  <tr key={key} className="hover:bg-[var(--surface-hover)]/20">
                    <td className="px-3 py-2 text-[var(--text-primary)] font-mono text-xs">{n}</td>
                    <td className="px-3 py-2 text-[var(--text-muted)]">{ns}</td>
                    <td className="px-3 py-2 text-right">
                      <button type="button" className="text-xs px-2 py-1 rounded bg-rose-500/20 text-rose-700 border border-rose-500/30 disabled:opacity-50" disabled={acting !== null} onClick={() => setConfirmDeleteResource({ payload: { action: 'delete_job', name: n, namespace: ns }, label: `Job ${ns}/${n}` })}>Delete</button>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
          {jobList.showToggle && (
            <div className="p-3 border-t border-[var(--apple-hairline)]">
              <ExpandableToggle expanded={jobList.expanded} hidden={jobList.hidden} listId={jobList.listId} onToggle={jobList.toggle} noun="jobs" />
            </div>
          )}
        </div>
      </div>

      <details className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden group">
        <summary className="px-4 py-3 cursor-pointer text-lg font-semibold text-[var(--text-primary)] select-none">Cluster tools (events, logs, apply, auth, Helm, API explorer)</summary>
        <div className="p-4 space-y-6 border-t border-[var(--apple-hairline)]/40">
          <div className="space-y-2">
            <div className="text-sm font-medium text-[var(--text-secondary)]">Events</div>
            <button type="button" className="text-xs px-3 py-1.5 rounded-lg bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)]" onClick={() => {
              void getK8sEvents({ allNamespaces: namespace === 'all', namespace: namespace === 'all' ? undefined : namespace, context: ctxTrim })
                .then((ev) => {
                  setEventsItems(ev.items ?? [])
                  setEventsText('')
                })
                .catch((e: unknown) => {
                  setEventsItems([])
                  setEventsText(formatUserError(e))
                })
            }}>Load events</button>
            {eventsItems.length > 0 ? (
              <div className="overflow-x-auto rounded border border-[var(--apple-hairline)]">
                <table className="w-full text-xs text-left" aria-label="Kubernetes events">
                  <thead className="text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                    <tr><th scope="col" className="px-2 py-1">Type</th><th scope="col" className="px-2 py-1">Reason</th><th scope="col" className="px-2 py-1">Message</th></tr>
                  </thead>
                  <tbody>
                    {asArray(eventsItems).slice(0, 15).map((item, i) => {
                      const row = asRecord(item) ?? {}
                      const meta = asRecord(row.metadata) ?? {}
                      return (
                        <tr key={String(row.reason ?? row.type ?? i)} className="border-b border-[var(--apple-hairline)]/60">
                          <td className="px-2 py-1 text-[var(--text-muted)]">{String(row.type ?? '—')}</td>
                          <td className="px-2 py-1 text-[var(--text-secondary)]">{String(row.reason ?? '—')}</td>
                          <td className="px-2 py-1 text-[var(--text-muted)]">{String(row.message ?? meta.name ?? '—')}</td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="text-xs text-[var(--text-muted)]">{eventsText || 'Click Load events to fetch cluster events.'}</p>
            )}
            {eventsItems.length > 0 && <JsonInspector data={eventsItems} />}
          </div>
          <div className="space-y-2">
            <div className="text-sm font-medium text-[var(--text-secondary)]">Pod logs</div>
            <div className="flex flex-wrap gap-2 items-end">
              <label className="text-xs text-[var(--text-muted)]">Pod <input className="ml-1 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-[var(--text-primary)]" value={logPod} onChange={(e) => setLogPod(e.target.value)} /></label>
              <label className="text-xs text-[var(--text-muted)]">NS <input className="ml-1 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 w-28 text-[var(--text-primary)]" value={logNs} onChange={(e) => setLogNs(e.target.value)} /></label>
              <label className="text-xs text-[var(--text-muted)]">Container <input className="ml-1 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 w-28 text-[var(--text-primary)]" value={logContainer} onChange={(e) => setLogContainer(e.target.value)} placeholder="opt" /></label>
              <button type="button" className={`text-xs px-3 py-1.5 rounded-lg border ${statusBadgeClasses('ok')} border-[color-mix(in_srgb,var(--machina-status-ok)_40%,transparent)]`} onClick={() => {
                if (!logPod.trim()) { toast.error('Pod name required'); return }
                void getK8sPodLogs({ pod: logPod.trim(), namespace: logNs.trim() || 'default', container: logContainer.trim() || undefined, tailLines: 500, context: ctxTrim })
                  .then((r) => setLogOut(`${r.stdout}\n${r.stderr}`.trim()))
                  .catch((e: unknown) => setLogOut(formatUserError(e)))
              }}>Fetch logs</button>
            </div>
            <pre className="text-xs bg-[var(--apple-surface)]/80 border border-[var(--apple-hairline)] rounded p-2 max-h-56 overflow-auto text-[var(--text-secondary)] whitespace-pre-wrap">{logOut || '—'}</pre>
          </div>
          <div className="space-y-2">
            <div className="text-sm font-medium text-[var(--text-secondary)]">kubectl apply (YAML)</div>
            <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]"><input type="checkbox" checked={applyDry} onChange={(e) => setApplyDry(e.target.checked)} /> Server dry-run</label>
            <textarea aria-label="kubectl apply YAML" className="w-full min-h-[120px] bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded p-2 text-xs font-mono text-[var(--text-primary)]" value={applyYaml} onChange={(e) => setApplyYaml(e.target.value)} placeholder="apiVersion: v1&#10;kind: ConfigMap&#10;..." />
            <button type="button" className={`text-xs px-3 py-1.5 rounded-lg border ${statusBadgeClasses('warn')} border-[color-mix(in_srgb,var(--machina-status-warn)_40%,transparent)]`} onClick={() => {
              void postK8sApply(applyYaml, applyDry, ctxTrim).then((r) => setApplyOut(r)).catch((e: unknown) => setApplyOut(formatUserError(e)))
            }}>Apply</button>
            {applyOut != null && (typeof applyOut === 'object' ? <JsonInspector data={applyOut} /> : (
              <p className="text-xs text-[var(--text-secondary)] whitespace-pre-wrap">{String(applyOut)}</p>
            ))}
          </div>
          <div className="space-y-2">
            <div className="text-sm font-medium text-[var(--text-secondary)]">kubectl auth can-i</div>
            <div className="flex flex-wrap gap-2 items-end">
              <input aria-label="kubectl verb" className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs w-24" value={caniVerb} onChange={(e) => setCaniVerb(e.target.value)} placeholder="verb" />
              <input aria-label="kubectl resource" className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs flex-1 min-w-[8rem]" value={caniRes} onChange={(e) => setCaniRes(e.target.value)} placeholder="resource" />
              <input aria-label="kubectl namespace" className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs w-28" value={caniNs} onChange={(e) => setCaniNs(e.target.value)} placeholder="-n (opt)" />
              <button type="button" className="text-xs px-3 py-1.5 rounded-lg bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)]" onClick={() => {
                void postK8sAuthCanI({ verb: caniVerb.trim(), resource: caniRes.trim(), namespace: caniNs.trim() || undefined, context: ctxTrim }).then((r) => setCaniOut(r.stdout.trim() || JSON.stringify(r))).catch((e: unknown) => setCaniOut(formatUserError(e)))
              }}>Check</button>
            </div>
            <div className={`rounded-lg border px-3 py-2 text-sm ${
              caniOut === 'yes' ? statusBadgeClasses('ok')
                : caniOut === 'no' ? statusBadgeClasses('error')
                  : 'border-[var(--apple-hairline)] bg-[var(--apple-surface)] text-[var(--text-secondary)]'
            }`}>
              {caniOut ? (
                <>
                  <span className="font-medium">{caniOut === 'yes' || caniOut === 'no' ? caniOut.toUpperCase() : 'Result'}</span>
                  {caniOut !== 'yes' && caniOut !== 'no' && (
                    <p className="text-xs mt-1 whitespace-pre-wrap font-mono">{caniOut}</p>
                  )}
                </>
              ) : (
                <span className="text-[var(--text-muted)]">Run a check to see yes/no</span>
              )}
            </div>
          </div>
          <div className="space-y-2">
            <div className="text-sm font-medium text-[var(--text-secondary)]">Helm releases</div>
            <button type="button" className="text-xs px-3 py-1.5 rounded-lg bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)]" onClick={() => {
              void getK8sHelmReleases('*', ctxTrim).then((h) => setHelmJson(h)).catch((e: unknown) => setHelmJson(formatUserError(e)))
            }}>helm list -A (JSON)</button>
            {helmJson != null && (typeof helmJson === 'object' ? <JsonInspector data={helmJson} /> : (
              <p className="text-xs text-[var(--text-secondary)] whitespace-pre-wrap">{String(helmJson)}</p>
            ))}
          </div>
          <div className="space-y-2">
            <div className="text-sm font-medium text-[var(--text-secondary)]">API list explorer</div>
            <div className="flex flex-wrap gap-2">
              <select aria-label="Resource kind" className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs" value={explorerKind} onChange={(e) => setExplorerKind(e.target.value)}>
                <option value="ingresses">Ingresses</option>
                <option value="cronjobs">CronJobs</option>
                <option value="pvcs">PVCs</option>
                <option value="pvs">PVs</option>
                <option value="storageclasses">StorageClasses</option>
              </select>
              <button type="button" className="text-xs px-3 py-1.5 rounded-lg bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)]" onClick={() => {
                const c = ctxTrim
                const ns = nsValue
                const p = (() => {
                  if (explorerKind === 'ingresses') return getK8sIngresses(ns, c)
                  if (explorerKind === 'cronjobs') return getK8sCronJobs(ns, c)
                  if (explorerKind === 'pvcs') return getK8sPersistentVolumeClaims(ns, c)
                  if (explorerKind === 'pvs') return getK8sPersistentVolumes(c)
                  return getK8sStorageClasses(c)
                })()
                void p.then((x) => setExplorerJson(x)).catch((e: unknown) => setExplorerJson(formatUserError(e)))
              }}>Fetch</button>
            </div>
            {explorerJson != null && (typeof explorerJson === 'object' ? <JsonInspector data={explorerJson} /> : (
              <p className="text-xs text-[var(--text-secondary)] whitespace-pre-wrap">{String(explorerJson)}</p>
            ))}
          </div>
        </div>
      </details>

      <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
        <div className="px-6 py-4 border-b border-[var(--apple-hairline)] space-y-1">
          <h2 className="text-lg font-semibold">KubeVirt VirtualMachines</h2>
          <p className="text-xs text-[var(--text-muted)]">
            Merged with <code className="bg-[var(--apple-surface)] px-1 rounded">VirtualMachineInstance</code> for guest IP, launcher <code className="text-xs bg-[var(--apple-surface)] px-1 rounded">podIP</code>, and node <code className="text-xs bg-[var(--apple-surface)] px-1 rounded">InternalIP</code>.
            <strong className="text-[var(--text-muted)]">Expose / SSH</strong> opens a planner for Service type, ports, <code className="text-xs bg-[var(--apple-surface)] px-1 rounded">virtctl expose vm …</code>, and copyable SSH once a matching Service exists.
            In-browser VNC uses machina&apos;s WebSocket proxy or run <code className="text-xs bg-[var(--apple-surface)] px-1 rounded">virtctl vnc</code> locally.
            Libvirt VMs stay under <strong className="text-[var(--text-muted)]">VMs</strong>.
          </p>
          {kubevirtListError && (
            <div className="mt-2">
              <K8sConnectionErrorBanner
                title="Could not load KubeVirt VM summary"
                message={kubevirtListError}
              />
            </div>
          )}
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              type="button"
              data-testid="kubevirt-create-yaml"
              disabled={kubevirtVmBusy === 'create'}
              className="inline-flex items-center gap-1.5 rounded-lg border border-teal-500/40 px-3 py-1.5 text-xs text-teal-700 hover:bg-teal-500/10 disabled:opacity-50"
              onClick={() => setShowKubevirtCreate((v) => !v)}
            >
              {showKubevirtCreate ? 'Hide create YAML' : 'Create VM (YAML)'}
            </button>
            <button
              type="button"
              data-testid="kubevirt-load-crs"
              disabled={kubevirtCrBusy}
              className="inline-flex items-center gap-1.5 rounded-lg border border-[var(--accent)]/40 px-3 py-1.5 text-xs text-[var(--link)] hover:bg-[var(--accent-soft)] disabled:opacity-50"
              onClick={() => void loadKubevirtCrs()}
            >
              {kubevirtCrBusy ? 'Loading CRs…' : 'Load VirtualMachine CRs'}
            </button>
            {kubevirtVmCrs.length > 0 && (
              <span className="text-xs text-[var(--text-muted)]" data-testid="kubevirt-cr-count">{kubevirtVmCrs.length} CR(s) from /k8s/kubevirt/virtualmachines</span>
            )}
          </div>
          {showKubevirtCreate && (
            <div className="mt-3 space-y-2">
              <textarea
                aria-label="KubeVirt VirtualMachine YAML"
                value={kubevirtCreateYaml}
                onChange={(e) => setKubevirtCreateYaml(e.target.value)}
                rows={12}
                className="w-full rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/80 p-3 font-mono text-xs text-[var(--text-primary)]"
                spellCheck={false}
              />
              <button
                type="button"
                className="btn-primary text-xs"
                disabled={kubevirtVmBusy === 'create'}
                onClick={() => void applyKubevirtYaml()}
              >
                {kubevirtVmBusy === 'create' ? 'Applying…' : 'kubectl apply manifest'}
              </button>
            </div>
          )}
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-sm min-w-[72rem]" aria-label="KubeVirt VirtualMachines">
            <thead>
              <tr className="border-b border-[var(--apple-hairline)] text-[var(--text-muted)] text-xs uppercase tracking-wider">
                <th scope="col" className="text-left px-4 py-3">Name</th>
                <th scope="col" className="text-left px-4 py-3">Namespace</th>
                <th scope="col" className="text-left px-4 py-3">Run / ready</th>
                <th scope="col" className="text-left px-4 py-3">VM status</th>
                <th scope="col" className="text-left px-4 py-3">VMI</th>
                <th scope="col" className="text-left px-4 py-3">Guest IP</th>
                <th scope="col" className="text-left px-4 py-3">Pod IP</th>
                <th scope="col" className="text-left px-4 py-3">Node</th>
                <th scope="col" className="text-left px-4 py-3">Node IP</th>
                <th scope="col" className="text-center px-4 py-3">Console / VNC / live</th>
                <th scope="col" className="text-center px-4 py-3">Lifecycle</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/30" id={kubevirtRowList.listId}>
              {kubevirtRowList.shown.map((v) => {
                const key = `${v.namespace}/${v.name}`
                const runState =
                  v.spec_running === true ? 'on'
                  : v.spec_running === false ? 'off'
                  : v.spec_run_strategy === 'Halted' ? 'off'
                  : v.spec_run_strategy === 'Always' ? 'on'
                  : v.spec_run_strategy ?? '—'
                const rr = `${runState} / ${v.vm_ready === true ? 'yes' : v.vm_ready === false ? 'no' : '—'}`
                return (
                  <tr key={key} className="hover:bg-[var(--surface-hover)]/30">
                    <td className="px-4 py-3 text-[var(--text-primary)] font-medium">{v.name}</td>
                    <td className="px-4 py-3 text-[var(--text-secondary)]">{v.namespace}</td>
                    <td className="px-4 py-3 text-[var(--text-secondary)] font-mono text-xs">{rr}</td>
                    <td className="px-4 py-3 text-[var(--text-secondary)]">{v.vm_printable_status ?? '—'}</td>
                    <td className="px-4 py-3 text-[var(--text-muted)]">{v.vmi_phase ?? '—'}</td>
                    <td className="px-4 py-3 text-[var(--text-muted)] font-mono text-xs">{v.guest_ip ?? '—'}</td>
                    <td className="px-4 py-3 text-[var(--text-muted)] font-mono text-xs">{v.pod_ip ?? '—'}</td>
                    <td className="px-4 py-3 text-[var(--text-muted)] font-mono text-xs">{v.node_name ?? '—'}</td>
                    <td className="px-4 py-3 text-[var(--text-muted)] font-mono text-xs">{v.node_internal_ip ?? '—'}</td>
                    <td className="px-4 py-3">
                      <div className="flex flex-wrap items-center justify-center gap-1">
                        <button
                          type="button"
                          title={v.virtctl_console}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-[var(--surface-hover)]/80 text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)]"
                          onClick={() => copyText('virtctl console', v.virtctl_console)}
                        >
                          <Terminal className="w-3.5 h-3.5" /> Console
                        </button>
                        <button
                          type="button"
                          title={v.virtctl_vnc}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-violet-500/15 text-[var(--link)] border border-violet-500/35 hover:bg-[var(--accent-hover)]/25"
                          onClick={() => copyText('virtctl vnc', v.virtctl_vnc)}
                        >
                          <Monitor className="w-3.5 h-3.5" /> VNC
                        </button>
                        <button
                          type="button"
                          title={v.virtctl_vnc_socks}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-[var(--surface-hover)]/80 text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)]"
                          onClick={() => copyText('virtctl vnc --proxy-only', v.virtctl_vnc_socks)}
                        >
                          <Copy className="w-3.5 h-3.5" /> SOCKS
                        </button>
                        <button
                          type="button"
                          title={v.vnc_subresource_path}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-[var(--surface-hover)]/80 text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)]"
                          onClick={() => copyText('VNC API path', v.vnc_subresource_path)}
                        >
                          <Copy className="w-3.5 h-3.5" /> API path
                        </button>
                        <button
                          type="button"
                          title="Open noVNC in this browser (machina proxies to the cluster)"
                          className={`inline-flex items-center gap-1 hover:bg-[color-mix(in_srgb,var(--machina-status-ok)_30%,transparent)] ${statusPillClasses('ok')}`}
                          onClick={() => setLiveKubeVirt({ kind: 'vnc', namespace: v.namespace, name: v.name })}
                        >
                          <ExternalLink className="w-3.5 h-3.5" /> Live VNC
                        </button>
                        <button
                          type="button"
                          title="Serial console in this browser"
                          className={`inline-flex items-center gap-1 text-xs hover:bg-[color-mix(in_srgb,var(--machina-status-info)_30%,transparent)] ${statusPillClasses('info')}`}
                          onClick={() => setLiveKubeVirt({ kind: 'console', namespace: v.namespace, name: v.name })}
                        >
                          <Terminal className="w-3.5 h-3.5" /> Live console
                        </button>
                        <button
                          type="button"
                          title="Plan Service / NodePort SSH and copy virtctl expose"
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-teal-500/20 text-teal-800 border border-teal-500/35 hover:bg-teal-500/30"
                          onClick={() =>
                            setExposeVm({
                              name: v.name,
                              namespace: v.namespace,
                              nodeInternalIp: v.node_internal_ip,
                            })}
                        >
                          <Network className="w-3.5 h-3.5" /> Expose / SSH
                        </button>
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex flex-wrap items-center justify-center gap-1">
                        <button
                          type="button"
                          title="Start VM"
                          disabled={kubevirtVmBusy != null}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-emerald-500/15 text-emerald-700 border border-emerald-500/35 hover:bg-emerald-500/25 disabled:opacity-50"
                          onClick={() => void runKubevirtLifecycle(v.namespace, v.name, 'start')}
                        >
                          <Play className="w-3.5 h-3.5" /> Start
                        </button>
                        <button
                          type="button"
                          title="Stop VM"
                          disabled={kubevirtVmBusy != null}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-[var(--surface-hover)]/80 text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)] disabled:opacity-50"
                          onClick={() => void runKubevirtLifecycle(v.namespace, v.name, 'stop')}
                        >
                          <Square className="w-3.5 h-3.5" /> Stop
                        </button>
                        <button
                          type="button"
                          title="Restart VM"
                          disabled={kubevirtVmBusy != null}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-amber-500/15 text-amber-700 border border-amber-500/35 hover:bg-amber-500/25 disabled:opacity-50"
                          onClick={() => void runKubevirtLifecycle(v.namespace, v.name, 'restart')}
                        >
                          <RotateCw className="w-3.5 h-3.5" /> Restart
                        </button>
                        <button
                          type="button"
                          title="Delete VirtualMachine CR"
                          disabled={kubevirtVmBusy != null}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-red-500/15 text-red-700 border border-red-500/35 hover:bg-red-500/25 disabled:opacity-50"
                          onClick={() => void deleteKubevirtVm(v.namespace, v.name)}
                        >
                          <Trash2 className="w-3.5 h-3.5" /> Delete
                        </button>
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        {kubevirtRowList.showToggle && (
          <div className="p-3 border-t border-[var(--apple-hairline)]">
            <ExpandableToggle expanded={kubevirtRowList.expanded} hidden={kubevirtRowList.hidden} listId={kubevirtRowList.listId} onToggle={kubevirtRowList.toggle} noun="VMs" />
          </div>
        )}
        </div>
        {kubevirtRows.length === 0 && !kubevirtListError && (
          <div className="p-6 text-center text-[var(--text-muted)] text-sm">No KubeVirt VirtualMachines in scope (or CRD not installed).</div>
        )}
        {kubevirtVmCrs.length > 0 && (
          <div className="border-t border-[var(--apple-hairline)] p-4">
            <JsonInspector data={kubevirtVmCrs} />
          </div>
        )}
      </div>

      <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
        <div className="px-6 py-4 border-b border-[var(--apple-hairline)]">
          <h2 className="text-lg font-semibold">Pods</h2>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-sm" aria-label="Pods">
            <thead>
              <tr className="border-b border-[var(--apple-hairline)] text-[var(--text-muted)] text-xs uppercase tracking-wider">
                <th scope="col" className="text-left px-4 py-3">Name</th>
                <th scope="col" className="text-left px-4 py-3">Namespace</th>
                <th scope="col" className="text-left px-4 py-3">Phase</th>
                <th scope="col" className="text-left px-4 py-3">Pod IP</th>
                <th scope="col" className="text-left px-4 py-3">Node</th>
                <th scope="col" className="text-left px-4 py-3">Node IP</th>
                <th scope="col" className="text-center px-4 py-3">Action</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/30" id={podList.listId}>
              {pods.length === 0 && (
                <tr>
                  <td colSpan={7} className="px-4 py-8">
                    <EmptyState
                      className="py-6 border-0 bg-transparent"
                      title="No pods"
                      description="Pods appear when controllers create them or you run standalone Pod manifests."
                    />
                  </td>
                </tr>
              )}
              {podList.shown.map((p) => (
                <tr key={`${p.metadata.namespace || 'default'}/${p.metadata?.name ?? ''}`} className="hover:bg-[var(--surface-hover)]/30">
                  <td className="px-4 py-3 text-[var(--text-primary)] font-medium">{p.metadata?.name ?? '—'}</td>
                  <td className="px-4 py-3 text-[var(--text-secondary)]">{p.metadata.namespace || 'default'}</td>
                  <td className="px-4 py-3 text-[var(--text-secondary)]">{p.status?.phase || 'unknown'}</td>
                  <td className="px-4 py-3 text-[var(--text-muted)] font-mono text-xs">{p.status?.podIP || '—'}</td>
                  <td className="px-4 py-3 text-[var(--text-muted)] font-mono text-xs">{p.spec?.nodeName ?? '—'}</td>
                  <td className="px-4 py-3 text-[var(--text-muted)] font-mono text-xs">{p.status?.hostIP ?? '—'}</td>
                  <td className="px-4 py-3 text-center">
                    <button
                      className={`text-xs disabled:opacity-50 hover:bg-[color-mix(in_srgb,var(--machina-status-error)_30%,transparent)] ${statusPillClasses('error')}`}
                      disabled={acting !== null}
                      onClick={() => setConfirmDeleteResource({
                        payload: {
                          action: 'delete_pod',
                          name: p.metadata?.name ?? '',
                          namespace: p.metadata.namespace || 'default',
                        },
                        label: `Pod ${p.metadata.namespace || 'default'}/${p.metadata?.name ?? ''}`,
                      })}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        {podList.showToggle && (
          <div className="p-3 border-t border-[var(--apple-hairline)]">
            <ExpandableToggle expanded={podList.expanded} hidden={podList.hidden} listId={podList.listId} onToggle={podList.toggle} noun="pods" />
          </div>
        )}
        </div>
      </div>

      {exposeVm && (
        <KubeVirtExposeServiceModal
          vm={exposeVm}
          services={services}
          onClose={() => setExposeVm(null)}
          onCopy={copyText}
        />
      )}

      {liveKubeVirt && (
        <div className="fixed inset-0 z-[80] flex flex-col bg-[var(--apple-surface)]/95 backdrop-blur-sm" role="dialog" aria-modal="true" aria-label="KubeVirt console">
          <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--apple-hairline)] bg-[var(--apple-surface)] shrink-0">
            <span className="text-sm text-[var(--text-primary)]">
              KubeVirt {liveKubeVirt.kind === 'vnc' ? 'VNC' : 'serial console'} — {liveKubeVirt.namespace}/{liveKubeVirt.name}
            </span>
            <button
              type="button"
              className="px-3 py-1.5 rounded-lg text-sm bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] text-[var(--text-primary)]"
              onClick={() => setLiveKubeVirt(null)}
            >
              Close
            </button>
          </div>
          <div className="flex-1 min-h-0 overflow-hidden p-2">
            {liveKubeVirt.kind === 'vnc' ? (
              <VNCViewer
                vmName={liveKubeVirt.name}
                kubeVirtNamespace={liveKubeVirt.namespace}
                port={1}
                hideInstallerHint
                {...fillViewportVncProps}
              />
            ) : (
              <KubeVirtSerialConsole namespace={liveKubeVirt.namespace} vmName={liveKubeVirt.name} />
            )}
          </div>
        </div>
      )}
      <ConfirmDialog
        open={deleteKubevirtTarget !== null}
        title="Delete KubeVirt VM"
        message={deleteKubevirtTarget ? `Delete VirtualMachine ${deleteKubevirtTarget.namespace}/${deleteKubevirtTarget.name}? This cannot be undone.` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setDeleteKubevirtTarget(null)}
        onConfirm={async () => {
          const t = deleteKubevirtTarget
          setDeleteKubevirtTarget(null)
          if (!t) return
          const key = `delete:${t.namespace}/${t.name}`
          setKubevirtVmBusy(key)
          try {
            await deleteK8sKubevirtVm(t.namespace, t.name, ctxTrim || undefined)
            toast.success(`Deleted ${t.namespace}/${t.name}`)
            await load(true)
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          } finally {
            setKubevirtVmBusy(null)
          }
        }}
      />
      <ConfirmDialog
        open={confirmDeleteResource !== null}
        title="Delete resource"
        message={confirmDeleteResource ? `Delete ${confirmDeleteResource.label}? This cannot be undone.` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDeleteResource(null)}
        onConfirm={() => {
          const target = confirmDeleteResource
          setConfirmDeleteResource(null)
          if (target) void runAction(target.payload)
        }}
      />
    </PageLayout>
  )
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiDelete, apiPost, readJsonArray, readJsonObject, readJsonItemsList } from './client'

const API = '/api/v1'

export interface K8sOverview {
  version: string
  nodes: number
  ready_nodes: number
  namespaces: number
  pods: number
  deployments: number
  services: number
  /** Heuristic: k3s, rke2, eks, gke, aks, ack, tke, cce, minikube, kind, generic, unknown */
  distribution?: string
  distribution_hints?: string[]
  extra_resource_counts?: Record<string, number>
}

interface K8sHostSignals {
  k3s_config_present: boolean
  k3s_data_dir_present: boolean
  rke2_config_present: boolean
  rke2_data_dir_present: boolean
  k3s_systemd: string
  k3s_agent_systemd: string
  rke2_server_systemd: string
  rke2_agent_systemd: string
  k3s_binary_version: string | null
  rke2_binary_version: string | null
  helm_version: string | null
  crictl_version: string | null
}

export interface K8sEnvironment {
  kubectl_on_path: boolean
  kubectl_client_version: string | null
  kubectl_server_reachable: boolean
  kubeconfig_hint: string | null
  kubeconfig_from_env: boolean
  /** When set, machina-daemon injects this file as `--kubeconfig` for kubectl (auto-detected). */
  kubeconfig_auto_selected?: string | null
  current_context: string | null
  cluster_distribution: string
  cluster_distribution_hints: string[]
  host: K8sHostSignals
  snippets: Record<string, string>
}

interface K8sNodeTaint {
  key: string
  value?: string | null
  effect: string
}

export interface K8sNodeInfo {
  name: string
  roles: string[]
  ready: boolean
  kubelet_version: string
  os_image: string
  kernel_version: string
  container_runtime: string
  architecture: string
  capacity: Record<string, string>
  allocatable: Record<string, string>
  labels: Record<string, string>
  metadata_uid?: string | null
  system_uuid?: string | null
  provider_id?: string | null
  /** From node-role labels: control_plane | worker | mixed */
  plane?: string
  cpu_capacity_millicores?: number | null
  cpu_allocatable_millicores?: number | null
  memory_capacity_bytes?: number | null
  memory_allocatable_bytes?: number | null
  topology_hints?: Record<string, string>
  unschedulable?: boolean
  taints?: K8sNodeTaint[]
  memory_pressure?: boolean
  disk_pressure?: boolean
  pid_pressure?: boolean
  network_unavailable?: boolean
  /** Set on cluster-inventory when API gitVersion parses; compares kubelet vs apiserver major.minor */
  kubelet_minor_matches_apiserver?: boolean | null
  /** Daemon host DMI UUID matches Node systemUUID */
  daemon_matches_this_machine?: boolean | null
}

interface K8sWebhookSummaryRow {
  name: string
  webhook_rules_count: number
}

interface K8sAddonDaemonSetRow {
  namespace: string
  name: string
  primary_image: string
}

export interface K8sExtendedClusterInsights {
  validating_webhooks?: K8sWebhookSummaryRow[]
  mutating_webhooks?: K8sWebhookSummaryRow[]
  addon_daemonsets?: K8sAddonDaemonSetRow[]
  gpu_allocatable_cluster_totals?: Record<string, string>
  daemon_machine_product_uuid?: string | null
  etcd_member_list_stdout?: string | null
  etcd_member_list_stderr?: string | null
  operator_alerts?: string[]
}

/** Rolled-up cpu/memory from Node capacity (kubernetes-style inventory). */
export interface K8sPlaneRollup {
  node_count: number
  ready_node_count: number
  cpu_capacity_millicores: number
  cpu_allocatable_millicores: number
  memory_capacity_bytes: number
  memory_allocatable_bytes: number
}

interface K8sTaintPlaneRollup {
  nodes_total: number
  nodes_with_scheduling_taints: number
}

interface K8sCpStackPod {
  component: string
  namespace: string
  name: string
  node_name?: string | null
  node_plane?: string | null
  phase: string
  container_images?: string[]
  inferred_k8s_semver_tag?: string | null
}

interface K8sUpgradeInsights {
  disclaimer?: string
  inferred_etcd_member_pods_running?: number
  max_kubelet_minor_lag_behind_apiserver?: number | null
  nodes_kubelet_newer_than_apiserver?: string[]
  /** Same API major; kubelet minor lag > policy */
  nodes_kubelet_minor_lag_exceeds_policy?: string[]
  /** Kubelet major older than API server */
  nodes_kubelet_major_behind_apiserver?: string[]
  kube_apiserver_pod_image_minors?: string[]
  etcd_pod_image_minors?: string[]
  upgrade_warnings?: string[]
  suggested_upgrade_order?: string[]
}

export interface K8sClusterInventoryResponse {
  collected_at_rfc3339: string
  disclaimer: string
  totals_all_nodes: K8sPlaneRollup
  by_plane: Record<string, K8sPlaneRollup>
  combined_control_plane_and_mixed: K8sPlaneRollup
  combined_worker_dataplane_and_mixed: K8sPlaneRollup
  nodes: K8sNodeInfo[]
  apiserver_git_version?: string
  apiserver_major_minor?: string
  cluster_livez_ok?: boolean
  cluster_readyz_ok?: boolean
  cluster_health_notes?: string[]
  nodes_with_kubelet_minor_skew?: number
  topology_nodes_by_zone?: Record<string, number>
  topology_nodes_by_region?: Record<string, number>
  taints_by_plane?: Record<string, K8sTaintPlaneRollup>
  running_pods_by_plane?: Record<string, number>
  running_pods_total?: number
  running_pods_without_node?: number
  pending_pods_unscheduled?: number
  etcd_placement_pods?: K8sCpStackPod[]
  control_plane_stack_pods?: K8sCpStackPod[]
  upgrade_insights?: K8sUpgradeInsights
  extended?: K8sExtendedClusterInsights
}

interface K8sObjectMeta {
  name?: string
  namespace?: string
}

/** Standard Kubernetes list item with nested metadata. */
export interface K8sMetadataName {
  metadata?: { name?: string; namespace?: string }
}

export interface K8sDeployment {
  metadata: K8sObjectMeta
  spec?: { replicas?: number }
  status?: { readyReplicas?: number; replicas?: number; availableReplicas?: number }
}

export interface K8sPod {
  metadata: K8sObjectMeta
  spec?: { nodeName?: string }
  status?: { phase?: string; podIP?: string; hostIP?: string }
}

interface K8sServicePort {
  name?: string
  port?: number
  /** Service target; may be number or named port string from pod spec. */
  targetPort?: number | string
  nodePort?: number
  protocol?: string
}

export interface K8sService {
  metadata?: K8sObjectMeta & { labels?: Record<string, string> }
  spec?: {
    type?: string
    clusterIP?: string
    /** Selectors for VM-expose Services often include `kubevirt.io/vmName` or `kubevirt.io/domain`. */
    selector?: Record<string, string>
    ports?: K8sServicePort[]
  }
  status?: {
    loadBalancer?: {
      ingress?: Array<{ ip?: string; hostname?: string }>
    }
  }
}

/** KubeVirt `VirtualMachine` (`virtualmachines.kubevirt.io`). */
export interface K8sKubeVirtVM {
  metadata?: { name?: string; namespace?: string }
  spec?: { running?: boolean }
  status?: { printableStatus?: string; ready?: boolean; created?: boolean }
}

/** VM + VMI merge from `GET /k8s/kubevirt/vm-summary`. */
export interface KubeVirtVmSummaryRow {
  name: string
  namespace: string
  spec_running?: boolean | null
  spec_run_strategy?: string | null
  vm_printable_status?: string | null
  vm_ready?: boolean | null
  guest_ip?: string | null
  pod_ip?: string | null
  vmi_phase?: string | null
  node_name?: string | null
  node_internal_ip?: string | null
  virtctl_console: string
  virtctl_vnc: string
  virtctl_vnc_socks: string
  vnc_subresource_path: string
}

type K8sAction =
  | 'node_cordon'
  | 'node_uncordon'
  | 'node_drain'
  | 'rollout_restart_deployment'
  | 'rollout_restart_stateful_set'
  | 'rollout_restart_daemon_set'
  | 'delete_pod'
  | 'delete_job'
  | 'scale_deployment'
  | 'scale_stateful_set'

interface K8sActionRequest {
  action: K8sAction
  name: string
  namespace?: string
  replicas?: number
  context?: string
}

export interface K8sActionResult {
  command: string
  stdout: string
  stderr: string
  exit_code: number
  ok: boolean
}

function withK8sContext(base: string, context?: string): string {
  const c = context?.trim()
  if (!c) return base
  return `${base}${base.includes('?') ? '&' : '?'}context=${encodeURIComponent(c)}`
}

export const getK8sOverview = (context?: string) =>
  readJsonObject<K8sOverview>(withK8sContext(`${API}/k8s/overview`, context))

export const getK8sEnvironment = () => readJsonObject<K8sEnvironment>(`${API}/k8s/environment`)

export const getK8sContexts = () =>
  readJsonObject<{ contexts?: unknown }>(`${API}/k8s/contexts`).then((o) => ({
    contexts: Array.isArray(o.contexts)
      ? (o.contexts as unknown[]).filter((x): x is string => typeof x === 'string')
      : [],
  }))

export const getK8sNodes = (context?: string) =>
  readJsonArray<K8sNodeInfo>(withK8sContext(`${API}/k8s/nodes`, context))

export const getK8sClusterInventory = (context?: string) =>
  readJsonObject<K8sClusterInventoryResponse>(
    withK8sContext(`${API}/k8s/cluster-inventory`, context),
  )

interface K8sTopRow {
  name: string
  cpu: string
  cpu_percent: string
  memory: string
  memory_percent: string
}

export interface K8sMetricsResponse {
  metrics_available: boolean
  metrics_server_hint?: string
  nodes_top: K8sTopRow[]
  pods_top: K8sTopRow[]
  nodes_error?: string | null
  pods_error?: string | null
}

export const getK8sMetrics = (context?: string) =>
  readJsonObject<K8sMetricsResponse>(withK8sContext(`${API}/k8s/metrics`, context))

export interface K8sClusterInventoryHistoryResponse {
  path: string
  entries: Record<string, unknown>[]
}

export const getK8sClusterInventoryHistory = (limit?: number, context?: string) => {
  const q = limit != null ? `?limit=${encodeURIComponent(String(limit))}` : ''
  let url = `${API}/k8s/cluster-inventory/history${q}`
  url = withK8sContext(url, context)
  return readJsonObject<K8sClusterInventoryHistoryResponse>(url)
}

/** Single JSON file for audits: overview + cluster inventory + environment (client-side merge). */
export async function buildK8sAuditBundleJson(context?: string): Promise<string> {
  const [inventory, overview, environment] = await Promise.all([
    getK8sClusterInventory(context),
    getK8sOverview(context),
    getK8sEnvironment(),
  ])
  return JSON.stringify(
    {
      exported_at_rfc3339: new Date().toISOString(),
      cluster_inventory: inventory,
      k8s_overview: overview,
      k8s_environment: environment,
    },
    null,
    2,
  )
}

export function downloadTextAsFile(filename: string, text: string, mime: string) {
  const blob = new Blob([text], { type: mime })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

export const getK8sNamespaces = (context?: string) =>
  readJsonItemsList<K8sMetadataName>(withK8sContext(`${API}/k8s/namespaces`, context))

export const getK8sPods = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sPod>(
    withK8sContext(
      namespace ? `${API}/k8s/pods?namespace=${encodeURIComponent(namespace)}` : `${API}/k8s/pods?all_namespaces=true`,
      context,
    ),
  )

export const getK8sDeployments = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sDeployment>(
    withK8sContext(
      namespace
        ? `${API}/k8s/deployments?namespace=${encodeURIComponent(namespace)}`
        : `${API}/k8s/deployments?all_namespaces=true`,
      context,
    ),
  )

export const getK8sServices = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sService>(
    withK8sContext(
      namespace ? `${API}/k8s/services?namespace=${encodeURIComponent(namespace)}` : `${API}/k8s/services?all_namespaces=true`,
      context,
    ),
  )

export const getK8sStatefulSets = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sDeployment>(
    withK8sContext(
      namespace
        ? `${API}/k8s/statefulsets?namespace=${encodeURIComponent(namespace)}`
        : `${API}/k8s/statefulsets?all_namespaces=true`,
      context,
    ),
  )

export const getK8sDaemonSets = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sDeployment>(
    withK8sContext(
      namespace ? `${API}/k8s/daemonsets?namespace=${encodeURIComponent(namespace)}` : `${API}/k8s/daemonsets?all_namespaces=true`,
      context,
    ),
  )

export const getK8sJobs = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sMetadataName>(
    withK8sContext(
      namespace ? `${API}/k8s/jobs?namespace=${encodeURIComponent(namespace)}` : `${API}/k8s/jobs?all_namespaces=true`,
      context,
    ),
  )

export const getK8sCronJobs = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sMetadataName>(
    withK8sContext(
      namespace ? `${API}/k8s/cronjobs?namespace=${encodeURIComponent(namespace)}` : `${API}/k8s/cronjobs?all_namespaces=true`,
      context,
    ),
  )

export const getK8sIngresses = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sMetadataName>(
    withK8sContext(
      namespace ? `${API}/k8s/ingresses?namespace=${encodeURIComponent(namespace)}` : `${API}/k8s/ingresses?all_namespaces=true`,
      context,
    ),
  )

export const getK8sPersistentVolumeClaims = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sMetadataName>(
    withK8sContext(
      namespace
        ? `${API}/k8s/persistentvolumeclaims?namespace=${encodeURIComponent(namespace)}`
        : `${API}/k8s/persistentvolumeclaims?all_namespaces=true`,
      context,
    ),
  )

export const getK8sPersistentVolumes = (context?: string) =>
  readJsonItemsList<unknown>(withK8sContext(`${API}/k8s/persistentvolumes`, context))

export const getK8sStorageClasses = (context?: string) =>
  readJsonItemsList<unknown>(withK8sContext(`${API}/k8s/storageclasses`, context))

export const getK8sEvents = (opts: { namespace?: string; allNamespaces?: boolean; context?: string }) => {
  const q: string[] = []
  if (opts.allNamespaces) q.push('all_namespaces=true')
  else if (opts.namespace) q.push(`namespace=${encodeURIComponent(opts.namespace)}`)
  else q.push('namespace=default')
  let url = `${API}/k8s/events?${q.join('&')}`
  url = withK8sContext(url, opts.context)
  return readJsonItemsList<unknown>(url)
}

export const getK8sPodLogs = (opts: {
  pod: string
  namespace?: string
  container?: string
  tailLines?: number
  previous?: boolean
  context?: string
}) => {
  const q = new URLSearchParams()
  q.set('pod', opts.pod)
  if (opts.namespace) q.set('namespace', opts.namespace)
  if (opts.container) q.set('container', opts.container)
  if (opts.tailLines != null) q.set('tail_lines', String(opts.tailLines))
  if (opts.previous) q.set('previous', 'true')
  let url = `${API}/k8s/logs?${q.toString()}`
  url = withK8sContext(url, opts.context)
  return readJsonObject<K8sActionResult>(url)
}

export const postK8sApply = (manifest: string, dryRun?: boolean, context?: string) =>
  apiPost<K8sActionResult>(`${API}/k8s/apply`, { manifest, dry_run: dryRun, context })

export const postK8sAuthCanI = (body: {
  verb: string
  resource: string
  namespace?: string
  resource_name?: string
  context?: string
}) => apiPost<K8sActionResult>(`${API}/k8s/auth-can-i`, body)

export const getK8sHelmReleases = (namespace?: string, context?: string) => {
  const q = new URLSearchParams()
  if (namespace) q.set('namespace', namespace === '*' ? 'all' : namespace)
  let url = `${API}/k8s/helm/releases${q.toString() ? `?${q}` : ''}`
  url = withK8sContext(url, context)
  return readJsonObject<Record<string, unknown>>(url)
}

export const getK8sKubevirtVirtualMachines = (namespace?: string, context?: string) =>
  readJsonItemsList<K8sKubeVirtVM>(
    withK8sContext(
      namespace
        ? `${API}/k8s/kubevirt/virtualmachines?namespace=${encodeURIComponent(namespace)}`
        : `${API}/k8s/kubevirt/virtualmachines?all_namespaces=true`,
      context,
    ),
  )

export const getK8sKubevirtVmSummary = (namespace?: string, context?: string) =>
  readJsonArray<KubeVirtVmSummaryRow>(
    withK8sContext(
      namespace
        ? `${API}/k8s/kubevirt/vm-summary?namespace=${encodeURIComponent(namespace)}`
        : `${API}/k8s/kubevirt/vm-summary?all_namespaces=true`,
      context,
    ),
  )

type KubeVirtLifecycleAction = 'start' | 'stop' | 'restart'

export const postK8sKubevirtVmLifecycle = (
  namespace: string,
  name: string,
  action: KubeVirtLifecycleAction,
  context?: string,
) =>
  apiPost<{ ok: boolean; action: string; namespace: string; name: string }>(
    `${API}/k8s/kubevirt/virtualmachines/${encodeURIComponent(namespace)}/${encodeURIComponent(name)}/lifecycle`,
    { action, context },
  )

export const deleteK8sKubevirtVm = (namespace: string, name: string, context?: string) => {
  let url = `${API}/k8s/kubevirt/virtualmachines/${encodeURIComponent(namespace)}/${encodeURIComponent(name)}`
  url = withK8sContext(url, context)
  return apiDelete(url)
}

export const patchK8sKubevirtVmSpec = (
  namespace: string,
  name: string,
  body: { vcpus?: number; memory_mib?: number; context?: string },
) =>
  apiPost<{ ok: boolean; namespace: string; name: string }>(
    `${API}/k8s/kubevirt/virtualmachines/${encodeURIComponent(namespace)}/${encodeURIComponent(name)}/spec`,
    body,
  )

export const runK8sAction = (body: K8sActionRequest) => apiPost<K8sActionResult>(`${API}/k8s/action`, body)

/** Allowlisted steps run on the **daemon host** via `helm` / `kubectl` (operator/admin only). */
export type KataDeployAction =
  | 'helm_install'
  | 'wait_kata_deploy_pod'
  | 'example_clh'
  | 'example_dragonball'
  | 'example_stratovirt'
  | 'example_qemu'

export const postKataDeploy = (body: { action: KataDeployAction; context?: string; dry_run?: boolean }) =>
  apiPost<K8sActionResult>(`${API}/k8s/kata-deploy`, body)

/** Install k3s on the daemon host via `https://get.k3s.io` (operator/admin, browser session). */
export const postK8sK3sInstall = (body: {
  install_k3s_exec?: string
  install_k3s_version?: string
  dry_run?: boolean
}) => apiPost<K8sActionResult>(`${API}/k8s/k3s/install`, body)

/** Run upstream `k3s-uninstall.sh` / `k3s-agent-uninstall.sh` on the daemon host. */
export const postK8sK3sUninstall = (body: {
  /** `server` | `agent` | `auto` (default) */
  role?: string
  dry_run?: boolean
}) => apiPost<K8sActionResult>(`${API}/k8s/k3s/uninstall`, body)

/** Phased host bootstrap — implemented in machina-daemon (`cluster_bootstrap.rs`). */
export type ClusterBootstrapPhase = 'full' | 'k3s' | 'cni' | 'metrics' | 'kubevirt_cdi'

export const postK8sClusterBootstrap = (body: {
  phase?: ClusterBootstrapPhase
  server_ip?: string
  /** When phase is `full`, skip KubeVirt/CDI/metrics virt stack */
  skip_kubevirt_cdi?: boolean
  install_metrics_server?: boolean
  /** Pod networking: k3s default (flannel + kube-proxy) unless `machina` opts in to machina-cni */
  cni?: 'default' | 'machina'
  dry_run?: boolean
}) => apiPost<K8sActionResult>(`${API}/k8s/cluster-bootstrap`, body)

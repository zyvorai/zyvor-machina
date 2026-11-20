// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{DefaultBodyLimit, Extension, Query};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use machina_core::config::K8sInventoryHistoryConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{info, warn};

use machina_core::{LibvirtError, LibvirtManager};

use crate::auth::{require_browser_session_for_host_insight, RequestActor};
use crate::cluster_bootstrap::{
    k3s_install_url, mirror, run_cluster_bootstrap, ClusterBootstrapParams, CniChoice,
};
use crate::error::AppError;
use crate::k8s_quantity::{parse_cpu_to_millicores, parse_memory_to_bytes};

const KUBECTL_TIMEOUT_SECS: u64 = 30;
const KUBECTL_PROBE_TIMEOUT_SECS: u64 = 8;
/// Kubernetes version skew: kubelet must not be newer than `kube-apiserver`, and may be at most
/// this many **minor** versions older (see upstream "Kubernetes version skew policy").
const MAX_KUBELET_MINOR_VERSIONS_BELOW_APISERVER: u32 = 3;
const KUBECTL_LOGS_TIMEOUT_SECS: u64 = 60;
const KUBECTL_APPLY_MAX_MANIFEST_BYTES: usize = 512 * 1024;
const SNIPPET_MAX_BYTES: usize = 18_432;
/// `kubectl apply -f https://…` for kata example manifests (network fetch).
const KATA_APPLY_TIMEOUT_SECS: u64 = 180;
/// `helm upgrade --install` pulling OCI chart + GitHub `releases/latest` probe.
const KATA_HELM_TIMEOUT_SECS: u64 = 300;
/// `kubectl wait` can block up to 10m for kata-deploy pods.
const KATA_WAIT_TIMEOUT_SECS: u64 = 660;
const KATA_HELM_RELEASE_NAME: &str = "kata-deploy";
const KATA_HELM_NAMESPACE: &str = "kube-system";
const KATA_HELM_CHART: &str = "oci://ghcr.io/kata-containers/kata-deploy-charts/kata-deploy";
const KATA_GITHUB_LATEST: &str =
    "https://api.github.com/repos/kata-containers/kata-containers/releases/latest";
/// When `curl` cannot reach GitHub, pin chart version (bump when kata ships a new major you care about).
const KATA_HELM_VERSION_FALLBACK: &str = "3.29.0";
/// The k3s installer (network + root on daemon host): `https://get.k3s.io` unless
/// `MACHINA_K3S_INSTALL_URL` points at an internal mirror (air-gapped sites). The URL is
/// single-quoted into a shell command, so anything with a metacharacter is refused and the
/// upstream default is used instead.
fn k3s_install_shell() -> String {
    let url = k3s_install_url();
    let bad = url.contains(['\'', '"', ';', '|', '&', '$', '`', '\n', ' ']);
    let url = if bad {
        "https://get.k3s.io".to_string()
    } else {
        url
    };
    format!("curl -sfL '{url}' | sh -")
}

fn kata_latest_api() -> String {
    mirror("MACHINA_KATA_LATEST_API_URL", KATA_GITHUB_LATEST)
}
fn kata_helm_chart() -> String {
    mirror("MACHINA_KATA_HELM_CHART", KATA_HELM_CHART)
}
fn kata_examples_base() -> String {
    mirror("MACHINA_KATA_EXAMPLES_URL", KATA_EXAMPLE_MANIFEST_BASE)
}
const K3S_INSTALL_TIMEOUT_SECS: u64 = 900;
const K3S_UNINSTALL_TIMEOUT_SECS: u64 = 420;
const K3S_UNINSTALL_SERVER: &str = "/usr/local/bin/k3s-uninstall.sh";
const K3S_UNINSTALL_AGENT: &str = "/usr/local/bin/k3s-agent-uninstall.sh";
const INSTALL_K3S_EXEC_MAX: usize = 8192;
const INSTALL_K3S_VERSION_MAX: usize = 96;
const BOOTSTRAP_SERVER_IP_MAX: usize = 253;
/// Sample workloads only — fixed upstream path on `main` (allowlisted for `kubectl apply -f`).
const KATA_EXAMPLE_MANIFEST_BASE: &str =
    "https://raw.githubusercontent.com/kata-containers/kata-containers/main/tools/packaging/kata-deploy/examples";

#[derive(Debug, Serialize)]
struct KubectlResult {
    command: String,
    stdout: String,
    stderr: String,
    exit_code: i32,
    ok: bool,
}

#[derive(Debug, Serialize, Clone)]
struct K8sNodeTaint {
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    effect: String,
}

#[derive(Debug, Serialize, Clone)]
struct K8sNodeInfo {
    name: String,
    roles: Vec<String>,
    ready: bool,
    kubelet_version: String,
    os_image: String,
    kernel_version: String,
    container_runtime: String,
    architecture: String,
    capacity: BTreeMap<String, String>,
    allocatable: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_id: Option<String>,
    /// `control_plane` | `worker` | `mixed` — from `node-role.kubernetes.io/*` labels only.
    plane: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_capacity_millicores: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_allocatable_millicores: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_capacity_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_allocatable_bytes: Option<i64>,
    /// Zone/region/instance-type and optional NFD CPU labels when present.
    topology_hints: BTreeMap<String, String>,
    /// Set when `spec.unschedulable` is true (cordoned).
    #[serde(default)]
    unschedulable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    taints: Vec<K8sNodeTaint>,
    /// Node condition `status == True` for pressure / unavailable types.
    #[serde(default)]
    memory_pressure: bool,
    #[serde(default)]
    disk_pressure: bool,
    #[serde(default)]
    pid_pressure: bool,
    #[serde(default)]
    network_unavailable: bool,
    /// Populated in cluster inventory when API server gitVersion parses cleanly.
    #[serde(skip_serializing_if = "Option::is_none")]
    kubelet_minor_matches_apiserver: Option<bool>,
    /// When daemon host DMI product UUID matches Node `status.nodeInfo.systemUUID`.
    #[serde(skip_serializing_if = "Option::is_none")]
    daemon_matches_this_machine: Option<bool>,
}

#[derive(Debug, Serialize, Default, Clone)]
struct K8sPlaneRollup {
    node_count: usize,
    ready_node_count: usize,
    cpu_capacity_millicores: i64,
    cpu_allocatable_millicores: i64,
    memory_capacity_bytes: i64,
    memory_allocatable_bytes: i64,
}

#[derive(Debug, Serialize, Default, Clone)]
struct K8sTaintPlaneRollup {
    nodes_total: usize,
    /// Nodes carrying at least one `NoSchedule` or `NoExecute` taint.
    nodes_with_scheduling_taints: usize,
}

#[derive(Debug, Serialize)]
struct K8sClusterInventoryResponse {
    collected_at_rfc3339: String,
    /// Kubernetes reports **schedulable** cpu/memory from kubelet, not physical sockets/cores unless mirrored in labels (NFD, cloud).
    disclaimer: String,
    totals_all_nodes: K8sPlaneRollup,
    /// Single bucket per plane (`control_plane`, `worker`, `mixed`); each node counted once.
    by_plane: BTreeMap<String, K8sPlaneRollup>,
    /// Control-plane & etcd footprint style view: control_plane nodes + mixed-role nodes.
    combined_control_plane_and_mixed: K8sPlaneRollup,
    /// Workload / data-plane scheduling view: worker nodes + mixed-role nodes (mixed counted in both combined views).
    combined_worker_dataplane_and_mixed: K8sPlaneRollup,
    nodes: Vec<K8sNodeInfo>,
    /// From `kubectl version -o json` → `serverVersion.gitVersion`.
    #[serde(default)]
    apiserver_git_version: String,
    /// `major.minor` parsed from `apiserver_git_version` when possible.
    #[serde(default)]
    apiserver_major_minor: String,
    /// Best-effort `kubectl get --raw /livez` (RBAC or endpoint gaps may report false).
    #[serde(default)]
    cluster_livez_ok: bool,
    #[serde(default)]
    cluster_readyz_ok: bool,
    #[serde(default)]
    cluster_health_notes: Vec<String>,
    /// Nodes whose kubelet **minor** does not match API server **minor** (upgrade hygiene).
    #[serde(default)]
    nodes_with_kubelet_minor_skew: usize,
    /// Node counts by `topology.kubernetes.io/zone` (and beta zone when zone missing).
    #[serde(default)]
    topology_nodes_by_zone: BTreeMap<String, usize>,
    /// Node counts by region labels (`topology.kubernetes.io/region` or failure-domain beta).
    #[serde(default)]
    topology_nodes_by_region: BTreeMap<String, usize>,
    /// Per-plane taint footprint (`NoSchedule` / `NoExecute`).
    #[serde(default)]
    taints_by_plane: BTreeMap<String, K8sTaintPlaneRollup>,
    /// Running pods (`phase == Running`) grouped by node plane label bucket.
    #[serde(default)]
    running_pods_by_plane: BTreeMap<String, usize>,
    #[serde(default)]
    running_pods_total: usize,
    /// Running pods with no `nodeName` (unusual; included for completeness).
    #[serde(default)]
    running_pods_without_node: usize,
    /// `Pending` pods with no `nodeName` (not yet scheduled to a node).
    #[serde(default)]
    pending_pods_unscheduled: usize,
    /// Pods resembling etcd (name/image heuristics). Not true Raft membership; stacked clusters often show one pod per member.
    #[serde(default)]
    etcd_placement_pods: Vec<K8sCpStackPod>,
    /// Other control-plane static/mirror pods (apiserver, controller-manager, scheduler, …).
    #[serde(default)]
    control_plane_stack_pods: Vec<K8sCpStackPod>,
    #[serde(default)]
    upgrade_insights: K8sUpgradeInsights,
    #[serde(default)]
    extended: K8sExtendedClusterInsights,
}

#[derive(Debug, Serialize, Default, Clone)]
struct K8sWebhookSummaryRow {
    name: String,
    webhook_rules_count: usize,
}

#[derive(Debug, Serialize, Default, Clone)]
struct K8sAddonDaemonSetRow {
    namespace: String,
    name: String,
    primary_image: String,
}

#[derive(Debug, Serialize, Default, Clone)]
struct K8sExtendedClusterInsights {
    #[serde(default)]
    validating_webhooks: Vec<K8sWebhookSummaryRow>,
    #[serde(default)]
    mutating_webhooks: Vec<K8sWebhookSummaryRow>,
    #[serde(default)]
    addon_daemonsets: Vec<K8sAddonDaemonSetRow>,
    #[serde(default)]
    gpu_allocatable_cluster_totals: BTreeMap<String, String>,
    #[serde(default)]
    daemon_machine_product_uuid: Option<String>,
    #[serde(default)]
    etcd_member_list_stdout: Option<String>,
    #[serde(default)]
    etcd_member_list_stderr: Option<String>,
    #[serde(default)]
    operator_alerts: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
struct K8sCpStackPod {
    /// `etcd`, `kube-apiserver`, `kube-controller-manager`, `kube-scheduler`, or `unknown`.
    component: String,
    namespace: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_name: Option<String>,
    /// Plane segment for `node_name` when known (`control_plane`, `worker`, `mixed`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    node_plane: Option<String>,
    phase: String,
    #[serde(default)]
    container_images: Vec<String>,
    /// Best-effort semver-like tag parsed from an image ref (`:v1.29.x`).
    #[serde(skip_serializing_if = "Option::is_none")]
    inferred_k8s_semver_tag: Option<String>,
}

#[derive(Debug, Serialize, Default, Clone)]
struct K8sUpgradeInsights {
    #[serde(default)]
    disclaimer: String,
    /// Number of **Running** pods classified as etcd by heuristics (often ≈ stacked etcd members).
    #[serde(default)]
    inferred_etcd_member_pods_running: usize,
    /// Greatest kubelet minor lag behind API server among nodes with parseable versions (same major).
    #[serde(skip_serializing_if = "Option::is_none")]
    max_kubelet_minor_lag_behind_apiserver: Option<u32>,
    /// Nodes whose kubelet is **newer** than the API server minor (unsupported skew).
    #[serde(default)]
    nodes_kubelet_newer_than_apiserver: Vec<String>,
    /// Same Kubernetes **major** as API server, but kubelet minor lag exceeds supported policy.
    #[serde(default)]
    nodes_kubelet_minor_lag_exceeds_policy: Vec<String>,
    /// Kubelet **major** is older than API server (`kubelet < apiserver` major).
    #[serde(default)]
    nodes_kubelet_major_behind_apiserver: Vec<String>,
    /// Distinct `major.minor` strings parsed from `kube-apiserver` pod images.
    #[serde(default)]
    kube_apiserver_pod_image_minors: Vec<String>,
    /// Distinct `major.minor` from `etcd` pod images when parseable.
    #[serde(default)]
    etcd_pod_image_minors: Vec<String>,
    #[serde(default)]
    upgrade_warnings: Vec<String>,
    /// Generic safe ordering hints (distro still wins).
    #[serde(default)]
    suggested_upgrade_order: Vec<String>,
}

#[derive(Debug, Serialize)]
struct K8sOverview {
    version: String,
    nodes: usize,
    ready_nodes: usize,
    namespaces: usize,
    pods: usize,
    deployments: usize,
    services: usize,
    /// Best-effort: `k3s`, `rke2`, `eks`, `gke`, `aks`, `ack`, `tke`, `cce`, `minikube`, `kind`, `generic`, or `unknown`.
    #[serde(default)]
    distribution: String,
    #[serde(default)]
    distribution_hints: Vec<String>,
    #[serde(default)]
    extra_resource_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct K8sHostSignals {
    k3s_config_present: bool,
    k3s_data_dir_present: bool,
    rke2_config_present: bool,
    rke2_data_dir_present: bool,
    k3s_systemd: String,
    k3s_agent_systemd: String,
    rke2_server_systemd: String,
    rke2_agent_systemd: String,
    k3s_binary_version: Option<String>,
    rke2_binary_version: Option<String>,
    helm_version: Option<String>,
    crictl_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct K8sEnvironment {
    kubectl_on_path: bool,
    kubectl_client_version: Option<String>,
    kubectl_server_reachable: bool,
    kubeconfig_hint: Option<String>,
    kubeconfig_from_env: bool,
    /// When set, machina injects `--kubeconfig` with this path for all cluster kubectl calls (auto-detected).
    #[serde(skip_serializing_if = "Option::is_none")]
    kubeconfig_auto_selected: Option<String>,
    current_context: Option<String>,
    cluster_distribution: String,
    cluster_distribution_hints: Vec<String>,
    host: K8sHostSignals,
    /// Truncated command output for quick operator inspection (fixed allowlist only).
    snippets: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct K8sListQuery {
    namespace: Option<String>,
    #[serde(default)]
    all_namespaces: Option<bool>,
    /// Optional `kubectl --context` (must match a context name in the merged kubeconfig).
    #[serde(default)]
    context: Option<String>,
    /// When true, return `{ rows, kubevirt_available, list_error }` for platform inventory sync.
    #[serde(default)]
    meta: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct K8sOverviewQuery {
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct K8sContextQuery {
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Deserialize)]
struct K8sLogsQuery {
    pod: String,
    namespace: Option<String>,
    #[serde(default)]
    container: Option<String>,
    #[serde(default)]
    tail_lines: Option<u32>,
    #[serde(default)]
    previous: Option<bool>,
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Deserialize)]
struct K8sEventsQuery {
    namespace: Option<String>,
    #[serde(default)]
    all_namespaces: Option<bool>,
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Deserialize)]
struct K8sApplyRequest {
    manifest: String,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum KataDeployAction {
    /// `helm upgrade --install` official OCI chart; chart `--version` from GitHub latest (fallback if probe fails).
    HelmInstall,
    /// `kubectl -n kube-system wait … -l name=kata-deploy pod`
    WaitKataDeployPod,
    ExampleClh,
    ExampleDragonball,
    ExampleStratovirt,
    ExampleQemu,
}

#[derive(Debug, Deserialize)]
struct KataDeployRequest {
    action: KataDeployAction,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct K8sAuthCanIRequest {
    verb: String,
    resource: String,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    resource_name: Option<String>,
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Deserialize)]
struct K8sHelmQuery {
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    context: Option<String>,
}

fn ensure_k8s_context_name(ctx: &str) -> Result<(), LibvirtError> {
    let t = ctx.trim();
    if t.is_empty() || t.len() > 200 {
        return Err(LibvirtError::Invalid("invalid kubectl context".into()));
    }
    let ok = t.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '.' | '-' | '_' | ':' | '/' | '@' | '#' | '+' | '%' | '[' | ']'
            )
    });
    if !ok {
        return Err(LibvirtError::Invalid(
            "kubectl context contains unsupported characters".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum K8sAction {
    NodeCordon,
    NodeUncordon,
    NodeDrain,
    RolloutRestartDeployment,
    RolloutRestartStatefulSet,
    RolloutRestartDaemonSet,
    DeletePod,
    DeleteJob,
    ScaleDeployment,
    ScaleStatefulSet,
}

#[derive(Debug, Deserialize)]
struct K8sActionRequest {
    action: K8sAction,
    name: String,
    namespace: Option<String>,
    replicas: Option<u32>,
    #[serde(default)]
    context: Option<String>,
}

fn ensure_safe_name(value: &str, field: &str) -> Result<(), LibvirtError> {
    if value.is_empty() {
        return Err(LibvirtError::Invalid(format!("{field} is required")));
    }
    // Bar a leading '-' so the value can't be interpreted as a kubectl flag when
    // used as a positional argv element (e.g. name "--all" → `kubectl delete pod --all`).
    if value.starts_with('-') {
        return Err(LibvirtError::Invalid(format!(
            "{field} must not start with '-'"
        )));
    }
    let ok = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_');
    if !ok {
        return Err(LibvirtError::Invalid(format!(
            "{field} contains invalid characters"
        )));
    }
    Ok(())
}

fn safe_namespace(value: Option<&str>) -> Result<String, LibvirtError> {
    let ns = value.unwrap_or("default");
    ensure_safe_name(ns, "namespace")?;
    Ok(ns.to_string())
}

async fn run_kubectl_timeout(
    args: &[String],
    timeout_secs: u64,
    context: Option<&str>,
) -> Result<KubectlResult, LibvirtError> {
    let choice = crate::k8s_kubeconfig::kubectl_kubeconfig_choice().await;
    let mut full = choice.prefix.clone();
    if let Some(ctx) = context {
        let t = ctx.trim();
        if !t.is_empty() {
            ensure_k8s_context_name(t)?;
            full.push("--context".into());
            full.push(t.to_string());
        }
    }
    full.extend_from_slice(args);

    let mut cmd = Command::new("kubectl");
    cmd.args(&full);
    let command_text = format!("kubectl {}", full.join(" "));
    let output = timeout(Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| LibvirtError::Operation("kubectl command timed out".into()))?
        .map_err(|e| LibvirtError::Operation(format!("failed to start kubectl: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);
    let ok = output.status.success();

    Ok(KubectlResult {
        command: command_text,
        stdout,
        stderr,
        exit_code,
        ok,
    })
}

/// User-facing cap for a kubectl error snippet — much smaller than
/// `SNIPPET_MAX_BYTES` (which is for raw output previews), since this text
/// goes straight into an HTTP error body shown in the UI.
const ERROR_SNIPPET_MAX_BYTES: usize = 400;

/// A crashed subprocess (Go runtime panic/SIGABRT from kubectl or a plugin)
/// writes a stack dump to stderr — goroutine addresses, PC registers, raw hex
/// — instead of a human-readable error. That's not truncatable into anything
/// useful, so detect it and swap in a clean message instead of leaking raw
/// memory addresses and internals to the browser. The full dump is still
/// logged server-side (via the `warn!` at the call site) for diagnosis.
fn sanitize_kubectl_stderr(stderr: &str) -> String {
    let looks_like_crash_dump = stderr.contains("goroutine ")
        || stderr.contains("SIGABRT")
        || stderr.contains("SIGSEGV")
        || stderr.starts_with("panic:")
        || stderr.contains("\npanic:")
        || stderr.contains("runtime/cgo:")
        || stderr.contains("runtime: g ")
        || stderr.contains("pthread_create failed");
    if looks_like_crash_dump {
        return "kubectl subprocess crashed unexpectedly — this is usually transient host resource pressure; retry, or check daemon logs for the full crash dump.".to_string();
    }
    truncate_error_snippet(stderr)
}

fn truncate_error_snippet(text: &str) -> String {
    let t = text.trim();
    if t.len() <= ERROR_SNIPPET_MAX_BYTES {
        return t.to_string();
    }
    let mut end = ERROR_SNIPPET_MAX_BYTES;
    while end > 0 && !t.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}… ({} more bytes, see daemon logs)",
        &t[..end],
        t.len() - end
    )
}

async fn run_kubectl_json_timeout(
    args: &[String],
    timeout_secs: u64,
    context: Option<&str>,
) -> Result<Value, LibvirtError> {
    let mut full_args = args.to_vec();
    full_args.push("-o".into());
    full_args.push("json".into());
    let res = run_kubectl_timeout(&full_args, timeout_secs, context).await?;
    if !res.ok {
        let msg = if res.stderr.trim().is_empty() {
            "kubectl command failed".to_string()
        } else {
            warn!("kubectl command failed ({}): {}", res.command, res.stderr);
            sanitize_kubectl_stderr(&res.stderr)
        };
        return Err(LibvirtError::Operation(msg));
    }
    serde_json::from_str::<Value>(&res.stdout)
        .map_err(|e| LibvirtError::Operation(format!("failed to parse kubectl JSON output: {e}")))
}

/// Services and endpoints for VM `toServices` rules; empty without kubectl
/// or a reachable cluster.
pub(crate) async fn netpol_services() -> Vec<machina_bpf::netpol::NetpolService> {
    let args = ["get", "services,endpoints", "-A"].map(String::from);
    match run_kubectl_json_timeout(&args, KUBECTL_PROBE_TIMEOUT_SECS, None).await {
        Ok(v) => machina_bpf::netpol::services_from_k8s(&v),
        Err(_) => Vec::new(),
    }
}

async fn run_kubectl_json_ctx(
    args: &[String],
    timeout_secs: u64,
    context: Option<&str>,
) -> Result<Value, LibvirtError> {
    run_kubectl_json_timeout(args, timeout_secs, context).await
}

fn truncate_snippet(text: &str) -> String {
    let t = text.trim();
    if t.len() <= SNIPPET_MAX_BYTES {
        return t.to_string();
    }
    // Truncate on a UTF-8 char boundary — `t` is external command output that may
    // contain multi-byte chars; a fixed byte-offset slice would panic.
    let mut end = SNIPPET_MAX_BYTES;
    while end > 0 && !t.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… ({} more bytes)", &t[..end], t.len() - end)
}

async fn path_exists_async(p: &str) -> bool {
    tokio::fs::metadata(p).await.is_ok()
}

fn count_list_items(res: &Result<Value, LibvirtError>) -> usize {
    res.as_ref()
        .ok()
        .and_then(|v| v.get("items").and_then(|x| x.as_array()).map(|a| a.len()))
        .unwrap_or(0)
}

async fn systemctl_line(unit: &str) -> String {
    match timeout(
        Duration::from_secs(3),
        Command::new("systemctl").args(["is-active", unit]).output(),
    )
    .await
    {
        Ok(Ok(out)) => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && s == "active" {
                "active".into()
            } else if s.is_empty() {
                "inactive".into()
            } else {
                s
            }
        }
        _ => "unknown".into(),
    }
}

async fn cmd_first_line_timeout(program: &str, args: &[&str], secs: u64) -> Option<String> {
    let out = timeout(
        Duration::from_secs(secs),
        Command::new(program).args(args.iter().copied()).output(),
    )
    .await
    .ok()
    .and_then(|r| r.ok())?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next().unwrap_or("").trim();
    if !line.is_empty() {
        return Some(line.to_string());
    }
    let e = String::from_utf8_lossy(&out.stderr);
    let el = e.lines().next().unwrap_or("").trim();
    if el.is_empty() {
        None
    } else {
        Some(el.to_string())
    }
}

fn kubeconfig_hint() -> (bool, Option<String>) {
    if let Ok(p) = std::env::var("KUBECONFIG") {
        let first = p.split(':').next().unwrap_or(&p).trim();
        if !first.is_empty() && Path::new(first).is_file() {
            return (true, Some(first.to_string()));
        }
        if !first.is_empty() {
            return (true, Some(first.to_string()));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let def = format!("{home}/.kube/config");
        if Path::new(&def).is_file() {
            return (false, Some(def));
        }
    }
    (false, None)
}

async fn collect_host_signals() -> K8sHostSignals {
    let (
        k3s_cfg,
        k3s_data,
        r2_cfg,
        r2_data,
        u_k3s,
        u_k3sa,
        u_r2s,
        u_r2a,
        k3s_ver,
        r2_ver,
        helm_v,
        cri_v,
    ) = tokio::join!(
        path_exists_async("/etc/rancher/k3s/k3s.yaml"),
        path_exists_async("/var/lib/rancher/k3s"),
        path_exists_async("/etc/rancher/rke2/config.yaml"),
        path_exists_async("/var/lib/rancher/rke2"),
        systemctl_line("k3s"),
        systemctl_line("k3s-agent"),
        systemctl_line("rke2-server"),
        systemctl_line("rke2-agent"),
        cmd_first_line_timeout("k3s", &["--version"], 5),
        cmd_first_line_timeout("rke2", &["--version"], 5),
        cmd_first_line_timeout("helm", &["version", "--short"], 5),
        cmd_first_line_timeout("crictl", &["--version"], 5),
    );
    K8sHostSignals {
        k3s_config_present: k3s_cfg,
        k3s_data_dir_present: k3s_data,
        rke2_config_present: r2_cfg,
        rke2_data_dir_present: r2_data,
        k3s_systemd: u_k3s,
        k3s_agent_systemd: u_k3sa,
        rke2_server_systemd: u_r2s,
        rke2_agent_systemd: u_r2a,
        k3s_binary_version: k3s_ver,
        rke2_binary_version: r2_ver,
        helm_version: helm_v,
        crictl_version: cri_v,
    }
}

fn infer_cluster_distribution(items: &[Value], host: &K8sHostSignals) -> (String, Vec<String>) {
    let mut hints = Vec::new();

    for n in items {
        let ni = n.get("status").and_then(|s| s.get("nodeInfo"));
        let kubelet = ni
            .and_then(|x| x.get("kubeletVersion"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let os_img = ni
            .and_then(|x| x.get("osImage"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let crt = ni
            .and_then(|x| x.get("containerRuntimeVersion"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let blob = format!("{kubelet} {os_img} {crt}").to_lowercase();
        if blob.contains("k3s") {
            hints.push(format!("node kubelet/OS/runtime mentions k3s ({kubelet})"));
            return ("k3s".to_string(), hints);
        }
        if blob.contains("rke2") {
            hints.push(format!("node kubelet/OS/runtime mentions rke2 ({kubelet})"));
            return ("rke2".to_string(), hints);
        }
    }

    for n in items {
        let prov = n
            .get("spec")
            .and_then(|s| s.get("providerID"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if prov.starts_with("aws://") {
            hints.push(format!("providerID {prov}"));
            return ("eks".to_string(), hints);
        }
        if prov.starts_with("gce://") || prov.starts_with("gcp://") {
            hints.push(format!("providerID {prov}"));
            return ("gke".to_string(), hints);
        }
        if prov.starts_with("azure://") {
            hints.push(format!("providerID {prov}"));
            return ("aks".to_string(), hints);
        }
        // Alibaba Cloud ACK (`kubernetes/cloud-provider-alibaba-cloud`; legacy `alibabacloud://`).
        if prov.starts_with("alicloud://") || prov.starts_with("alibabacloud://") {
            hints.push(format!("providerID {prov}"));
            return ("ack".to_string(), hints);
        }
        // Tencent TKE (kubelet `--cloud-provider` CCM; `qcloud://` seen on older stacks).
        if prov.starts_with("tencentcloud://") || prov.starts_with("qcloud://") {
            hints.push(format!("providerID {prov}"));
            return ("tke".to_string(), hints);
        }
        // Huawei Cloud CCE (`kubernetes-sigs/cloud-provider-huaweicloud`, `huaweicloud:///…`).
        if prov.starts_with("huaweicloud://") {
            hints.push(format!("providerID {prov}"));
            return ("cce".to_string(), hints);
        }
    }

    for n in items {
        let labels = n
            .get("metadata")
            .and_then(|m| m.get("labels"))
            .and_then(|x| x.as_object());
        if let Some(lab) = labels {
            if lab.contains_key("minikube.k8s.io/version")
                || lab.contains_key("minikube.k8s.io/name")
            {
                hints.push("minikube node labels".into());
                return ("minikube".to_string(), hints);
            }
            if lab.contains_key("kind.sigs.k8s.io/cluster") {
                hints.push("kind.sigs.k8s.io/cluster label".into());
                return ("kind".to_string(), hints);
            }
        }
    }

    for n in items {
        if let Some(name) = n
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(|x| x.as_str())
        {
            if name == "minikube" {
                hints.push("node named minikube".into());
                return ("minikube".to_string(), hints);
            }
            if name.contains("kind-control-plane") || name.contains("kind-worker") {
                hints.push(format!("node name suggests kind ({name})"));
                return ("kind".to_string(), hints);
            }
        }
    }

    if host.k3s_config_present {
        hints.push("/etc/rancher/k3s/k3s.yaml present on host".into());
        return ("k3s".to_string(), hints);
    }
    if host.k3s_data_dir_present && host.k3s_systemd == "active" {
        hints.push("k3s data dir + systemd k3s active".into());
        return ("k3s".to_string(), hints);
    }
    if host.rke2_config_present {
        hints.push("/etc/rancher/rke2/config.yaml present".into());
        return ("rke2".to_string(), hints);
    }
    if host.rke2_server_systemd == "active" {
        hints.push("rke2-server systemd active".into());
        return ("rke2".to_string(), hints);
    }

    if items.is_empty() {
        ("unknown".to_string(), hints)
    } else {
        hints.push("no known distro markers; cluster API reachable".into());
        ("generic".to_string(), hints)
    }
}

/// Helm chart value `k8sDistribution` — without this, kata-deploy looks for `/etc/containerd/config.toml`
/// inside the pod; k3s/rke2 keep config under `/var/lib/rancher/...` and need matching host mounts.
async fn kata_helm_k8s_distribution(ctx: Option<&str>) -> Option<&'static str> {
    let host = collect_host_signals().await;
    let Ok(v) = run_kubectl_json_timeout(&["get".into(), "nodes".into()], 15, ctx).await else {
        return None;
    };
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let (dist, _) = infer_cluster_distribution(&items, &host);
    match dist.as_str() {
        "k3s" => Some("k3s"),
        "rke2" => Some("rke2"),
        _ => None,
    }
}

async fn kubectl_client_version_short() -> Option<String> {
    let res = run_kubectl_timeout(
        &[
            "version".into(),
            "--client=true".into(),
            "-o".into(),
            "json".into(),
        ],
        6,
        None,
    )
    .await
    .ok()?;
    if !res.ok {
        return None;
    }
    let v: Value = serde_json::from_str(&res.stdout).ok()?;
    let gv = v
        .get("clientVersion")
        .and_then(|c| c.get("gitVersion"))
        .and_then(|x| x.as_str())?;
    Some(gv.to_string())
}

async fn k8s_environment(
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<K8sEnvironment>, AppError> {
    require_browser_session_for_host_insight(&actor)?;

    let host = collect_host_signals().await;
    let kubectl_probe =
        run_kubectl_timeout(&["version".into(), "--client=true".into()], 5, None).await;
    let kubectl_on_path = matches!(&kubectl_probe, Ok(r) if r.ok);

    let client_ver = if kubectl_on_path {
        kubectl_client_version_short().await
    } else {
        None
    };

    let nodes_res = run_kubectl_json_timeout(
        &["get".into(), "nodes".into()],
        KUBECTL_PROBE_TIMEOUT_SECS,
        None,
    )
    .await;
    let server_ok = nodes_res.is_ok();
    let node_items = nodes_res
        .as_ref()
        .ok()
        .and_then(|v| v.get("items").and_then(|x| x.as_array()).cloned())
        .unwrap_or_default();

    let (dist, hints) = infer_cluster_distribution(&node_items, &host);

    let (kubeconfig_from_env, kubeconfig_hint) = kubeconfig_hint();
    let kubeconfig_auto_selected = crate::k8s_kubeconfig::kubectl_kubeconfig_choice()
        .await
        .auto_selected_path
        .clone();

    let current_context = if kubectl_on_path {
        match run_kubectl_timeout(
            &[
                "config".into(),
                "view".into(),
                "--minify".into(),
                "-o".into(),
                "jsonpath={.current-context}".into(),
            ],
            6,
            None,
        )
        .await
        {
            Ok(r) if r.ok => {
                let s = r.stdout.trim();
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                }
            }
            _ => None,
        }
    } else {
        None
    };

    let mut snippets = BTreeMap::new();
    if kubectl_on_path {
        if let Ok(r) =
            run_kubectl_timeout(&["cluster-info".into()], KUBECTL_PROBE_TIMEOUT_SECS, None).await
        {
            snippets.insert(
                "kubectl_cluster_info".into(),
                truncate_snippet(&format!(
                    "exit={} stderr={}\n{}",
                    r.exit_code,
                    r.stderr.trim(),
                    r.stdout
                )),
            );
        }
        if let Ok(r) = run_kubectl_timeout(
            &["get".into(), "nodes".into(), "-o".into(), "wide".into()],
            KUBECTL_PROBE_TIMEOUT_SECS,
            None,
        )
        .await
        {
            snippets.insert(
                "kubectl_get_nodes_wide".into(),
                truncate_snippet(&format!(
                    "exit={} stderr={}\n{}",
                    r.exit_code,
                    r.stderr.trim(),
                    r.stdout
                )),
            );
        }
        if let Ok(r) = run_kubectl_timeout(
            &["get".into(), "--raw".into(), "/version".into()],
            KUBECTL_PROBE_TIMEOUT_SECS,
            None,
        )
        .await
        {
            snippets.insert(
                "kubectl_get_raw_version".into(),
                truncate_snippet(&r.stdout),
            );
        }
        if let Ok(r) = run_kubectl_timeout(
            &["config".into(), "get-contexts".into()],
            KUBECTL_PROBE_TIMEOUT_SECS,
            None,
        )
        .await
        {
            snippets.insert(
                "kubectl_config_get_contexts".into(),
                truncate_snippet(&r.stdout),
            );
        }
        if let Ok(r) = run_kubectl_timeout(
            &["api-resources".into(), "--verbs=list".into()],
            KUBECTL_PROBE_TIMEOUT_SECS,
            None,
        )
        .await
        {
            snippets.insert(
                "kubectl_api_resources_listable".into(),
                truncate_snippet(&r.stdout),
            );
        }
    }

    Ok(Json(K8sEnvironment {
        kubectl_on_path,
        kubectl_client_version: client_ver,
        kubectl_server_reachable: server_ok,
        kubeconfig_hint,
        kubeconfig_from_env,
        kubeconfig_auto_selected,
        current_context,
        cluster_distribution: dist.clone(),
        cluster_distribution_hints: hints.clone(),
        host,
        snippets,
    }))
}

fn classify_node_plane(roles: &[String]) -> &'static str {
    let cp = roles.iter().any(|r| r == "control-plane" || r == "master");
    let wr = roles.iter().any(|r| r == "worker");
    match (cp, wr) {
        (true, true) => "mixed",
        (true, false) => "control_plane",
        (false, _) => "worker",
    }
}

fn topology_hints_from_labels(labels: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    const KEYS: &[&str] = &[
        "node.kubernetes.io/instance-type",
        "beta.kubernetes.io/instance-type",
        "topology.kubernetes.io/region",
        "topology.kubernetes.io/zone",
        "failure-domain.beta.kubernetes.io/region",
        "failure-domain.beta.kubernetes.io/zone",
        "kubernetes.io/arch",
        "kubernetes.io/os",
    ];
    for k in KEYS {
        if let Some(v) = labels.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    for (k, v) in labels {
        if k.starts_with("feature.node.kubernetes.io/cpu") && out.len() < 48 {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

fn parse_k8s_node_item(item: &Value) -> Option<K8sNodeInfo> {
    let metadata = item.get("metadata").and_then(|x| x.as_object())?;
    let status = item.get("status").and_then(|x| x.as_object());
    let spec = item.get("spec").and_then(|x| x.as_object());
    let node_info = status
        .and_then(|s| s.get("nodeInfo"))
        .and_then(|x| x.as_object());

    let labels_obj = metadata
        .get("labels")
        .and_then(|x| x.as_object())
        .cloned()
        .unwrap_or_default();

    let mut roles = Vec::new();
    let mut safe_labels = BTreeMap::new();
    for (k, v) in labels_obj {
        if let Some(s) = v.as_str() {
            safe_labels.insert(k.clone(), s.to_string());
            if let Some(role) = k.strip_prefix("node-role.kubernetes.io/") {
                if !role.is_empty() {
                    roles.push(role.to_string());
                }
            }
        }
    }
    if roles.is_empty() {
        roles.push("worker".to_string());
    }

    let plane = classify_node_plane(&roles).to_string();
    let topology_hints = topology_hints_from_labels(&safe_labels);

    let conditions = status
        .and_then(|s| s.get("conditions"))
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let ready = conditions.iter().any(|c| {
        c.get("type").and_then(|x| x.as_str()) == Some("Ready")
            && c.get("status").and_then(|x| x.as_str()) == Some("True")
    });
    let memory_pressure = node_condition_true(&conditions, "MemoryPressure");
    let disk_pressure = node_condition_true(&conditions, "DiskPressure");
    let pid_pressure = node_condition_true(&conditions, "PIDPressure");
    let network_unavailable = node_condition_true(&conditions, "NetworkUnavailable");

    let unschedulable = spec
        .and_then(|sp| sp.get("unschedulable"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let mut taints = Vec::new();
    if let Some(arr) = spec
        .and_then(|sp| sp.get("taints"))
        .and_then(|x| x.as_array())
    {
        for t in arr {
            let key = t
                .get("key")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let effect = t
                .get("effect")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let value = t
                .get("value")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            if !key.is_empty() && !effect.is_empty() {
                taints.push(K8sNodeTaint { key, value, effect });
            }
        }
    }

    let capacity_map = status
        .and_then(|s| s.get("capacity"))
        .and_then(|x| x.as_object())
        .cloned()
        .unwrap_or_default();
    let alloc_map = status
        .and_then(|s| s.get("allocatable"))
        .and_then(|x| x.as_object())
        .cloned()
        .unwrap_or_default();

    let mut capacity = BTreeMap::new();
    for (k, v) in capacity_map {
        if let Some(s) = v.as_str() {
            capacity.insert(k, s.to_string());
        }
    }
    let mut allocatable = BTreeMap::new();
    for (k, v) in alloc_map {
        if let Some(s) = v.as_str() {
            allocatable.insert(k, s.to_string());
        }
    }

    let cpu_cap = capacity.get("cpu").and_then(|s| parse_cpu_to_millicores(s));
    let cpu_alloc = allocatable
        .get("cpu")
        .and_then(|s| parse_cpu_to_millicores(s));
    let mem_cap = capacity
        .get("memory")
        .and_then(|s| parse_memory_to_bytes(s));
    let mem_alloc = allocatable
        .get("memory")
        .and_then(|s| parse_memory_to_bytes(s));

    let metadata_uid = metadata
        .get("uid")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let system_uuid = node_info
        .and_then(|n| n.get("systemUUID"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let provider_id = spec
        .and_then(|sp| sp.get("providerID"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    Some(K8sNodeInfo {
        name: metadata
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        roles,
        ready,
        kubelet_version: node_info
            .and_then(|n| n.get("kubeletVersion"))
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        os_image: node_info
            .and_then(|n| n.get("osImage"))
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        kernel_version: node_info
            .and_then(|n| n.get("kernelVersion"))
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        container_runtime: node_info
            .and_then(|n| n.get("containerRuntimeVersion"))
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        architecture: node_info
            .and_then(|n| n.get("architecture"))
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        capacity,
        allocatable,
        labels: safe_labels,
        metadata_uid,
        system_uuid,
        provider_id,
        plane,
        cpu_capacity_millicores: cpu_cap,
        cpu_allocatable_millicores: cpu_alloc,
        memory_capacity_bytes: mem_cap,
        memory_allocatable_bytes: mem_alloc,
        topology_hints,
        unschedulable,
        taints,
        memory_pressure,
        disk_pressure,
        pid_pressure,
        network_unavailable,
        kubelet_minor_matches_apiserver: None,
        daemon_matches_this_machine: None,
    })
}

fn node_condition_true(conditions: &[Value], typ: &str) -> bool {
    conditions.iter().any(|c| {
        c.get("type").and_then(|x| x.as_str()) == Some(typ)
            && c.get("status").and_then(|x| x.as_str()) == Some("True")
    })
}

fn k8s_git_major_minor_tuple(git: &str) -> Option<(u32, u32)> {
    let s = git.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let (maj_s, rest) = s.split_once('.')?;
    let major: u32 = maj_s.parse().ok()?;
    let minor_digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let minor: u32 = minor_digits.parse().ok()?;
    Some((major, minor))
}

fn enrich_kubelet_apiserver_skew(nodes: &mut [K8sNodeInfo], server_git: &str) {
    let srv = k8s_git_major_minor_tuple(server_git);
    for n in nodes.iter_mut() {
        n.kubelet_minor_matches_apiserver = match srv {
            Some((maj, min)) => {
                let kv = k8s_git_major_minor_tuple(&n.kubelet_version);
                Some(kv.map(|(km, kn)| km == maj && kn == min).unwrap_or(false))
            }
            None => None,
        };
    }
}

fn rollup_topology_zones_regions(
    nodes: &[K8sNodeInfo],
) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
    let mut zones = BTreeMap::new();
    let mut regions = BTreeMap::new();
    for n in nodes {
        let z = n
            .topology_hints
            .get("topology.kubernetes.io/zone")
            .or_else(|| {
                n.topology_hints
                    .get("failure-domain.beta.kubernetes.io/zone")
            });
        if let Some(z) = z {
            *zones.entry(z.clone()).or_insert(0) += 1;
        }
        let r = n
            .topology_hints
            .get("topology.kubernetes.io/region")
            .or_else(|| {
                n.topology_hints
                    .get("failure-domain.beta.kubernetes.io/region")
            });
        if let Some(r) = r {
            *regions.entry(r.clone()).or_insert(0) += 1;
        }
    }
    (zones, regions)
}

fn taints_summary_by_plane(nodes: &[K8sNodeInfo]) -> BTreeMap<String, K8sTaintPlaneRollup> {
    let mut m: BTreeMap<String, K8sTaintPlaneRollup> = BTreeMap::new();
    for n in nodes {
        let e = m.entry(n.plane.clone()).or_default();
        e.nodes_total += 1;
        let harsh = n
            .taints
            .iter()
            .any(|t| t.effect == "NoSchedule" || t.effect == "NoExecute");
        if harsh {
            e.nodes_with_scheduling_taints += 1;
        }
    }
    m
}

fn rollup_pods_for_inventory(
    pods: &Value,
    node_to_plane: &BTreeMap<String, String>,
) -> (BTreeMap<String, usize>, usize, usize, usize) {
    let mut by_plane: BTreeMap<String, usize> = BTreeMap::new();
    let mut running_total = 0usize;
    let mut running_no_node = 0usize;
    let mut pending_unsched = 0usize;

    let items = pods
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    for item in items {
        let phase = item
            .get("status")
            .and_then(|s| s.get("phase"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let node_name = item
            .get("spec")
            .and_then(|s| s.get("nodeName"))
            .and_then(|x| x.as_str());

        if phase == "Pending" && node_name.is_none() {
            pending_unsched += 1;
            continue;
        }

        if phase != "Running" {
            continue;
        }

        running_total += 1;
        let Some(nn) = node_name else {
            running_no_node += 1;
            continue;
        };
        let plane = node_to_plane
            .get(nn)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        *by_plane.entry(plane).or_insert(0) += 1;
    }

    (by_plane, running_total, running_no_node, pending_unsched)
}

/// Image reference tag after the last `:`, ignoring `@sha256:` digests (returns None for digests-only).
fn container_image_tag(image: &str) -> Option<&str> {
    let t = image.trim();
    if t.is_empty() {
        return None;
    }
    if t.contains('@') {
        return None;
    }
    t.rsplit_once(':')
        .map(|(_, tag)| tag)
        .filter(|tag| !tag.contains('/') && *tag != "latest")
}

fn infer_k8s_semver_tag_from_images(images: &[String]) -> Option<String> {
    for img in images {
        let tag = container_image_tag(img)?;
        if k8s_git_major_minor_tuple(tag).is_some() {
            return Some(tag.trim().to_string());
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CpPodKind {
    Etcd,
    KubeApiserver,
    KubeControllerManager,
    KubeScheduler,
    Unknown,
}

/// Etcd: strong signal from image ref, or stacked static-pod style name in known control-plane namespaces.
fn is_likely_etcd_pod(ns: &str, pod_name: &str, images_lower: &str) -> bool {
    if images_lower.contains("/etcd:")
        || images_lower.contains("etcd:v")
        || images_lower.contains("coreos/etcd")
    {
        return true;
    }
    let ns_ok = ns == "kube-system" || ns == "openshift-etcd";
    if !ns_ok {
        return false;
    }
    let n = pod_name.to_lowercase();
    n.starts_with("etcd-") || n == "etcd"
}

fn classify_control_plane_pod_kind(ns: &str, pod_name: &str, images_lower: &str) -> CpPodKind {
    let n = pod_name.to_lowercase();
    if is_likely_etcd_pod(ns, pod_name, images_lower) {
        return CpPodKind::Etcd;
    }
    if n.contains("kube-apiserver") || images_lower.contains("kube-apiserver") {
        return CpPodKind::KubeApiserver;
    }
    if n.contains("kube-controller-manager") || images_lower.contains("kube-controller-manager") {
        return CpPodKind::KubeControllerManager;
    }
    if n.contains("kube-scheduler") || images_lower.contains("kube-scheduler") {
        return CpPodKind::KubeScheduler;
    }
    CpPodKind::Unknown
}

fn cp_kind_as_str(k: CpPodKind) -> &'static str {
    match k {
        CpPodKind::Etcd => "etcd",
        CpPodKind::KubeApiserver => "kube-apiserver",
        CpPodKind::KubeControllerManager => "kube-controller-manager",
        CpPodKind::KubeScheduler => "kube-scheduler",
        CpPodKind::Unknown => "unknown",
    }
}

fn collect_pod_container_images(pod: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let spec = pod.get("spec").and_then(|x| x.as_object());
    let Some(spec) = spec else {
        return out;
    };
    if let Some(arr) = spec.get("containers").and_then(|x| x.as_array()) {
        for c in arr {
            if let Some(im) = c.get("image").and_then(|x| x.as_str()) {
                out.push(im.to_string());
            }
        }
    }
    if let Some(arr) = spec.get("initContainers").and_then(|x| x.as_array()) {
        for c in arr {
            if let Some(im) = c.get("image").and_then(|x| x.as_str()) {
                out.push(im.to_string());
            }
        }
    }
    out
}

fn scan_control_plane_stack(
    pods: &Value,
    node_to_plane: &BTreeMap<String, String>,
) -> (Vec<K8sCpStackPod>, Vec<K8sCpStackPod>) {
    let mut etcd_pods = Vec::new();
    let mut other_cp = Vec::new();

    let items = pods
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    for pod in items {
        let meta = pod.get("metadata").and_then(|x| x.as_object());
        let Some(meta) = meta else {
            continue;
        };
        let name = meta
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let ns = meta
            .get("namespace")
            .and_then(|x| x.as_str())
            .unwrap_or("default")
            .to_string();

        let images = collect_pod_container_images(&pod);
        let images_lower = images.join(" ").to_lowercase();
        let kind = classify_control_plane_pod_kind(&ns, &name, &images_lower);
        if kind == CpPodKind::Unknown {
            continue;
        }

        let phase = pod
            .get("status")
            .and_then(|s| s.get("phase"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let node_name = pod
            .get("spec")
            .and_then(|s| s.get("nodeName"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());

        let node_plane = node_name
            .as_ref()
            .and_then(|nn| node_to_plane.get(nn).cloned());

        let inferred = infer_k8s_semver_tag_from_images(&images);

        let row = K8sCpStackPod {
            component: cp_kind_as_str(kind).to_string(),
            namespace: ns,
            name,
            node_name,
            node_plane,
            phase,
            container_images: images,
            inferred_k8s_semver_tag: inferred,
        };

        if kind == CpPodKind::Etcd {
            etcd_pods.push(row);
        } else {
            other_cp.push(row);
        }
    }

    etcd_pods.sort_by(|a, b| a.name.cmp(&b.name));
    other_cp.sort_by(|a, b| a.name.cmp(&b.name));
    (etcd_pods, other_cp)
}

fn kubelet_version_skew_lists(
    nodes: &[K8sNodeInfo],
    apiserver_git: &str,
) -> (Option<u32>, Vec<String>, Vec<String>, Vec<String>) {
    let Some((am, im)) = k8s_git_major_minor_tuple(apiserver_git) else {
        return (None, Vec::new(), Vec::new(), Vec::new());
    };

    let mut max_lag = 0u32;
    let mut newer = Vec::new();
    let mut minor_lag_exceeds = Vec::new();
    let mut major_behind = Vec::new();

    for n in nodes {
        let Some((km, kn)) = k8s_git_major_minor_tuple(&n.kubelet_version) else {
            continue;
        };

        if km > am || (km == am && kn > im) {
            newer.push(n.name.clone());
            continue;
        }

        if km < am {
            major_behind.push(format!(
                "{} (kubelet {}.{}, API {}.{})",
                n.name, km, kn, am, im
            ));
            continue;
        }

        if kn < im {
            let lag = im - kn;
            if lag > max_lag {
                max_lag = lag;
            }
            if lag > MAX_KUBELET_MINOR_VERSIONS_BELOW_APISERVER {
                minor_lag_exceeds.push(n.name.clone());
            }
        }
    }

    (Some(max_lag), newer, minor_lag_exceeds, major_behind)
}

fn minor_string_from_tag(tag: &str) -> Option<String> {
    k8s_git_major_minor_tuple(tag).map(|(a, b)| format!("{a}.{b}"))
}

fn build_upgrade_insights(
    nodes: &[K8sNodeInfo],
    apiserver_git: &str,
    etcd_pods: &[K8sCpStackPod],
    cp_pods: &[K8sCpStackPod],
    pods_inventory_available: bool,
) -> K8sUpgradeInsights {
    let mut ins = K8sUpgradeInsights {
        disclaimer: "etcd Raft membership is not read from the etcd API here — we infer likely members from pods (stacked kubeadm-style). Managed clouds (EKS, GKE, AKS, ACK, TKE, CCE, …) often expose no etcd pods; k3s/kubeadm may embed etcd without pod visibility.".to_string(),
        inferred_etcd_member_pods_running: etcd_pods.iter().filter(|p| p.phase == "Running").count(),
        ..Default::default()
    };

    let (max_lag, newer, minor_lag_exceeds, major_behind) =
        kubelet_version_skew_lists(nodes, apiserver_git);
    ins.max_kubelet_minor_lag_behind_apiserver = max_lag;
    ins.nodes_kubelet_newer_than_apiserver = newer;
    ins.nodes_kubelet_minor_lag_exceeds_policy = minor_lag_exceeds;
    ins.nodes_kubelet_major_behind_apiserver = major_behind;

    let mut api_minors = std::collections::BTreeSet::new();
    let mut etcd_minors = std::collections::BTreeSet::new();

    for p in cp_pods.iter().chain(etcd_pods.iter()) {
        if p.component == "kube-apiserver" {
            if let Some(ref tag) = p.inferred_k8s_semver_tag {
                if let Some(m) = minor_string_from_tag(tag) {
                    api_minors.insert(m);
                }
            }
        }
        if p.component == "etcd" {
            if let Some(ref tag) = p.inferred_k8s_semver_tag {
                if let Some(m) = minor_string_from_tag(tag) {
                    etcd_minors.insert(m);
                }
            }
        }
    }

    ins.kube_apiserver_pod_image_minors = api_minors.into_iter().collect();
    ins.etcd_pod_image_minors = etcd_minors.into_iter().collect();

    if ins.kube_apiserver_pod_image_minors.len() > 1 {
        ins.upgrade_warnings.push(format!(
            "Multiple distinct kube-apiserver image minors observed: {} — control plane may be mid-upgrade or misconfigured.",
            ins.kube_apiserver_pod_image_minors.join(", ")
        ));
    }
    if ins.etcd_pod_image_minors.len() > 1 {
        ins.upgrade_warnings.push(format!(
            "Multiple distinct etcd image minors observed: {} — verify HA etcd rollout state.",
            ins.etcd_pod_image_minors.join(", ")
        ));
    }

    if let Some((am, im)) = k8s_git_major_minor_tuple(apiserver_git) {
        let api_mm = format!("{am}.{im}");
        for m in &ins.kube_apiserver_pod_image_minors {
            if m != &api_mm {
                ins.upgrade_warnings.push(format!(
                    "kube-apiserver pod image minor ({m}) differs from kubectl API gitVersion minor ({api_mm}). Images may lag behind the live apiserver."
                ));
            }
        }
    }

    for p in etcd_pods {
        if p.phase == "Running" {
            if let Some(ref pl) = p.node_plane {
                if pl == "worker" {
                    ins.upgrade_warnings.push(format!(
                        "etcd pod `{}` in namespace `{}` runs on a node labeled worker-only — verify node roles (unusual for kubeadm HA).",
                        p.name, p.namespace
                    ));
                }
            }
        }
    }

    if !pods_inventory_available {
        ins.upgrade_warnings.push(
            "Pod list was unavailable — etcd/control-plane pod discovery was skipped.".into(),
        );
    } else if etcd_pods.is_empty() {
        ins.upgrade_warnings.push(
            "No etcd pods matched heuristics — cluster may use external/etcdless control plane (k3s, managed Kubernetes, etc.)."
                .into(),
        );
    } else if ins.inferred_etcd_member_pods_running > 0 {
        ins.suggested_upgrade_order.push(
            format!(
                "Observed {} running etcd-like pod(s); upgrade etcd before kube-apiserver when following kubeadm-style ordering.",
                ins.inferred_etcd_member_pods_running
            ),
        );
    }

    ins.suggested_upgrade_order.extend([
        "Follow your distribution's docs for control plane upgrades (order often: etcd → kube-apiserver → controller-manager → scheduler).".into(),
        format!(
            "Keep kubelets within supported skew: at most {MAX_KUBELET_MINOR_VERSIONS_BELOW_APISERVER} minor version(s) below the API server, and never newer than the API server."
        ),
        "After control plane upgrade, roll worker nodes / kubelets to match before widening the minor gap.".into(),
        "Check CNI/CSI and admission webhooks for compatibility with the target Kubernetes minor.".into(),
    ]);

    ins
}

fn add_node_rollup(r: &mut K8sPlaneRollup, n: &K8sNodeInfo) {
    r.node_count += 1;
    if n.ready {
        r.ready_node_count += 1;
    }
    r.cpu_capacity_millicores += n.cpu_capacity_millicores.unwrap_or(0);
    r.cpu_allocatable_millicores += n.cpu_allocatable_millicores.unwrap_or(0);
    r.memory_capacity_bytes += n.memory_capacity_bytes.unwrap_or(0);
    r.memory_allocatable_bytes += n.memory_allocatable_bytes.unwrap_or(0);
}

fn rollup_cluster_views(
    nodes: &[K8sNodeInfo],
) -> (
    K8sPlaneRollup,
    BTreeMap<String, K8sPlaneRollup>,
    K8sPlaneRollup,
    K8sPlaneRollup,
) {
    let mut totals = K8sPlaneRollup::default();
    let mut by_plane: BTreeMap<String, K8sPlaneRollup> = BTreeMap::new();
    let mut combined_control_plane_and_mixed = K8sPlaneRollup::default();
    let mut combined_worker_dataplane_and_mixed = K8sPlaneRollup::default();

    for n in nodes {
        add_node_rollup(&mut totals, n);
        add_node_rollup(by_plane.entry(n.plane.clone()).or_default(), n);

        if n.plane == "control_plane" || n.plane == "mixed" {
            add_node_rollup(&mut combined_control_plane_and_mixed, n);
        }
        if n.plane == "worker" || n.plane == "mixed" {
            add_node_rollup(&mut combined_worker_dataplane_and_mixed, n);
        }
    }

    (
        totals,
        by_plane,
        combined_control_plane_and_mixed,
        combined_worker_dataplane_and_mixed,
    )
}

fn read_daemon_product_uuid() -> Option<String> {
    std::fs::read_to_string("/sys/class/dmi/id/product_uuid")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn summarize_webhooks(json_res: &Result<Value, LibvirtError>) -> Vec<K8sWebhookSummaryRow> {
    let Ok(json) = json_res else {
        return Vec::new();
    };
    let items = json
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let name = item
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let webhook_rules_count = item
            .get("webhooks")
            .and_then(|x| x.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        out.push(K8sWebhookSummaryRow {
            name,
            webhook_rules_count,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn scan_addon_daemonsets(ds: &Value) -> Vec<K8sAddonDaemonSetRow> {
    let items = ds
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let meta = item.get("metadata").and_then(|x| x.as_object());
        let Some(meta) = meta else {
            continue;
        };
        let ns = meta
            .get("namespace")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let name = meta
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let nl = name.to_lowercase();
        let nsl = ns.to_lowercase();
        let is_addon = nsl == "kube-system"
            || nsl == "openshift-dns"
            || nl.contains("kube-proxy")
            || nl.contains("coredns")
            || nl.contains("kube-dns")
            || nl.contains("calico")
            || nl.contains("cilium")
            || nl.contains("flannel")
            || nl.contains("weave")
            || nl.contains("canal")
            || nl.contains("antrea")
            || nl.contains("csi");
        if !is_addon {
            continue;
        }
        let img = item
            .get("spec")
            .and_then(|s| s.get("template"))
            .and_then(|t| t.get("spec"))
            .and_then(|ps| ps.get("containers"))
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|co| co.get("image"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        out.push(K8sAddonDaemonSetRow {
            namespace: ns,
            name,
            primary_image: img,
        });
    }
    out.sort_by(|a, b| (a.namespace.cmp(&b.namespace)).then(a.name.cmp(&b.name)));
    out
}

fn rollup_gpu_allocatable(nodes: &[K8sNodeInfo]) -> BTreeMap<String, String> {
    let mut sums: BTreeMap<String, f64> = BTreeMap::new();
    for n in nodes {
        for (k, v) in &n.allocatable {
            let kl = k.to_lowercase();
            if kl.contains("gpu")
                || kl.contains("nvidia.com")
                || kl.contains("amd.com")
                || kl.contains("intel.com/gpu")
            {
                if let Ok(q) = v.parse::<f64>() {
                    *sums.entry(k.clone()).or_insert(0.0) += q;
                }
            }
        }
    }
    sums.into_iter()
        .map(|(k, v)| (k, format!("{}", v)))
        .collect()
}

fn build_operator_alerts(
    not_ready: usize,
    livez_ok: bool,
    readyz_ok: bool,
    minor_lag: &[String],
    major_behind: &[String],
) -> Vec<String> {
    let mut a = Vec::new();
    if not_ready > 0 {
        a.push(format!(
            "{not_ready} node(s) report NotReady — inspect kubelet, networking, and CSI."
        ));
    }
    if !livez_ok {
        a.push(
            "livez did not report success — confirm API health or RBAC for aggregated health endpoints."
                .into(),
        );
    }
    if !readyz_ok {
        a.push(
            "readyz did not report success — control-plane readiness checks failed or are blocked."
                .into(),
        );
    }
    if !minor_lag.is_empty() {
        a.push(format!(
            "{} node(s) exceed supported kubelet minor skew vs API server.",
            minor_lag.len()
        ));
    }
    if !major_behind.is_empty() {
        a.push(format!(
            "{} node(s) run an older kubelet major than the API server — upgrade soon.",
            major_behind.len()
        ));
    }
    a
}

async fn try_etcdctl_member_list(
    ctx: Option<&str>,
    etcd_pods: &[K8sCpStackPod],
) -> (Option<String>, Option<String>) {
    let Some(ep) = etcd_pods.iter().find(|p| p.phase == "Running") else {
        return (None, None);
    };
    let ns = ep.namespace.clone();
    let pod_name = ep.name.clone();
    let args = vec![
        "exec".into(),
        "-n".into(),
        ns,
        pod_name,
        "-c".into(),
        "etcd".into(),
        "--".into(),
        "etcdctl".into(),
        "member".into(),
        "list".into(),
    ];
    let res = match run_kubectl_timeout(&args, 20, ctx).await {
        Ok(r) => r,
        Err(e) => return (None, Some(format!("kubectl exec etcd: {e}"))),
    };
    let out = res.stdout.trim().to_string();
    let err = sanitize_kubectl_stderr(&res.stderr);
    if res.ok && !out.is_empty() {
        (Some(out), None)
    } else if res.ok && out.is_empty() {
        (None, if err.is_empty() { None } else { Some(err) })
    } else {
        (
            if out.is_empty() { None } else { Some(out) },
            if err.is_empty() {
                Some(format!("exit {}", res.exit_code))
            } else {
                Some(err)
            },
        )
    }
}

async fn k8s_nodes(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sContextQuery>,
) -> Result<Json<Vec<K8sNodeInfo>>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let v =
        run_kubectl_json_ctx(&["get".into(), "nodes".into()], KUBECTL_TIMEOUT_SECS, ctx).await?;
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if let Some(n) = parse_k8s_node_item(&item) {
            out.push(n);
        }
    }
    Ok(Json(out))
}

async fn k8s_cluster_inventory(
    Extension(actor): Extension<RequestActor>,
    Extension(hist_cfg): Extension<Arc<K8sInventoryHistoryConfig>>,
    Query(q): Query<K8sContextQuery>,
) -> Result<Json<K8sClusterInventoryResponse>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }

    let args_nodes = vec!["get".into(), "nodes".into()];
    let args_ver = vec!["version".into()];
    let args_pods = vec!["get".into(), "pods".into(), "-A".into()];
    let args_livez = vec!["get".into(), "--raw".into(), "/livez".into()];
    let args_readyz = vec!["get".into(), "--raw".into(), "/readyz".into()];
    let args_vwc = vec![
        "get".into(),
        "validatingwebhookconfigurations".into(),
        "-o".into(),
        "json".into(),
    ];
    let args_mwc = vec![
        "get".into(),
        "mutatingwebhookconfigurations".into(),
        "-o".into(),
        "json".into(),
    ];
    let args_ds = vec![
        "get".into(),
        "daemonsets".into(),
        "-A".into(),
        "-o".into(),
        "json".into(),
    ];

    let (nodes_res, ver_res, pods_res, livez_res, readyz_res, vwc_res, mwc_res, ds_res) = tokio::join!(
        run_kubectl_json_ctx(&args_nodes, KUBECTL_TIMEOUT_SECS, ctx),
        run_kubectl_json_ctx(&args_ver, KUBECTL_TIMEOUT_SECS, ctx),
        run_kubectl_json_ctx(&args_pods, KUBECTL_TIMEOUT_SECS, ctx),
        run_kubectl_timeout(&args_livez, KUBECTL_PROBE_TIMEOUT_SECS, ctx),
        run_kubectl_timeout(&args_readyz, KUBECTL_PROBE_TIMEOUT_SECS, ctx),
        run_kubectl_json_ctx(&args_vwc, KUBECTL_TIMEOUT_SECS, ctx),
        run_kubectl_json_ctx(&args_mwc, KUBECTL_TIMEOUT_SECS, ctx),
        run_kubectl_json_ctx(&args_ds, KUBECTL_TIMEOUT_SECS, ctx),
    );

    let v = nodes_res?;
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    let mut nodes = Vec::with_capacity(items.len());
    for item in items {
        if let Some(n) = parse_k8s_node_item(&item) {
            nodes.push(n);
        }
    }

    let apiserver_git_version = ver_res
        .ok()
        .and_then(|json| {
            json.get("serverVersion")
                .and_then(|s| s.get("gitVersion"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    let apiserver_major_minor = k8s_git_major_minor_tuple(&apiserver_git_version)
        .map(|(a, b)| format!("{a}.{b}"))
        .unwrap_or_default();

    if !apiserver_git_version.is_empty() {
        enrich_kubelet_apiserver_skew(&mut nodes, &apiserver_git_version);
    }

    let host_uuid = read_daemon_product_uuid();
    for n in &mut nodes {
        n.daemon_matches_this_machine = match (&host_uuid, &n.system_uuid) {
            (Some(h), Some(s)) if !h.is_empty() && !s.trim().is_empty() => {
                Some(h.eq_ignore_ascii_case(s.trim()))
            }
            _ => None,
        };
    }

    let nodes_with_kubelet_minor_skew = nodes
        .iter()
        .filter(|n| n.kubelet_minor_matches_apiserver == Some(false))
        .count();

    let not_ready_nodes = nodes.iter().filter(|n| !n.ready).count();

    let cluster_livez_ok = livez_res.as_ref().map(|r| r.ok).unwrap_or(false);
    let cluster_readyz_ok = readyz_res.as_ref().map(|r| r.ok).unwrap_or(false);

    let mut cluster_health_notes = Vec::new();
    if livez_res.as_ref().map(|r| !r.ok).unwrap_or(true) {
        if let Ok(r) = &livez_res {
            let hint = if !r.stderr.trim().is_empty() {
                r.stderr.trim()
            } else if !r.stdout.trim().is_empty() {
                r.stdout.trim()
            } else {
                "livez probe failed"
            };
            cluster_health_notes.push(format!("livez: {}", sanitize_kubectl_stderr(hint)));
        } else {
            cluster_health_notes.push("livez: kubectl error".to_string());
        }
    }
    if readyz_res.as_ref().map(|r| !r.ok).unwrap_or(true) {
        if let Ok(r) = &readyz_res {
            let hint = if !r.stderr.trim().is_empty() {
                r.stderr.trim()
            } else if !r.stdout.trim().is_empty() {
                r.stdout.trim()
            } else {
                "readyz probe failed"
            };
            cluster_health_notes.push(format!("readyz: {}", sanitize_kubectl_stderr(hint)));
        } else {
            cluster_health_notes.push("readyz: kubectl error".to_string());
        }
    }

    let node_to_plane: BTreeMap<String, String> = nodes
        .iter()
        .map(|n| (n.name.clone(), n.plane.clone()))
        .collect();

    let (
        running_pods_by_plane,
        running_pods_total,
        running_pods_without_node,
        pending_pods_unscheduled,
    ) = match &pods_res {
        Ok(pj) => rollup_pods_for_inventory(pj, &node_to_plane),
        Err(_) => (BTreeMap::new(), 0, 0, 0),
    };
    if pods_res.is_err() {
        cluster_health_notes
            .push("Pods list failed — running pod counts by plane omitted.".to_string());
    }

    let (topology_nodes_by_zone, topology_nodes_by_region) = rollup_topology_zones_regions(&nodes);
    let taints_by_plane = taints_summary_by_plane(&nodes);

    let (etcd_placement_pods, control_plane_stack_pods, upgrade_insights) = match pods_res.as_ref()
    {
        Ok(pj) => {
            let (etcd, cp) = scan_control_plane_stack(pj, &node_to_plane);
            let insights = build_upgrade_insights(&nodes, &apiserver_git_version, &etcd, &cp, true);
            (etcd, cp, insights)
        }
        Err(_) => (
            Vec::new(),
            Vec::new(),
            build_upgrade_insights(&nodes, &apiserver_git_version, &[], &[], false),
        ),
    };

    let (etcd_out, etcd_err) = try_etcdctl_member_list(ctx, &etcd_placement_pods).await;

    let validating_webhooks = summarize_webhooks(&vwc_res);
    let mutating_webhooks = summarize_webhooks(&mwc_res);
    let addon_daemonsets = ds_res
        .as_ref()
        .map(scan_addon_daemonsets)
        .unwrap_or_default();
    let gpu_allocatable_cluster_totals = rollup_gpu_allocatable(&nodes);

    let operator_alerts = build_operator_alerts(
        not_ready_nodes,
        cluster_livez_ok,
        cluster_readyz_ok,
        &upgrade_insights.nodes_kubelet_minor_lag_exceeds_policy,
        &upgrade_insights.nodes_kubelet_major_behind_apiserver,
    );

    let extended = K8sExtendedClusterInsights {
        validating_webhooks,
        mutating_webhooks,
        addon_daemonsets,
        gpu_allocatable_cluster_totals,
        daemon_machine_product_uuid: host_uuid,
        etcd_member_list_stdout: etcd_out,
        etcd_member_list_stderr: etcd_err,
        operator_alerts,
    };

    let (
        totals_all_nodes,
        by_plane,
        combined_control_plane_and_mixed,
        combined_worker_dataplane_and_mixed,
    ) = rollup_cluster_views(&nodes);

    let resp = K8sClusterInventoryResponse {
        collected_at_rfc3339: chrono::Utc::now().to_rfc3339(),
        disclaimer: "Kubernetes Node objects expose kubelet-reported capacity/allocatable cpu and memory, not physical CPU sockets/cores/hyperthreads. Those appear only if mirrored by labels (e.g. cloud instance-type, Node Feature Discovery) or custom operators. This is inventory visibility, not VMware-style subscription licensing.".to_string(),
        totals_all_nodes,
        by_plane,
        combined_control_plane_and_mixed,
        combined_worker_dataplane_and_mixed,
        nodes,
        apiserver_git_version,
        apiserver_major_minor,
        cluster_livez_ok,
        cluster_readyz_ok,
        cluster_health_notes,
        nodes_with_kubelet_minor_skew,
        topology_nodes_by_zone,
        topology_nodes_by_region,
        taints_by_plane,
        running_pods_by_plane,
        running_pods_total,
        running_pods_without_node,
        pending_pods_unscheduled,
        etcd_placement_pods,
        control_plane_stack_pods,
        upgrade_insights,
        extended,
    };

    if hist_cfg.enabled {
        if let Ok(line) = serde_json::to_string(&resp) {
            let max_b = hist_cfg.max_file_mb.saturating_mul(1024 * 1024);
            let _ = tokio::task::spawn_blocking(move || {
                crate::k8s_inventory_history::append_k8s_cluster_inventory_line(&line, max_b)
            })
            .await;
        }
    }

    Ok(Json(resp))
}

#[derive(Debug, Deserialize)]
struct K8sClusterInventoryHistoryQuery {
    #[serde(default)]
    limit: Option<usize>,
}

async fn k8s_cluster_inventory_history(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sClusterInventoryHistoryQuery>,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let lim = q.limit.unwrap_or(20).clamp(1, 100);
    let entries = crate::k8s_inventory_history::load_k8s_cluster_inventory_history(lim)
        .map_err(AppError::from)?;
    Ok(Json(serde_json::json!({
        "path": crate::k8s_inventory_history::k8s_cluster_inventory_jsonl_path().display().to_string(),
        "entries": entries,
    })))
}

async fn k8s_resource_list(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
    resource: &'static str,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    if let Some(ns) = q.namespace.as_deref() {
        ensure_safe_name(ns, "namespace")?;
    }
    let all_ns = q.all_namespaces.unwrap_or(false);

    let mut args = vec!["get".to_string(), resource.to_string()];
    if all_ns {
        args.push("-A".to_string());
    } else if let Some(ns) = q.namespace {
        args.push("-n".to_string());
        args.push(ns);
    }

    let v = run_kubectl_json_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    Ok(Json(v))
}

async fn k8s_namespaces(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sContextQuery>,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let v = run_kubectl_json_ctx(
        &["get".into(), "namespaces".into()],
        KUBECTL_TIMEOUT_SECS,
        ctx,
    )
    .await?;
    Ok(Json(v))
}

async fn k8s_pods(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "pods").await
}

async fn k8s_deployments(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "deployments").await
}

async fn k8s_services(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "services").await
}

async fn k8s_statefulsets(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "statefulsets").await
}

async fn k8s_daemonsets(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "daemonsets").await
}

async fn k8s_jobs(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "jobs").await
}

async fn k8s_cronjobs(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "cronjobs").await
}

async fn k8s_ingresses(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "ingresses.networking.k8s.io").await
}

async fn k8s_persistentvolumeclaims(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    k8s_resource_list(Extension(actor), Query(q), "persistentvolumeclaims").await
}

async fn k8s_persistentvolumes(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sContextQuery>,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let args = vec!["get".into(), "persistentvolumes".into()];
    let v = run_kubectl_json_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    Ok(Json(v))
}

async fn k8s_storageclasses(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sContextQuery>,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let args = vec!["get".into(), "storageclasses".into()];
    let v = run_kubectl_json_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    Ok(Json(v))
}

async fn k8s_events(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sEventsQuery>,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let all_ns = q.all_namespaces.unwrap_or(false);
    let mut args = vec![
        "get".into(),
        "events".into(),
        "-o".into(),
        "json".into(),
        "--sort-by=.metadata.creationTimestamp".into(),
    ];
    if all_ns {
        args.push("-A".into());
    } else if let Some(ns) = q.namespace.clone() {
        ensure_safe_name(&ns, "namespace")?;
        args.push("-n".into());
        args.push(ns);
    } else {
        args.push("-n".into());
        args.push("default".into());
    }
    let v = run_kubectl_json_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    Ok(Json(v))
}

async fn k8s_pod_logs(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sLogsQuery>,
) -> Result<Json<KubectlResult>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    ensure_safe_name(&q.pod, "pod")?;
    let ns = safe_namespace(q.namespace.as_deref())?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let tail = q.tail_lines.unwrap_or(200).clamp(1, 50_000);
    let mut args = vec![
        "logs".into(),
        q.pod.clone(),
        "-n".into(),
        ns,
        format!("--tail={tail}"),
    ];
    if let Some(ref c) = q.container {
        ensure_safe_name(c, "container")?;
        args.push("-c".into());
        args.push(c.clone());
    }
    if q.previous == Some(true) {
        args.push("--previous".into());
    }
    let res = run_kubectl_timeout(&args, KUBECTL_LOGS_TIMEOUT_SECS, ctx).await?;
    Ok(Json(res))
}

fn ensure_safe_k8s_token(s: &str, field: &str) -> Result<(), LibvirtError> {
    let t = s.trim();
    if t.is_empty() || t.len() > 80 {
        return Err(LibvirtError::Invalid(format!("{field} is invalid")));
    }
    let ok = t.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '*' | '/' | '.' | '-')
    });
    if !ok {
        return Err(LibvirtError::Invalid(format!(
            "{field} has invalid characters"
        )));
    }
    Ok(())
}

async fn k8s_auth_can_i(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<K8sAuthCanIRequest>,
) -> Result<Json<KubectlResult>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    ensure_safe_k8s_token(&body.verb, "verb")?;
    ensure_safe_k8s_token(&body.resource, "resource")?;
    let ctx = body.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let mut args = vec![
        "auth".into(),
        "can-i".into(),
        body.verb.clone(),
        body.resource.clone(),
    ];
    if let Some(ref n) = body.resource_name {
        ensure_safe_name(n, "resource_name")?;
        args.push(n.clone());
    }
    if let Some(ref ns) = body.namespace {
        ensure_safe_name(ns, "namespace")?;
        args.push("-n".into());
        args.push(ns.clone());
    }
    let res = run_kubectl_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    Ok(Json(res))
}

async fn k8s_apply_manifest(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<K8sApplyRequest>,
) -> Result<Json<KubectlResult>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Applying manifests requires the operator or admin role.".into(),
        )
        .into());
    }
    if body.manifest.len() > KUBECTL_APPLY_MAX_MANIFEST_BYTES {
        return Err(LibvirtError::Invalid(format!(
            "manifest exceeds {} bytes",
            KUBECTL_APPLY_MAX_MANIFEST_BYTES
        ))
        .into());
    }
    let ctx = body.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let path = std::env::temp_dir().join(format!(
        "machina-k8s-apply-{}.yaml",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    tokio::fs::write(&path, body.manifest.as_bytes())
        .await
        .map_err(|e| LibvirtError::Operation(format!("temp manifest: {e}")))?;
    let ps = path.to_string_lossy().to_string();
    let mut args = vec!["apply".into(), "-f".into(), ps.clone()];
    if body.dry_run == Some(true) {
        args.push("--dry-run=server".into());
    }
    let out = run_kubectl_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await;
    let _ = tokio::fs::remove_file(&path).await;
    Ok(Json(out?))
}

/// Allowlisted Helm / `kubectl` steps for upstream kata-deploy (fixed chart URL + example manifest URLs).
async fn k8s_kata_deploy(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<KataDeployRequest>,
) -> Result<Json<KubectlResult>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Kata deploy automation requires the operator or admin role.".into(),
        )
        .into());
    }
    let ctx = body.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }

    let build_example_apply = |suffix: &str, dry_run_apply: bool| -> Vec<String> {
        let url = format!("{}{suffix}", kata_examples_base());
        let mut a = vec!["apply".into(), "-f".into(), url];
        if dry_run_apply {
            a.push("--dry-run=server".into());
        }
        a
    };

    let dry = body.dry_run == Some(true);
    let (res, tool): (KubectlResult, &'static str) = match body.action {
        KataDeployAction::HelmInstall => {
            let version = match kata_containers_latest_release_tag().await {
                Ok(t) => t,
                Err(e) => {
                    warn!(
                        target: "machina_k8s",
                        error = %e,
                        "kata GitHub latest tag fetch failed; using fallback chart version {}",
                        KATA_HELM_VERSION_FALLBACK
                    );
                    KATA_HELM_VERSION_FALLBACK.to_string()
                }
            };
            let mut args = vec![
                "upgrade".into(),
                "--install".into(),
                KATA_HELM_RELEASE_NAME.into(),
                kata_helm_chart(),
            ];
            if let Some(d) = kata_helm_k8s_distribution(ctx).await {
                info!(
                    target: "machina_k8s",
                    "kata-deploy helm: setting k8sDistribution={d} (k3s/rke2 containerd paths)"
                );
                args.push("--set".into());
                args.push(format!("k8sDistribution={d}"));
            }
            args.extend([
                "--version".into(),
                version,
                "-n".into(),
                KATA_HELM_NAMESPACE.into(),
                "--create-namespace".into(),
            ]);
            if dry {
                args.push("--dry-run".into());
            }
            let out = run_helm_timeout_kube(&args, KATA_HELM_TIMEOUT_SECS, ctx).await?;
            (out, "helm")
        }
        KataDeployAction::WaitKataDeployPod => {
            if dry {
                return Err(LibvirtError::Invalid(
                    "dry_run is not supported for wait_kata_deploy_pod".into(),
                )
                .into());
            }
            let args = vec![
                "-n".into(),
                "kube-system".into(),
                "wait".into(),
                "--timeout=10m".into(),
                "--for=condition=Ready".into(),
                "-l".into(),
                "name=kata-deploy".into(),
                "pod".into(),
            ];
            let out = run_kubectl_timeout(&args, KATA_WAIT_TIMEOUT_SECS, ctx).await?;
            (out, "kubectl")
        }
        KataDeployAction::ExampleClh => {
            let args = build_example_apply("/test-deploy-kata-clh.yaml", dry);
            let out = run_kubectl_timeout(&args, KATA_APPLY_TIMEOUT_SECS, ctx).await?;
            (out, "kubectl")
        }
        KataDeployAction::ExampleDragonball => {
            let args = build_example_apply("/test-deploy-kata-dragonball.yaml", dry);
            let out = run_kubectl_timeout(&args, KATA_APPLY_TIMEOUT_SECS, ctx).await?;
            (out, "kubectl")
        }
        KataDeployAction::ExampleStratovirt => {
            let args = build_example_apply("/test-deploy-kata-stratovirt.yaml", dry);
            let out = run_kubectl_timeout(&args, KATA_APPLY_TIMEOUT_SECS, ctx).await?;
            (out, "kubectl")
        }
        KataDeployAction::ExampleQemu => {
            let args = build_example_apply("/test-deploy-kata-qemu.yaml", dry);
            let out = run_kubectl_timeout(&args, KATA_APPLY_TIMEOUT_SECS, ctx).await?;
            (out, "kubectl")
        }
    };

    if !res.ok {
        let msg = if res.stderr.trim().is_empty() {
            format!("{tool} failed (exit {}): {}", res.exit_code, res.command)
        } else {
            warn!("{tool} command failed ({}): {}", res.command, res.stderr);
            sanitize_kubectl_stderr(&res.stderr)
        };
        return Err(LibvirtError::Operation(msg).into());
    }
    Ok(Json(res))
}

async fn k8s_contexts_list(
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let res = run_kubectl_timeout(
        &[
            "config".into(),
            "get-contexts".into(),
            "-o".into(),
            "name".into(),
        ],
        20,
        None,
    )
    .await?;
    let contexts: Vec<String> = res
        .stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(Json(serde_json::json!({ "contexts": contexts })))
}

async fn run_helm_timeout(
    args: &[String],
    timeout_secs: u64,
) -> Result<KubectlResult, LibvirtError> {
    let mut cmd = Command::new("helm");
    cmd.args(args);
    let command_text = format!("helm {}", args.join(" "));
    let output = timeout(Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| LibvirtError::Operation("helm command timed out".into()))?
        .map_err(|e| LibvirtError::Operation(format!("failed to start helm: {e}")))?;
    Ok(KubectlResult {
        command: command_text,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
        ok: output.status.success(),
    })
}

/// Same kubeconfig auto-selection as `kubectl` (k3s/rke2 admin files) + optional `--kube-context`.
async fn run_helm_timeout_kube(
    subcommand_and_args: &[String],
    timeout_secs: u64,
    context: Option<&str>,
) -> Result<KubectlResult, LibvirtError> {
    let choice = crate::k8s_kubeconfig::kubectl_kubeconfig_choice().await;
    let mut full = choice.prefix.clone();
    if let Some(ctx) = context {
        let t = ctx.trim();
        if !t.is_empty() {
            ensure_k8s_context_name(t)?;
            full.push("--kube-context".into());
            full.push(t.to_string());
        }
    }
    full.extend_from_slice(subcommand_and_args);
    run_helm_timeout(&full, timeout_secs).await
}

async fn kata_containers_latest_release_tag() -> Result<String, LibvirtError> {
    let output = timeout(
        Duration::from_secs(25),
        Command::new("curl")
            .args([
                "-fsSL",
                "-H",
                "User-Agent: machina-daemon",
                kata_latest_api().as_str(),
            ])
            .output(),
    )
    .await
    .map_err(|_| {
        LibvirtError::Operation("curl GitHub releases/latest timed out (is curl installed?)".into())
    })?
    .map_err(|e| LibvirtError::Operation(format!("failed to start curl: {e}")))?;
    if !output.status.success() {
        return Err(LibvirtError::Operation(format!(
            "curl GitHub API exit {} (install curl for latest chart version, or chart version falls back in code path only for helm — check stderr)",
            output.status.code().unwrap_or(-1)
        )));
    }
    let v: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| LibvirtError::Operation(format!("GitHub API JSON: {e}")))?;
    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| LibvirtError::Operation("GitHub API missing tag_name".into()))?;
    if tag.is_empty() || tag.len() > 64 {
        return Err(LibvirtError::Operation(
            "refusing empty or oversized release tag".into(),
        ));
    }
    if !tag
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(LibvirtError::Operation(
            "refusing odd release tag characters".into(),
        ));
    }
    Ok(tag.to_string())
}

async fn k8s_helm_releases(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sHelmQuery>,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    let mut inner: Vec<String> = vec![
        "list".into(),
        "-o".into(),
        "json".into(),
        "--max".into(),
        "200".into(),
    ];
    if q.namespace.as_deref() == Some("all") || q.namespace.as_deref() == Some("*") {
        inner.push("-A".into());
    } else if let Some(ns) = q.namespace.clone() {
        ensure_safe_name(&ns, "namespace")?;
        inner.push("-n".into());
        inner.push(ns);
    } else {
        inner.push("-A".into());
    }
    let res = run_helm_timeout_kube(&inner, 45, ctx).await?;
    if !res.ok {
        warn!("helm list command failed ({}): {}", res.command, res.stderr);
        return Err(LibvirtError::Operation(sanitize_kubectl_stderr(&res.stderr)).into());
    }
    let v: Value = serde_json::from_str(&res.stdout)
        .map_err(|e| LibvirtError::Operation(format!("helm list JSON: {e}")))?;
    Ok(Json(v))
}

#[derive(Debug, Serialize)]
struct KubeVirtVmSummaryRow {
    name: String,
    namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec_running: Option<bool>,
    /// `spec.runStrategy` — the modern KubeVirt run-intent field; `spec.running` and this are
    /// mutually exclusive (a VM sets one or the other, never both).
    #[serde(skip_serializing_if = "Option::is_none")]
    spec_run_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vm_printable_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vm_ready: Option<bool>,
    /// Primary guest-visible IP(s) from the VMI `status.interfaces` list.
    #[serde(skip_serializing_if = "Option::is_none")]
    guest_ip: Option<String>,
    /// Virt-launcher / pod network IP when reported on the VMI (`status.podIP` or first interface).
    #[serde(skip_serializing_if = "Option::is_none")]
    pod_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vmi_phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_internal_ip: Option<String>,
    /// Run on a machine with cluster credentials (often same host as machina).
    virtctl_console: String,
    virtctl_vnc: String,
    /// `virtctl vnc` with SOCKS proxy for browsers / clients that support it.
    virtctl_vnc_socks: String,
    /// API path segment (use with `kubectl proxy` + authorized WebSocket client).
    vnc_subresource_path: String,
}

fn node_internal_ip_map(nodes: &[Value]) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for n in nodes {
        let Some(name) = n
            .get("metadata")
            .and_then(|x| x.get("name"))
            .and_then(|x| x.as_str())
        else {
            continue;
        };
        let Some(addrs) = n
            .get("status")
            .and_then(|s| s.get("addresses"))
            .and_then(|x| x.as_array())
        else {
            continue;
        };
        for a in addrs {
            if a.get("type").and_then(|x| x.as_str()) == Some("InternalIP") {
                if let Some(ip) = a.get("address").and_then(|x| x.as_str()) {
                    m.insert(name.to_string(), ip.to_string());
                    break;
                }
            }
        }
    }
    m
}

fn vmi_guest_ips(vmi: &Value) -> Option<String> {
    let ifs = vmi
        .get("status")
        .and_then(|s| s.get("interfaces"))
        .and_then(|x| x.as_array())?;
    let mut ips = Vec::new();
    for i in ifs {
        if let Some(ip) = i.get("ipAddress").and_then(|x| x.as_str()) {
            if !ip.is_empty() && !ip.starts_with("127.") && !ips.iter().any(|e| e == ip) {
                ips.push(ip.to_string());
            }
        }
    }
    if ips.is_empty() {
        None
    } else {
        Some(ips.join(", "))
    }
}

fn vmi_pod_ip_strict(vmi: &Value) -> Option<String> {
    vmi.get("status")
        .and_then(|s| s.get("podIP"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn vmi_node_and_phase(vmi: &Value) -> (Option<String>, Option<String>) {
    let st = vmi.get("status").and_then(|x| x.as_object());
    let node = st
        .and_then(|s| s.get("nodeName"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let phase = st
        .and_then(|s| s.get("phase"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    (node, phase)
}

fn index_vmi_by_ns_name(items: &[Value]) -> BTreeMap<(String, String), Value> {
    let mut m = BTreeMap::new();
    for item in items {
        let meta = item.get("metadata").and_then(|x| x.as_object());
        let Some(ns) = meta
            .and_then(|x| x.get("namespace"))
            .and_then(|x| x.as_str())
        else {
            continue;
        };
        let Some(name) = meta.and_then(|x| x.get("name")).and_then(|x| x.as_str()) else {
            continue;
        };
        m.insert((ns.to_string(), name.to_string()), item.clone());
    }
    m
}

/// True when the API server has no KubeVirt `VirtualMachine` CRD (or similar), so an empty list is OK.
fn kubevirt_vm_list_unavailable(err: &LibvirtError) -> bool {
    let LibvirtError::Operation(msg) = err else {
        return false;
    };
    let m = msg.to_lowercase();
    m.contains("the server doesn't have a resource type")
        || m.contains("couldn't find resource")
        || m.contains("no matches for kind")
        || m.contains("does not support")
        || m.contains("unable to recognize")
}

/// `kubectl get virtualmachines.kubevirt.io` (KubeVirt). Returns an empty list when the CRD is not installed.
async fn k8s_kubevirt_virtualmachines(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<Json<Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    if let Some(ns) = q.namespace.as_deref() {
        ensure_safe_name(ns, "namespace")?;
    }
    let all_ns = q.all_namespaces.unwrap_or(false);

    let mut args = vec!["get".into(), "virtualmachines.kubevirt.io".into()];
    if all_ns {
        args.push("-A".into());
    } else if let Some(ns) = q.namespace.clone() {
        args.push("-n".into());
        args.push(ns);
    }

    match run_kubectl_json_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await {
        Ok(v) => Ok(Json(v)),
        Err(e) if kubevirt_vm_list_unavailable(&e) => {
            warn!("kubevirt VirtualMachine list skipped: {e}");
            Ok(Json(serde_json::json!({
                "apiVersion": "v1",
                "items": [],
                "kind": "List",
                "metadata": {}
            })))
        }
        Err(e) => Err(e.into()),
    }
}

/// VirtualMachines merged with VMIs and node InternalIPs, plus copy-paste `virtctl` / VNC API paths.
async fn k8s_kubevirt_vm_summary(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sListQuery>,
) -> Result<axum::response::Response, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    if let Some(ns) = q.namespace.as_deref() {
        ensure_safe_name(ns, "namespace")?;
    }
    let all_ns = q.all_namespaces.unwrap_or(false);

    let mut vm_args = vec!["get".into(), "virtualmachines.kubevirt.io".into()];
    if all_ns {
        vm_args.push("-A".into());
    } else if let Some(ns) = q.namespace.clone() {
        vm_args.push("-n".into());
        vm_args.push(ns);
    }

    let meta = q.meta.unwrap_or(false);
    let vm_json = match run_kubectl_json_timeout(&vm_args, KUBECTL_TIMEOUT_SECS, ctx).await {
        Ok(v) => v,
        Err(e) if kubevirt_vm_list_unavailable(&e) => {
            if meta {
                return Ok(axum::Json(serde_json::json!({
                    "rows": [],
                    "kubevirt_available": false,
                    "list_error": e.to_string(),
                }))
                .into_response());
            }
            return Ok(axum::Json(Vec::<KubeVirtVmSummaryRow>::new()).into_response());
        }
        Err(e) => {
            if meta {
                return Ok(axum::Json(serde_json::json!({
                    "rows": [],
                    "kubevirt_available": true,
                    "list_error": e.to_string(),
                }))
                .into_response());
            }
            return Err(e.into());
        }
    };

    let mut vmi_args = vec!["get".into(), "virtualmachineinstances.kubevirt.io".into()];
    if all_ns {
        vmi_args.push("-A".into());
    } else if let Some(ns) = q.namespace.clone() {
        vmi_args.push("-n".into());
        vmi_args.push(ns);
    }

    let args_nodes = vec!["get".into(), "nodes".into()];
    let (vmi_res, nodes_res) = tokio::join!(
        run_kubectl_json_timeout(&vmi_args, KUBECTL_TIMEOUT_SECS, ctx),
        run_kubectl_json_timeout(&args_nodes, KUBECTL_TIMEOUT_SECS, ctx),
    );

    let vmi_items = match vmi_res {
        Ok(v) => v
            .get("items")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default(),
        Err(e) if kubevirt_vm_list_unavailable(&e) => {
            warn!("kubevirt VMI list skipped: {e}");
            vec![]
        }
        Err(e) => return Err(e.into()),
    };

    let node_items = nodes_res
        .ok()
        .and_then(|v| v.get("items").and_then(|x| x.as_array()).cloned())
        .unwrap_or_default();
    let node_ips = node_internal_ip_map(&node_items);
    let vmi_index = index_vmi_by_ns_name(&vmi_items);

    let vm_items = vm_json
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    let mut rows = Vec::with_capacity(vm_items.len());
    for vm in vm_items {
        let meta = vm.get("metadata").and_then(|x| x.as_object());
        let Some(ns) = meta
            .and_then(|m| m.get("namespace"))
            .and_then(|x| x.as_str())
        else {
            continue;
        };
        let Some(name) = meta.and_then(|m| m.get("name")).and_then(|x| x.as_str()) else {
            continue;
        };

        let spec_running = vm
            .get("spec")
            .and_then(|s| s.get("running"))
            .and_then(|x| x.as_bool());
        let spec_run_strategy = vm
            .get("spec")
            .and_then(|s| s.get("runStrategy"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let vm_printable_status = vm
            .get("status")
            .and_then(|s| s.get("printableStatus"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let vm_ready = vm
            .get("status")
            .and_then(|s| s.get("ready"))
            .and_then(|x| x.as_bool());

        let key = (ns.to_string(), name.to_string());
        let (guest_ip, pod_ip, node_name, vmi_phase) = if let Some(vmi) = vmi_index.get(&key) {
            let guest = vmi_guest_ips(vmi);
            let pod = vmi_pod_ip_strict(vmi);
            let (nn, ph) = vmi_node_and_phase(vmi);
            (guest, pod, nn, ph)
        } else {
            (None, None, None, None)
        };

        let node_internal_ip = node_name.as_ref().and_then(|nn| node_ips.get(nn).cloned());

        let virtctl_console = format!("virtctl console {name} -n {ns}");
        let virtctl_vnc = format!("virtctl vnc {name} -n {ns}");
        let virtctl_vnc_socks = format!("virtctl vnc {name} -n {ns} --proxy-only");
        let vnc_subresource_path = format!(
            "/apis/subresources.kubevirt.io/v1/namespaces/{ns}/virtualmachineinstances/{name}/vnc"
        );

        rows.push(KubeVirtVmSummaryRow {
            name: name.to_string(),
            namespace: ns.to_string(),
            spec_running,
            spec_run_strategy,
            vm_printable_status,
            vm_ready,
            guest_ip,
            pod_ip,
            vmi_phase,
            node_name,
            node_internal_ip,
            virtctl_console,
            virtctl_vnc,
            virtctl_vnc_socks,
            vnc_subresource_path,
        });
    }

    if meta {
        Ok(axum::Json(serde_json::json!({
            "rows": rows,
            "kubevirt_available": true,
            "list_error": null,
        }))
        .into_response())
    } else {
        Ok(axum::Json(rows).into_response())
    }
}

async fn k8s_kubevirt_delete_vm(
    Extension(actor): Extension<RequestActor>,
    axum::extract::Path((namespace, name)): axum::extract::Path<(String, String)>,
    Query(q): Query<K8sOverviewQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Deleting a KubeVirt VM requires the operator or admin role.".into(),
        )
        .into());
    }
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    ensure_safe_name(&namespace, "namespace")?;
    ensure_safe_name(&name, "name")?;
    let args = vec![
        "delete".into(),
        "virtualmachine.kubevirt.io".into(),
        name.clone(),
        "-n".into(),
        namespace.clone(),
        "--wait=false".into(),
    ];
    let res = run_kubectl_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    if !res.ok {
        warn!(
            "kubectl delete virtualmachine failed (exit {}): {}{}",
            res.exit_code, res.stderr, res.stdout
        );
        return Err(AppError::from(LibvirtError::Operation(format!(
            "kubectl delete virtualmachine failed (exit {}): {}",
            res.exit_code,
            sanitize_kubectl_stderr(&res.stderr)
        ))));
    }
    Ok(Json(serde_json::json!({
        "deleted": true,
        "namespace": namespace,
        "name": name,
    })))
}

#[derive(Debug, Deserialize)]
struct KubeVirtLifecycleBody {
    action: String,
    #[serde(default)]
    context: Option<String>,
}

async fn k8s_kubevirt_vm_lifecycle(
    Extension(actor): Extension<RequestActor>,
    axum::extract::Path((namespace, name)): axum::extract::Path<(String, String)>,
    Json(body): Json<KubeVirtLifecycleBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Changing KubeVirt VM power state requires the operator or admin role.".into(),
        )
        .into());
    }
    let ctx = body.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    ensure_safe_name(&namespace, "namespace")?;
    ensure_safe_name(&name, "name")?;
    let action = body.action.trim().to_ascii_lowercase();
    let patch = match action.as_str() {
        "start" => Some(r#"{"spec":{"running":true}}"#),
        "stop" => Some(r#"{"spec":{"running":false}}"#),
        "restart" => None,
        other => {
            return Err(AppError::from(LibvirtError::Invalid(format!(
                "action must be start, stop, or restart (got \"{other}\")"
            ))));
        }
    };
    if action == "restart" {
        let stop_args = vec![
            "patch".into(),
            "virtualmachine.kubevirt.io".into(),
            name.clone(),
            "-n".into(),
            namespace.clone(),
            "--type".into(),
            "merge".into(),
            "-p".into(),
            r#"{"spec":{"running":false}}"#.into(),
        ];
        let stop_res = run_kubectl_timeout(&stop_args, KUBECTL_TIMEOUT_SECS, ctx).await?;
        if !stop_res.ok {
            warn!(
                "kubevirt restart (stop phase) failed (exit {}): {}{}",
                stop_res.exit_code, stop_res.stderr, stop_res.stdout
            );
            return Err(AppError::from(LibvirtError::Operation(format!(
                "kubevirt restart (stop phase) failed (exit {}): {}",
                stop_res.exit_code,
                sanitize_kubectl_stderr(&stop_res.stderr)
            ))));
        }
        let start_args = vec![
            "patch".into(),
            "virtualmachine.kubevirt.io".into(),
            name.clone(),
            "-n".into(),
            namespace.clone(),
            "--type".into(),
            "merge".into(),
            "-p".into(),
            r#"{"spec":{"running":true}}"#.into(),
        ];
        let start_res = run_kubectl_timeout(&start_args, KUBECTL_TIMEOUT_SECS, ctx).await?;
        if !start_res.ok {
            warn!(
                "kubevirt restart (start phase) failed (exit {}): {}{}",
                start_res.exit_code, start_res.stderr, start_res.stdout
            );
            return Err(AppError::from(LibvirtError::Operation(format!(
                "kubevirt restart (start phase) failed (exit {}): {}",
                start_res.exit_code,
                sanitize_kubectl_stderr(&start_res.stderr)
            ))));
        }
        return Ok(Json(serde_json::json!({
            "action": action,
            "namespace": namespace,
            "name": name,
            "ok": true,
        })));
    }
    let patch = patch.expect("start/stop always have patch");
    let args = vec![
        "patch".into(),
        "virtualmachine.kubevirt.io".into(),
        name.clone(),
        "-n".into(),
        namespace.clone(),
        "--type".into(),
        "merge".into(),
        "-p".into(),
        patch.into(),
    ];
    let res = run_kubectl_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    if !res.ok {
        warn!(
            "kubevirt {action} failed (exit {}): {}{}",
            res.exit_code, res.stderr, res.stdout
        );
        return Err(AppError::from(LibvirtError::Operation(format!(
            "kubevirt {action} failed (exit {}): {}",
            res.exit_code,
            sanitize_kubectl_stderr(&res.stderr)
        ))));
    }
    Ok(Json(serde_json::json!({
        "action": action,
        "namespace": namespace,
        "name": name,
        "ok": true,
    })))
}

#[derive(Debug, Deserialize)]
struct KubeVirtSpecBody {
    #[serde(default)]
    vcpus: Option<u32>,
    #[serde(default)]
    memory_mib: Option<u64>,
    #[serde(default)]
    context: Option<String>,
}

async fn k8s_kubevirt_vm_spec(
    Extension(actor): Extension<RequestActor>,
    axum::extract::Path((namespace, name)): axum::extract::Path<(String, String)>,
    Json(body): Json<KubeVirtSpecBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Editing KubeVirt VM specs requires the operator or admin role.".into(),
        )
        .into());
    }
    let ctx = body.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }
    ensure_safe_name(&namespace, "namespace")?;
    ensure_safe_name(&name, "name")?;
    if body.vcpus.is_none() && body.memory_mib.is_none() {
        return Err(AppError::from(LibvirtError::Invalid(
            "Provide vcpus and/or memory_mib".into(),
        )));
    }
    let mut domain = serde_json::Map::new();
    if let Some(cores) = body.vcpus.filter(|&c| c > 0) {
        domain.insert("cpu".into(), serde_json::json!({ "cores": cores }));
    }
    if let Some(mib) = body.memory_mib.filter(|&m| m > 0) {
        // Reject absurd sizes and use saturating arithmetic so the MiB→GiB
        // rounding below cannot overflow on attacker-controlled input.
        const MAX_MEMORY_MIB: u64 = 4 * 1024 * 1024; // 4 TiB
        if mib > MAX_MEMORY_MIB {
            return Err(AppError::from(LibvirtError::Invalid(format!(
                "memory_mib exceeds the maximum of {MAX_MEMORY_MIB} MiB (4 TiB)"
            ))));
        }
        let mem_gi = (mib.saturating_add(1023) / 1024).max(1);
        domain.insert(
            "resources".into(),
            serde_json::json!({
                "requests": { "memory": format!("{mem_gi}Gi") }
            }),
        );
    }
    let patch = serde_json::json!({
        "spec": {
            "template": {
                "spec": {
                    "domain": domain
                }
            }
        }
    });
    let patch_str = patch.to_string();
    let args = vec![
        "patch".into(),
        "virtualmachine.kubevirt.io".into(),
        name.clone(),
        "-n".into(),
        namespace.clone(),
        "--type".into(),
        "merge".into(),
        "-p".into(),
        patch_str,
    ];
    let res = run_kubectl_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    if !res.ok {
        warn!(
            "kubevirt spec patch failed (exit {}): {}{}",
            res.exit_code, res.stderr, res.stdout
        );
        return Err(AppError::from(LibvirtError::Operation(format!(
            "kubevirt spec patch failed (exit {}): {}",
            res.exit_code,
            sanitize_kubectl_stderr(&res.stderr)
        ))));
    }
    Ok(Json(serde_json::json!({
        "namespace": namespace,
        "name": name,
        "ok": true,
        "patch": patch,
    })))
}

async fn k8s_overview(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<K8sOverviewQuery>,
) -> Result<Json<K8sOverview>, AppError> {
    require_browser_session_for_host_insight(&actor)?;

    let host = collect_host_signals().await;
    let ctx = q.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }

    let (
        version_res,
        nodes_res,
        ns_res,
        pods_res,
        deploys_res,
        svc_res,
        sts_res,
        ds_res,
        cj_res,
        job_res,
        pv_res,
        pvc_res,
        sc_res,
        ing_res,
        apisvc_res,
        kvvm_res,
    ) = {
        let args_version = vec!["version".into()];
        let args_nodes = vec!["get".into(), "nodes".into()];
        let args_ns = vec!["get".into(), "namespaces".into()];
        let args_pods = vec!["get".into(), "pods".into(), "-A".into()];
        let args_deploy = vec!["get".into(), "deployments".into(), "-A".into()];
        let args_svc = vec!["get".into(), "services".into(), "-A".into()];
        let args_sts = vec!["get".into(), "statefulsets".into(), "-A".into()];
        let args_ds = vec!["get".into(), "daemonsets".into(), "-A".into()];
        let args_cj = vec!["get".into(), "cronjobs".into(), "-A".into()];
        let args_jobs = vec!["get".into(), "jobs".into(), "-A".into()];
        let args_pv = vec!["get".into(), "persistentvolumes".into()];
        let args_pvc = vec!["get".into(), "persistentvolumeclaims".into(), "-A".into()];
        let args_sc = vec!["get".into(), "storageclasses".into()];
        let args_ing = vec![
            "get".into(),
            "ingresses.networking.k8s.io".into(),
            "-A".into(),
        ];
        let args_apisvc = vec!["get".into(), "apiservices".into()];
        let args_kvvm = vec![
            "get".into(),
            "virtualmachines.kubevirt.io".into(),
            "-A".into(),
        ];

        tokio::join!(
            run_kubectl_json_timeout(&args_version, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_nodes, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_ns, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_pods, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_deploy, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_svc, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_sts, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_ds, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_cj, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_jobs, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_pv, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_pvc, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_sc, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_ing, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_apisvc, KUBECTL_TIMEOUT_SECS, ctx),
            run_kubectl_json_timeout(&args_kvvm, KUBECTL_TIMEOUT_SECS, ctx),
        )
    };

    let version = match version_res {
        Ok(v) => v
            .get("serverVersion")
            .and_then(|x| x.get("gitVersion"))
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        Err(e) => {
            warn!("k8s overview version error: {e}");
            "unknown".to_string()
        }
    };

    let nodes = nodes_res
        .as_ref()
        .ok()
        .and_then(|v| {
            v.get("items")
                .and_then(|x| x.as_array())
                .map(|a| a.to_vec())
        })
        .unwrap_or_default();

    let ready_nodes = nodes
        .iter()
        .filter(|n| {
            n.get("status")
                .and_then(|s| s.get("conditions"))
                .and_then(|c| c.as_array())
                .map(|conds| {
                    conds.iter().any(|c| {
                        c.get("type").and_then(|x| x.as_str()) == Some("Ready")
                            && c.get("status").and_then(|x| x.as_str()) == Some("True")
                    })
                })
                .unwrap_or(false)
        })
        .count();

    let namespaces = count_list_items(&ns_res);
    let pods = count_list_items(&pods_res);
    let deployments = count_list_items(&deploys_res);
    let services = count_list_items(&svc_res);

    let mut extra_resource_counts = BTreeMap::new();
    extra_resource_counts.insert("statefulsets".into(), count_list_items(&sts_res));
    extra_resource_counts.insert("daemonsets".into(), count_list_items(&ds_res));
    extra_resource_counts.insert("cronjobs".into(), count_list_items(&cj_res));
    extra_resource_counts.insert("jobs".into(), count_list_items(&job_res));
    extra_resource_counts.insert("persistentvolumes".into(), count_list_items(&pv_res));
    extra_resource_counts.insert("persistentvolumeclaims".into(), count_list_items(&pvc_res));
    extra_resource_counts.insert("storageclasses".into(), count_list_items(&sc_res));
    extra_resource_counts.insert("ingresses".into(), count_list_items(&ing_res));
    extra_resource_counts.insert("apiservices".into(), count_list_items(&apisvc_res));
    extra_resource_counts.insert(
        "kubevirt_virtualmachines".into(),
        count_list_items(&kvvm_res),
    );

    let (distribution, distribution_hints) = infer_cluster_distribution(&nodes, &host);

    Ok(Json(K8sOverview {
        version,
        nodes: nodes.len(),
        ready_nodes,
        namespaces,
        pods,
        deployments,
        services,
        distribution,
        distribution_hints,
        extra_resource_counts,
    }))
}

async fn k8s_action(
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<K8sActionRequest>,
) -> Result<Json<KubectlResult>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Cluster node/workload actions require the operator or admin role.".into(),
        )
        .into());
    }
    ensure_safe_name(&req.name, "name")?;
    let ctx = req.context.as_deref();
    if let Some(c) = ctx {
        ensure_k8s_context_name(c)?;
    }

    let args = match req.action {
        K8sAction::NodeCordon => vec!["cordon".into(), req.name],
        K8sAction::NodeUncordon => vec!["uncordon".into(), req.name],
        K8sAction::NodeDrain => vec![
            "drain".into(),
            req.name,
            "--ignore-daemonsets".into(),
            "--delete-emptydir-data".into(),
            "--force".into(),
        ],
        K8sAction::RolloutRestartDeployment => {
            let ns = safe_namespace(req.namespace.as_deref())?;
            vec![
                "rollout".into(),
                "restart".into(),
                format!("deployment/{}", req.name),
                "-n".into(),
                ns,
            ]
        }
        K8sAction::RolloutRestartStatefulSet => {
            let ns = safe_namespace(req.namespace.as_deref())?;
            vec![
                "rollout".into(),
                "restart".into(),
                format!("statefulset/{}", req.name),
                "-n".into(),
                ns,
            ]
        }
        K8sAction::RolloutRestartDaemonSet => {
            let ns = safe_namespace(req.namespace.as_deref())?;
            vec![
                "rollout".into(),
                "restart".into(),
                format!("daemonset/{}", req.name),
                "-n".into(),
                ns,
            ]
        }
        K8sAction::DeletePod => {
            let ns = safe_namespace(req.namespace.as_deref())?;
            vec!["delete".into(), "pod".into(), req.name, "-n".into(), ns]
        }
        K8sAction::DeleteJob => {
            let ns = safe_namespace(req.namespace.as_deref())?;
            vec!["delete".into(), "job".into(), req.name, "-n".into(), ns]
        }
        K8sAction::ScaleDeployment => {
            let ns = safe_namespace(req.namespace.as_deref())?;
            let replicas = req
                .replicas
                .ok_or_else(|| LibvirtError::Invalid("replicas is required".into()))?;
            vec![
                "scale".into(),
                format!("deployment/{}", req.name),
                format!("--replicas={replicas}"),
                "-n".into(),
                ns,
            ]
        }
        K8sAction::ScaleStatefulSet => {
            let ns = safe_namespace(req.namespace.as_deref())?;
            let replicas = req
                .replicas
                .ok_or_else(|| LibvirtError::Invalid("replicas is required".into()))?;
            vec![
                "scale".into(),
                format!("statefulset/{}", req.name),
                format!("--replicas={replicas}"),
                "-n".into(),
                ns,
            ]
        }
    };

    let res = run_kubectl_timeout(&args, KUBECTL_TIMEOUT_SECS, ctx).await?;
    if !res.ok {
        let msg = if res.stderr.trim().is_empty() {
            format!("k8s action failed: {}", res.command)
        } else {
            warn!("kubectl action failed ({}): {}", res.command, res.stderr);
            sanitize_kubectl_stderr(&res.stderr)
        };
        return Err(LibvirtError::Operation(msg).into());
    }
    Ok(Json(res))
}

/// Validates a value bound for `INSTALL_K3S_EXEC`/`INSTALL_K3S_VERSION`, the
/// env vars fed to the upstream `get.k3s.io` installer script, which this
/// daemon runs as root (`curl -sfL https://get.k3s.io | sh -`).
///
/// The installer word-splits/evaluates `INSTALL_K3S_EXEC` in its own shell
/// context to build the final `k3s server ...` invocation, so any shell
/// metacharacter here (`;`, `` ` ``, `$()`, `|`, `&`, quotes, newlines, ...)
/// is a potential root command-injection vector — rejecting only a few known-
/// bad characters (the previous approach) is exactly how that stays
/// exploitable. This is an allowlist instead: only the character set
/// legitimate values for each var actually use is permitted, everything else
/// is rejected outright, length limits aside.
fn validate_k3s_install_env_value(s: &str, label: &str, max: usize) -> Result<(), LibvirtError> {
    if s.is_empty() {
        return Err(LibvirtError::Invalid(format!("{label} must not be empty")));
    }
    if s.len() > max {
        return Err(LibvirtError::Invalid(format!(
            "{label} exceeds max length ({max} bytes)"
        )));
    }
    match label {
        // `INSTALL_K3S_EXEC`: an optional bare leading `server`/`agent` role
        // token (the daemon's fixed install command
        // `curl -sfL https://get.k3s.io | sh -` passes no positional role
        // argument, so per get.k3s.io's own convention that role must come
        // from the *start* of `INSTALL_K3S_EXEC` itself, e.g.
        // `server --disable=traefik --node-taint=CriticalAddonsOnly=true:NoExecute`),
        // followed by whitespace-separated k3s flags. Each remaining token
        // must be `--name` or `--name=value`; `name` is restricted to
        // `[a-zA-Z0-9-]+`, `value` to the character set real k3s flag values use
        // (alnum, `.`, `_`, `-`, `/`, `,`, `:`, `=` — the last so a value can
        // itself contain `key=value` pairs, as the taint example above does).
        "install_k3s_exec" => {
            for (i, token) in s.split_whitespace().enumerate() {
                if i == 0 && (token == "server" || token == "agent") {
                    continue;
                }
                let Some(rest) = token.strip_prefix("--") else {
                    return Err(LibvirtError::Invalid(format!(
                        "{label}: flag {token:?} must start with --"
                    )));
                };
                let (name, value) = match rest.split_once('=') {
                    Some((n, v)) => (n, Some(v)),
                    None => (rest, None),
                };
                let name_ok =
                    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
                if !name_ok {
                    return Err(LibvirtError::Invalid(format!(
                        "{label}: invalid flag name in {token:?}"
                    )));
                }
                if let Some(v) = value {
                    let value_ok = !v.is_empty()
                        && v.chars().all(|c| {
                            c.is_ascii_alphanumeric()
                                || matches!(c, '.' | '_' | '-' | '/' | ',' | ':' | '=')
                        });
                    if !value_ok {
                        return Err(LibvirtError::Invalid(format!(
                            "{label}: invalid flag value in {token:?}"
                        )));
                    }
                }
            }
        }
        // `INSTALL_K3S_VERSION`: a release tag/channel, e.g. `v1.30.3+k3s1`,
        // `latest`, `stable`.
        "install_k3s_version" => {
            let ok = s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-' | '_'));
            if !ok {
                return Err(LibvirtError::Invalid(format!(
                    "{label} contains characters outside the allowed set (alnum, '.', '+', '-', '_')"
                )));
            }
        }
        _ => {
            // Unused today, but keep a conservative fallback (no control chars)
            // rather than silently accepting anything for a future caller.
            if s.chars().any(|c| c.is_control()) {
                return Err(LibvirtError::Invalid(format!(
                    "{label} must not contain control characters"
                )));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct K3sInstallRequest {
    /// Passed to the installer as `INSTALL_K3S_EXEC` (flags for `k3s server` / agent).
    #[serde(default)]
    install_k3s_exec: Option<String>,
    /// Optional pin, e.g. `v1.30.3+k3s1` → `INSTALL_K3S_VERSION`.
    #[serde(default)]
    install_k3s_version: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct K3sUninstallRequest {
    /// `server` → `k3s-uninstall.sh`, `agent` → `k3s-agent-uninstall.sh`, `auto` picks an existing script.
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

async fn run_k3s_install_script(req: &K3sInstallRequest) -> Result<KubectlResult, LibvirtError> {
    if let Some(ref v) = req.install_k3s_version {
        validate_k3s_install_env_value(v, "install_k3s_version", INSTALL_K3S_VERSION_MAX)?;
    }
    if let Some(ref e) = req.install_k3s_exec {
        validate_k3s_install_env_value(e, "install_k3s_exec", INSTALL_K3S_EXEC_MAX)?;
    }

    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(k3s_install_shell());
    cmd.stdin(Stdio::null());
    if let Some(ref v) = req.install_k3s_version {
        cmd.env("INSTALL_K3S_VERSION", v);
    }
    if let Some(ref e) = req.install_k3s_exec {
        cmd.env("INSTALL_K3S_EXEC", e);
    }

    let mut command_text = k3s_install_shell();
    if req.install_k3s_version.is_some() {
        command_text.push_str(" (INSTALL_K3S_VERSION set)");
    }
    if let Some(e) = req.install_k3s_exec.as_ref() {
        command_text.push_str(&format!(" (INSTALL_K3S_EXEC {} bytes)", e.len()));
    }

    let output = timeout(Duration::from_secs(K3S_INSTALL_TIMEOUT_SECS), cmd.output())
        .await
        .map_err(|_| LibvirtError::Operation("k3s install timed out".into()))?
        .map_err(|e| LibvirtError::Operation(format!("failed to run k3s install: {e}")))?;

    Ok(KubectlResult {
        command: command_text,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
        ok: output.status.success(),
    })
}

fn resolve_k3s_uninstall_script(role: &str) -> Result<&'static str, LibvirtError> {
    match role {
        "server" => {
            if Path::new(K3S_UNINSTALL_SERVER).is_file() {
                Ok(K3S_UNINSTALL_SERVER)
            } else {
                Err(LibvirtError::Invalid(format!(
                    "k3s server uninstall script not found at {K3S_UNINSTALL_SERVER}"
                )))
            }
        }
        "agent" => {
            if Path::new(K3S_UNINSTALL_AGENT).is_file() {
                Ok(K3S_UNINSTALL_AGENT)
            } else {
                Err(LibvirtError::Invalid(format!(
                    "k3s agent uninstall script not found at {K3S_UNINSTALL_AGENT}"
                )))
            }
        }
        "auto" => {
            if Path::new(K3S_UNINSTALL_SERVER).is_file() {
                Ok(K3S_UNINSTALL_SERVER)
            } else if Path::new(K3S_UNINSTALL_AGENT).is_file() {
                Ok(K3S_UNINSTALL_AGENT)
            } else {
                Err(LibvirtError::Invalid(
                    "Neither k3s-uninstall.sh nor k3s-agent-uninstall.sh found under /usr/local/bin"
                        .into(),
                ))
            }
        }
        _ => Err(LibvirtError::Invalid(
            "role must be \"server\", \"agent\", or \"auto\"".into(),
        )),
    }
}

async fn run_k3s_uninstall_script(script: &str) -> Result<KubectlResult, LibvirtError> {
    let mut cmd = Command::new(script);
    cmd.stdin(Stdio::null());
    let output = timeout(
        Duration::from_secs(K3S_UNINSTALL_TIMEOUT_SECS),
        cmd.output(),
    )
    .await
    .map_err(|_| LibvirtError::Operation("k3s uninstall timed out".into()))?
    .map_err(|e| LibvirtError::Operation(format!("failed to run k3s uninstall: {e}")))?;

    Ok(KubectlResult {
        command: script.to_string(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
        ok: output.status.success(),
    })
}

/// Runs the upstream k3s install script on the **daemon host** (machina-daemon is root in the stock unit).
async fn k8s_k3s_install(
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<K3sInstallRequest>,
) -> Result<Json<KubectlResult>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "k3s install requires the operator or admin role.".into(),
        )
        .into());
    }

    if req.dry_run == Some(true) {
        let preview = serde_json::json!({
            "note": "dry_run only — no install was executed",
            "shell": k3s_install_shell(),
            "INSTALL_K3S_VERSION": req.install_k3s_version,
            "INSTALL_K3S_EXEC": req.install_k3s_exec,
        });
        return Ok(Json(KubectlResult {
            command: format!("(dry_run) {}", k3s_install_shell()),
            stdout: preview.to_string(),
            stderr: String::new(),
            exit_code: 0,
            ok: true,
        }));
    }

    info!(
        target: "machina_k8s",
        user = %actor.username,
        "k3s install via get.k3s.io"
    );

    let res = run_k3s_install_script(&req).await?;
    if !res.ok {
        let msg = if res.stderr.trim().is_empty() {
            format!(
                "k3s install failed (exit {}): {}",
                res.exit_code, res.command
            )
        } else {
            warn!(
                "k3s install script failed ({}): {}",
                res.command, res.stderr
            );
            sanitize_kubectl_stderr(&res.stderr)
        };
        return Err(LibvirtError::Operation(msg).into());
    }
    Ok(Json(res))
}

async fn k8s_k3s_uninstall(
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<K3sUninstallRequest>,
) -> Result<Json<KubectlResult>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "k3s uninstall requires the operator or admin role.".into(),
        )
        .into());
    }

    let role = req.role.as_deref().unwrap_or("auto").trim();
    if role.is_empty() {
        return Err(LibvirtError::Invalid("role cannot be empty".into()).into());
    }

    let script = resolve_k3s_uninstall_script(role)?;

    if req.dry_run == Some(true) {
        let preview = serde_json::json!({
            "note": "dry_run only — uninstall script was not executed",
            "script": script,
            "role": role,
        });
        return Ok(Json(KubectlResult {
            command: format!("(dry_run) {}", script),
            stdout: preview.to_string(),
            stderr: String::new(),
            exit_code: 0,
            ok: true,
        }));
    }

    info!(
        target: "machina_k8s",
        user = %actor.username,
        script = %script,
        "k3s uninstall"
    );

    let res = run_k3s_uninstall_script(script).await?;
    if !res.ok {
        let msg = if res.stderr.trim().is_empty() {
            format!(
                "k3s uninstall failed (exit {}): {}",
                res.exit_code, res.command
            )
        } else {
            warn!(
                "k3s uninstall script failed ({}): {}",
                res.command, res.stderr
            );
            sanitize_kubectl_stderr(&res.stderr)
        };
        return Err(LibvirtError::Operation(msg).into());
    }
    Ok(Json(res))
}

/// `server_ip` is written into the kubeconfig and passed to shell commands by
/// `cluster_bootstrap.rs`, so a denylist of whitespace/control characters isn't
/// enough. This validates the value is actually shaped like an IPv4/IPv6
/// address or a DNS hostname, rejecting `,`, quotes and anything else outside
/// that shape even for an otherwise valid-looking hostname.
fn validate_bootstrap_server_ip(s: &str) -> Result<(), LibvirtError> {
    if s.is_empty() {
        return Err(LibvirtError::Invalid("server_ip cannot be empty".into()));
    }
    if s.len() > BOOTSTRAP_SERVER_IP_MAX {
        return Err(LibvirtError::Invalid(format!(
            "server_ip exceeds max length ({BOOTSTRAP_SERVER_IP_MAX})"
        )));
    }
    if s.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    // Not a literal IP — validate as a DNS hostname: labels of alnum/`-`,
    // separated by `.`, no leading/trailing/double dots or hyphens at label
    // edges. This also covers the comma-injection case above, since `,` is
    // outside the allowed charset.
    let labels: Vec<&str> = s.split('.').collect();
    let hostname_ok = !s.starts_with('.')
        && !s.ends_with('.')
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if !hostname_ok {
        return Err(LibvirtError::Invalid(
            "server_ip must be a valid IPv4/IPv6 address or hostname".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ClusterBootstrapRequest {
    /// `full` | `k3s` | `cni` (alias `cilium`) | `metrics` | `kubevirt_cdi`
    #[serde(default)]
    phase: Option<String>,
    /// API advertise IP for the kubeconfig (optional; defaults to first address from `hostname -I`).
    #[serde(default)]
    server_ip: Option<String>,
    /// When `phase` is `full`, skip KubeVirt/CDI/virtctl after CNI/metrics.
    #[serde(default)]
    skip_kubevirt_cdi: Option<bool>,
    /// Install metrics-server on full/metrics phases (recommended on k3s labs).
    #[serde(default)]
    install_metrics_server: Option<bool>,
    /// Pod networking: `default` (k3s flannel + kube-proxy, the default) or
    /// `machina` (opt-in native eBPF machina-cni).
    #[serde(default)]
    cni: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

fn parse_bootstrap_cni(v: Option<&str>) -> Result<CniChoice, LibvirtError> {
    CniChoice::parse(v.unwrap_or("default"))
        .ok_or_else(|| LibvirtError::Invalid("cni must be \"default\" or \"machina\"".into()))
}

/// Runs phased cluster bootstrap in-process (`daemon/src/cluster_bootstrap.rs`).
async fn k8s_cluster_bootstrap(
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<ClusterBootstrapRequest>,
) -> Result<Json<KubectlResult>, AppError> {
    require_browser_session_for_host_insight(&actor)?;
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "cluster bootstrap requires the operator or admin role.".into(),
        )
        .into());
    }

    let phase = req.phase.as_deref().unwrap_or("full").trim();
    match phase {
        "full" | "k3s" | "cni" | "cilium" | "metrics" | "kubevirt_cdi" => {}
        _ => {
            return Err(LibvirtError::Invalid(
                "phase must be \"full\", \"k3s\", \"cni\", \"metrics\", or \"kubevirt_cdi\"".into(),
            )
            .into())
        }
    }

    if let Some(ip) = req.server_ip.as_ref() {
        validate_bootstrap_server_ip(ip)?;
    }
    let cni = parse_bootstrap_cni(req.cni.as_deref())?;

    if req.dry_run == Some(true) {
        let preview = serde_json::json!({
            "note": "dry_run — cluster_bootstrap Rust module not executed",
            "phase": phase,
            "server_ip": req.server_ip,
            "skip_kubevirt_cdi": req.skip_kubevirt_cdi,
            "install_metrics_server": req.install_metrics_server,
            "cni": cni.as_str(),
        });
        return Ok(Json(KubectlResult {
            command: format!("(dry_run) MACHINA_BOOTSTRAP_PHASE={phase}"),
            stdout: preview.to_string(),
            stderr: String::new(),
            exit_code: 0,
            ok: true,
        }));
    }

    info!(
        target: "machina_k8s",
        user = %actor.username,
        phase = %phase,
        "cluster bootstrap (Rust)"
    );

    let out = run_cluster_bootstrap(ClusterBootstrapParams {
        phase: phase.to_string(),
        server_ip: req.server_ip.clone(),
        skip_kubevirt_cdi: req.skip_kubevirt_cdi == Some(true),
        install_metrics_server: req.install_metrics_server != Some(false),
        cni,
    })
    .await?;

    Ok(Json(KubectlResult {
        command: out.command,
        stdout: out.stdout,
        stderr: out.stderr,
        exit_code: out.exit_code,
        ok: out.ok,
    }))
}

#[derive(Debug, Deserialize)]
struct K8sMetricsQuery {
    #[serde(default)]
    context: Option<String>,
}

fn top_row_json(row: machina_core::K8sTopRow) -> serde_json::Value {
    serde_json::json!({
        "name": row.name,
        "cpu": row.cpu,
        "cpu_percent": row.cpu_percent,
        "memory": row.memory,
        "memory_percent": row.memory_percent,
    })
}

/// Live cluster utilization via `kubectl top` (requires metrics-server).
async fn k8s_metrics(
    Query(q): Query<K8sMetricsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = q.context.as_deref();
    let nodes_res = run_kubectl_timeout(
        &["top".into(), "nodes".into(), "--no-headers".into()],
        25,
        ctx,
    )
    .await;
    let pods_res = run_kubectl_timeout(
        &[
            "top".into(),
            "pods".into(),
            "-A".into(),
            "--no-headers".into(),
        ],
        45,
        ctx,
    )
    .await;

    let mut nodes_top: Vec<serde_json::Value> = Vec::new();
    let mut nodes_error: Option<String> = None;
    if let Ok(res) = &nodes_res {
        if res.ok {
            for line in res.stdout.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(row) = machina_core::parse_kubectl_top_line(line) {
                    nodes_top.push(top_row_json(row));
                }
            }
        } else {
            warn!("kubectl top nodes failed: {}", res.stderr);
            nodes_error = Some(sanitize_kubectl_stderr(&res.stderr));
        }
    } else if let Err(e) = &nodes_res {
        nodes_error = Some(e.to_string());
    }

    let mut pods_top: Vec<serde_json::Value> = Vec::new();
    let mut pods_error: Option<String> = None;
    if let Ok(res) = &pods_res {
        if res.ok {
            for line in res.stdout.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(row) = machina_core::parse_kubectl_top_line(line) {
                    pods_top.push(top_row_json(row));
                }
            }
        } else {
            warn!("kubectl top pods failed: {}", res.stderr);
            pods_error = Some(sanitize_kubectl_stderr(&res.stderr));
        }
    } else if let Err(e) = &pods_res {
        pods_error = Some(e.to_string());
    }

    let metrics_available = !nodes_top.is_empty() || !pods_top.is_empty();
    crate::k8s_metrics_cache::record_k8s_metrics_probe(metrics_available);

    Ok(Json(serde_json::json!({
        "metrics_available": metrics_available,
        "metrics_server_hint": "Install metrics-server (cluster bootstrap phase metrics or helm) when kubectl top fails",
        "nodes_top": nodes_top,
        "pods_top": pods_top,
        "nodes_error": nodes_error,
        "pods_error": pods_error,
        "nodes_command": nodes_res.ok().map(|r| r.command),
        "pods_command": pods_res.ok().map(|r| r.command),
    })))
}

pub fn k8s_routes() -> Router<LibvirtManager> {
    let apply = Router::new()
        .route("/k8s/apply", post(k8s_apply_manifest))
        .layer(DefaultBodyLimit::max(
            KUBECTL_APPLY_MAX_MANIFEST_BYTES + 64 * 1024,
        ));

    Router::new()
        .merge(apply)
        .route("/k8s/overview", get(k8s_overview))
        .route("/k8s/environment", get(k8s_environment))
        .route("/k8s/contexts", get(k8s_contexts_list))
        .route(
            "/k8s/cluster-inventory/history",
            get(k8s_cluster_inventory_history),
        )
        .route("/k8s/cluster-inventory", get(k8s_cluster_inventory))
        .route("/k8s/metrics", get(k8s_metrics))
        .route("/k8s/nodes", get(k8s_nodes))
        .route("/k8s/namespaces", get(k8s_namespaces))
        .route("/k8s/pods", get(k8s_pods))
        .route("/k8s/deployments", get(k8s_deployments))
        .route("/k8s/services", get(k8s_services))
        .route("/k8s/statefulsets", get(k8s_statefulsets))
        .route("/k8s/daemonsets", get(k8s_daemonsets))
        .route("/k8s/jobs", get(k8s_jobs))
        .route("/k8s/cronjobs", get(k8s_cronjobs))
        .route("/k8s/ingresses", get(k8s_ingresses))
        .route(
            "/k8s/persistentvolumeclaims",
            get(k8s_persistentvolumeclaims),
        )
        .route("/k8s/persistentvolumes", get(k8s_persistentvolumes))
        .route("/k8s/storageclasses", get(k8s_storageclasses))
        .route("/k8s/events", get(k8s_events))
        .route("/k8s/logs", get(k8s_pod_logs))
        .route("/k8s/auth-can-i", post(k8s_auth_can_i))
        .route("/k8s/helm/releases", get(k8s_helm_releases))
        .route(
            "/k8s/kubevirt/virtualmachines",
            get(k8s_kubevirt_virtualmachines),
        )
        .route("/k8s/kubevirt/vm-summary", get(k8s_kubevirt_vm_summary))
        .route(
            "/k8s/kubevirt/virtualmachines/{namespace}/{name}",
            axum::routing::delete(k8s_kubevirt_delete_vm),
        )
        .route(
            "/k8s/kubevirt/virtualmachines/{namespace}/{name}/lifecycle",
            post(k8s_kubevirt_vm_lifecycle),
        )
        .route(
            "/k8s/kubevirt/virtualmachines/{namespace}/{name}/spec",
            post(k8s_kubevirt_vm_spec),
        )
        .route("/k8s/action", post(k8s_action))
        .route("/k8s/kata-deploy", post(k8s_kata_deploy))
        .route("/k8s/k3s/install", post(k8s_k3s_install))
        .route("/k8s/k3s/uninstall", post(k8s_k3s_uninstall))
        .route("/k8s/cluster-bootstrap", post(k8s_cluster_bootstrap))
}

#[cfg(test)]
mod k3s_install_env_value_tests {
    use super::*;

    #[test]
    fn accepts_standard_server_role_with_flags() {
        assert!(validate_k3s_install_env_value(
            "server --disable=traefik --node-taint=CriticalAddonsOnly=true:NoExecute",
            "install_k3s_exec",
            INSTALL_K3S_EXEC_MAX,
        )
        .is_ok());
    }

    #[test]
    fn accepts_agent_role_with_server_flag() {
        assert!(validate_k3s_install_env_value(
            "agent --server=https://1.2.3.4:6443",
            "install_k3s_exec",
            INSTALL_K3S_EXEC_MAX,
        )
        .is_ok());
    }

    #[test]
    fn rejects_shell_metacharacters_after_role_token() {
        assert!(validate_k3s_install_env_value(
            "server; rm -rf /",
            "install_k3s_exec",
            INSTALL_K3S_EXEC_MAX,
        )
        .is_err());
    }

    #[test]
    fn accepts_flags_only_with_no_leading_role_token() {
        assert!(validate_k3s_install_env_value(
            "--disable=traefik --write-kubeconfig-mode=644",
            "install_k3s_exec",
            INSTALL_K3S_EXEC_MAX,
        )
        .is_ok());
    }

    #[test]
    fn rejects_role_token_only_in_non_leading_position() {
        // "server" is only tolerated as the *first* token; elsewhere it must
        // still look like a flag.
        assert!(validate_k3s_install_env_value(
            "--disable=traefik server",
            "install_k3s_exec",
            INSTALL_K3S_EXEC_MAX,
        )
        .is_err());
    }
}

#[cfg(test)]
mod bootstrap_cni_tests {
    use super::*;

    #[test]
    fn cni_defaults_to_k3s_networking() {
        assert_eq!(parse_bootstrap_cni(None).unwrap(), CniChoice::Default);
        assert_eq!(
            parse_bootstrap_cni(Some("default")).unwrap(),
            CniChoice::Default
        );
        assert_eq!(
            parse_bootstrap_cni(Some("machina")).unwrap(),
            CniChoice::Machina
        );
        assert!(parse_bootstrap_cni(Some("flannel; rm -rf /")).is_err());
    }
}

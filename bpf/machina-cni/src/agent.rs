// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Per-node reconciler. Polls the API server through `kubectl` (k3s ships
//! one; honours `KUBECONFIG`), so there is no client-go/kube-rs dependency:
//!
//! 1. writes `/etc/cni/net.d/05-machina.conflist` from this node's podCIDR
//!    (the kubelet keeps the node NotReady until it exists);
//! 2. routes other nodes' podCIDRs via their InternalIP (direct routing) and
//!    masquerades pod traffic leaving the cluster CIDR;
//! 3. compiles NetworkPolicies / Services and syncs them into machina-bpfd.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use machina_bpf::api::{CniNodeConfig, Request};
use machina_bpf::BpfdClient;
use serde_json::{json, Value};

use crate::compile::{self, Inputs};

/// Routes we own carry this protocol number so stale ones can be found.
const ROUTE_PROTO: &str = "233";
const NFT_TABLE: &str = "machina_cni";
const SYSCTL_OVERRIDE: &str = "/etc/sysctl.d/99-zzz-machina-cni.conf";
/// NodePort replies leave a pod veth with the node address as source, which
/// the kernel drops as martian unless reverse-path filtering is off and local
/// sources are accepted. systemd-sysctl re-applies `*.rp_filter=2` to every
/// new link after the plugin ran, so the override has to live in sysctl.d.
const VETH_SYSCTLS: &[(&str, &str)] = &[("rp_filter", "0"), ("accept_local", "1")];
const RESYNC_EVERY: Duration = Duration::from_secs(30);

pub struct Config {
    pub node: String,
    pub kubectl: String,
    pub cluster_cidr: String,
    /// CNI config dirs; existing parents only, the first is always created.
    /// k3s reads its own dir, other runtimes /etc/cni/net.d.
    pub conf_dirs: Vec<String>,
    /// Where the plugin binary is installed (same rule as `conf_dirs`).
    pub bin_dirs: Vec<String>,
    pub mtu: u32,
    pub interval: Duration,
    /// Also enforce CiliumNetworkPolicy / CiliumClusterwideNetworkPolicy.
    pub cilium_policies: bool,
    /// IPv6 cluster CIDR; set to run dual-stack (nodes need an IPv6 podCIDR).
    pub cluster_cidr6: Option<String>,
    /// NodePort to remote backends: "snat" (default) or "dsr" (IPv4 IPIP).
    pub lb_mode: Option<String>,
    /// NodePort fast path for local backends at XDP on the uplink.
    pub xdp: bool,
    /// Start even when another CNI is already configured on this node.
    pub takeover: bool,
}

const MACHINA_CONFLIST: &str = "05-machina.conflist";

/// Another CNI owns this node and `MACHINA_CNI_TAKEOVER` is off.
#[derive(Debug)]
pub struct ForeignCni(pub Vec<String>);

impl std::fmt::Display for ForeignCni {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "another CNI is already configured ({}); refusing to replace it. \
             Install the cluster without its bundled CNI (k3s: --flannel-backend=none \
             --disable-network-policy --disable-kube-proxy) or set MACHINA_CNI_TAKEOVER=1",
            self.0.join(", ")
        )
    }
}

impl std::error::Error for ForeignCni {}

/// CNI configs in the usable conf dirs that this agent did not write.
pub fn foreign_cni_configs(conf_dirs: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for dir in usable_dirs(conf_dirs) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_cfg =
                name.ends_with(".conf") || name.ends_with(".conflist") || name.ends_with(".json");
            if is_cfg && name != MACHINA_CONFLIST && e.path().is_file() {
                found.push(e.path().display().to_string());
            }
        }
    }
    found.sort();
    found
}

fn truthy(v: &str) -> bool {
    matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

const CILIUM_RESOURCES: &str =
    "ciliumnetworkpolicies.cilium.io,ciliumclusterwidenetworkpolicies.cilium.io";

fn list(v: String) -> Vec<String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// The first dir is always used; the rest only when their parent exists.
fn usable_dirs(dirs: &[String]) -> Vec<String> {
    dirs.iter()
        .enumerate()
        .filter(|(i, d)| *i == 0 || std::path::Path::new(d).parent().is_some_and(|p| p.is_dir()))
        .map(|(_, d)| d.clone())
        .collect()
}

/// Copy this executable into each CNI bin dir when missing or different.
fn install_plugin(bin_dirs: &[String]) {
    let Ok(me) = std::env::current_exe() else {
        return;
    };
    let Ok(bytes) = std::fs::read(&me) else {
        return;
    };
    for dir in usable_dirs(bin_dirs) {
        let dst = std::path::Path::new(&dir).join("machina-cni");
        if dst == me || std::fs::read(&dst).ok().as_deref() == Some(bytes.as_slice()) {
            continue;
        }
        let tmp = dst.with_extension("tmp");
        let res = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(&tmp, &bytes))
            .and_then(|_| {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            })
            .and_then(|_| std::fs::rename(&tmp, &dst));
        match res {
            Ok(()) => tracing::info!("installed CNI plugin {}", dst.display()),
            Err(e) => tracing::warn!("install {}: {e}", dst.display()),
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        let env = |k: &str, d: &str| {
            std::env::var(k)
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| d.to_string())
        };
        // kubelet registers the node under the lowercased hostname.
        let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        Self {
            node: env("NODE_NAME", &host),
            kubectl: env("MACHINA_CNI_KUBECTL", "kubectl"),
            cluster_cidr: env("MACHINA_CNI_CLUSTER_CIDR", "10.42.0.0/16"),
            conf_dirs: list(env(
                "MACHINA_CNI_CONF_DIRS",
                "/etc/cni/net.d,/var/lib/rancher/k3s/agent/etc/cni/net.d",
            )),
            bin_dirs: list(env(
                "MACHINA_CNI_BIN_DIRS",
                "/opt/cni/bin,/var/lib/rancher/k3s/data/current/bin",
            )),
            mtu: env("MACHINA_CNI_MTU", "1500").parse().unwrap_or(1500),
            interval: Duration::from_secs(
                env("MACHINA_CNI_INTERVAL_SECS", "3")
                    .parse()
                    .unwrap_or(3)
                    .max(1),
            ),
            cilium_policies: truthy(&env("MACHINA_CNI_CILIUM_POLICIES", "0")),
            cluster_cidr6: Some(env("MACHINA_CNI_CLUSTER_CIDR6", "")).filter(|v| !v.is_empty()),
            lb_mode: Some(env("MACHINA_CNI_LB_MODE", "")).filter(|v| !v.is_empty()),
            xdp: truthy(&env("MACHINA_CNI_XDP", "0")),
            takeover: truthy(&env("MACHINA_CNI_TAKEOVER", "0")),
        }
    }
}

fn kubectl(cfg: &Config, args: &[&str]) -> Result<Value> {
    let out = Command::new(&cfg.kubectl)
        .args(args)
        .output()
        .with_context(|| format!("run {}", cfg.kubectl))?;
    if !out.status.success() {
        bail!(
            "kubectl {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

fn items(v: &Value) -> Vec<Value> {
    v["items"].as_array().cloned().unwrap_or_default()
}

fn run_cmd(prog: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(prog)
        .args(args)
        .output()
        .with_context(|| format!("run {prog}"))?;
    if !out.status.success() {
        bail!(
            "{prog} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn internal_ips(node: &Value) -> Vec<String> {
    node["status"]["addresses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["type"] == "InternalIP")
        .filter_map(|a| a["address"].as_str().map(String::from))
        .collect()
}

fn internal_ip(node: &Value, v6: bool) -> Option<String> {
    internal_ips(node)
        .into_iter()
        .find(|s| s.contains(':') == v6)
}

fn pod_cidr(node: &Value, v6: bool) -> Option<String> {
    let cidrs = node["spec"]["podCIDRs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c.as_str());
    cidrs
        .chain(node["spec"]["podCIDR"].as_str())
        .find(|c| c.contains(':') == v6)
        .map(String::from)
}

/// Interface that holds `addr` (the uplink used for NodePort DNAT).
fn iface_with_addr(addr: &str) -> Option<String> {
    let out = run_cmd("ip", &["-j", "addr", "show"]).ok()?;
    let v: Value = serde_json::from_str(&out).ok()?;
    v.as_array()?.iter().find_map(|l| {
        let has = l["addr_info"]
            .as_array()?
            .iter()
            .any(|a| a["local"].as_str() == Some(addr));
        has.then(|| l["ifname"].as_str().map(String::from))
            .flatten()
    })
}

pub fn conflist(subnet: &str, subnet6: Option<&str>, mtu: u32) -> String {
    let mut plugin = json!({ "type": "machina-cni", "subnet": subnet, "mtu": mtu });
    if let Some(s6) = subnet6 {
        plugin["subnet6"] = json!(s6);
    }
    serde_json::to_string_pretty(&json!({
        "cniVersion": "1.0.0",
        "name": "machina",
        "plugins": [plugin]
    }))
    .unwrap_or_default()
}

fn write_if_changed(path: &str, body: &str) -> Result<bool> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(body) {
        return Ok(false);
    }
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// Routes to other nodes' podCIDRs; removes ours that no longer apply.
fn sync_routes(want: &BTreeSet<(String, String)>, v6: bool) -> Result<()> {
    let fam = if v6 { "-6" } else { "-4" };
    let out = run_cmd("ip", &["-j", fam, "route", "show", "proto", ROUTE_PROTO])?;
    let have: Vec<Value> = serde_json::from_str(&out).unwrap_or_default();
    for r in &have {
        let dst = r["dst"].as_str().unwrap_or("");
        let gw = r["gateway"].as_str().unwrap_or("");
        if !want.contains(&(dst.to_string(), gw.to_string())) {
            let _ = run_cmd("ip", &[fam, "route", "del", dst, "proto", ROUTE_PROTO]);
        }
    }
    for (cidr, gw) in want {
        if let Err(e) = run_cmd(
            "ip",
            &[
                fam,
                "route",
                "replace",
                cidr,
                "via",
                gw,
                "proto",
                ROUTE_PROTO,
            ],
        ) {
            tracing::warn!("route {cidr} via {gw}: {e:#}");
        }
    }
    Ok(())
}

pub fn nft_rules(cluster_cidr: &str, cluster_cidr6: Option<&str>) -> String {
    let mut nat = format!("    ip saddr {cluster_cidr} ip daddr != {cluster_cidr} masquerade\n");
    let mut fwd =
        format!("    ip saddr {cluster_cidr} accept\n    ip daddr {cluster_cidr} accept\n");
    if let Some(c6) = cluster_cidr6 {
        nat.push_str(&format!(
            "    ip6 saddr {c6} ip6 daddr != {c6} masquerade\n"
        ));
        fwd.push_str(&format!(
            "    ip6 saddr {c6} accept\n    ip6 daddr {c6} accept\n"
        ));
    }
    format!(
        "table inet {NFT_TABLE} {{\n  chain postrouting {{\n    type nat hook postrouting priority srcnat; policy accept;\n\
         {nat}  }}\n  chain forward {{\n    type filter hook forward priority filter; policy accept;\n\
         {fwd}  }}\n}}\n"
    )
}

pub fn sysctl_override() -> String {
    let mut s = String::from("# Written by machina-cni agent.\n");
    for (k, v) in VETH_SYSCTLS {
        s.push_str(&format!("net.ipv4.conf.mc*.{k} = {v}\n"));
    }
    s
}

/// Pin the veth sysctls on existing pod veths (new ones get the sysctl.d override).
fn pin_veth_sysctls() {
    let Ok(dir) = std::fs::read_dir("/proc/sys/net/ipv4/conf") else {
        return;
    };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("mc") {
            continue;
        }
        for (k, v) in VETH_SYSCTLS {
            let path = e.path().join(k);
            if std::fs::read_to_string(&path)
                .map(|c| c.trim() != *v)
                .unwrap_or(false)
            {
                let _ = std::fs::write(&path, v);
            }
        }
    }
}

/// IPv6 forwarding makes `accept_ra=1` interfaces ignore router adverts, which
/// would drop a SLAAC uplink's default route; bump those to 2 first.
fn enable_v6_forwarding() {
    if let Ok(dir) = std::fs::read_dir("/proc/sys/net/ipv6/conf") {
        for e in dir.flatten() {
            let ra = e.path().join("accept_ra");
            if std::fs::read_to_string(&ra).is_ok_and(|v| v.trim() == "1") {
                let _ = std::fs::write(&ra, "2");
            }
        }
    }
    let _ = std::fs::write("/proc/sys/net/ipv6/conf/all/forwarding", "1");
}

fn ensure_masquerade(cluster_cidr: &str, cluster_cidr6: Option<&str>) -> Result<()> {
    let _ = std::fs::write("/proc/sys/net/ipv4/ip_forward", "1");
    if cluster_cidr6.is_some() {
        enable_v6_forwarding();
    }
    let _ = run_cmd("nft", &["delete", "table", "ip", NFT_TABLE]);
    let _ = run_cmd("nft", &["delete", "table", "inet", NFT_TABLE]);
    let path = std::env::temp_dir().join("machina-cni.nft");
    std::fs::write(&path, nft_rules(cluster_cidr, cluster_cidr6))?;
    run_cmd("nft", &["-f", &path.display().to_string()]).map(|_| ())
}

#[derive(PartialEq)]
struct NodeSetup {
    pod_cidr: String,
    pod_cidr6: Option<String>,
    node_addr: String,
    node_addr6: Option<String>,
}

pub async fn run(cfg: Config) -> Result<()> {
    tracing::info!(node = %cfg.node, cluster_cidr = %cfg.cluster_cidr, "machina-cni agent starting");
    let foreign = foreign_cni_configs(&cfg.conf_dirs);
    if !foreign.is_empty() {
        if !cfg.takeover {
            return Err(ForeignCni(foreign).into());
        }
        tracing::warn!(configs = ?foreign, "MACHINA_CNI_TAKEOVER set: replacing the existing CNI");
    }
    let bpfd = BpfdClient::from_env();
    let mut setup: Option<NodeSetup> = None;
    let mut last_pushed: Option<(machina_bpf::api::CniState, Instant)> = None;
    let mut last_warnings: Vec<String> = Vec::new();
    let mut tick = tokio::time::interval(cfg.interval);
    loop {
        tick.tick().await;
        if let Err(e) = reconcile(
            &cfg,
            &bpfd,
            &mut setup,
            &mut last_pushed,
            &mut last_warnings,
        )
        .await
        {
            tracing::warn!("reconcile: {e:#}");
        }
    }
}

async fn reconcile(
    cfg: &Config,
    bpfd: &BpfdClient,
    setup: &mut Option<NodeSetup>,
    last_pushed: &mut Option<(machina_bpf::api::CniState, Instant)>,
    last_warnings: &mut Vec<String>,
) -> Result<()> {
    let list = tokio::task::block_in_place(|| {
        kubectl(
            cfg,
            &[
                "get",
                "nodes,namespaces,pods,networkpolicies,services,endpointslices",
                "-A",
                "-o",
                "json",
            ],
        )
    })?;
    let mut cilium_warning = None;
    let cilium_policies = if cfg.cilium_policies {
        match tokio::task::block_in_place(|| {
            kubectl(cfg, &["get", CILIUM_RESOURCES, "-A", "-o", "json"])
        }) {
            Ok(v) => items(&v),
            Err(e) if format!("{e:#}").contains("doesn't have a resource type") => {
                cilium_warning = Some(
                    "MACHINA_CNI_CILIUM_POLICIES is set but the cilium.io CRDs are not installed"
                        .into(),
                );
                Vec::new()
            }
            Err(e) => return Err(e.context("list Cilium policies")),
        }
    } else {
        Vec::new()
    };
    let all = items(&list);
    let of_kind =
        |k: &str| -> Vec<Value> { all.iter().filter(|i| i["kind"] == k).cloned().collect() };
    let nodes = of_kind("Node");
    let me = nodes
        .iter()
        .find(|n| n["metadata"]["name"].as_str() == Some(cfg.node.as_str()))
        .ok_or_else(|| anyhow!("node {} not registered yet", cfg.node))?;

    // Node bring-up: CNI config, bpfd node config, masquerade.
    let dual = cfg.cluster_cidr6.is_some();
    let want = NodeSetup {
        pod_cidr: pod_cidr(me, false)
            .ok_or_else(|| anyhow!("node {} has no podCIDR yet", cfg.node))?,
        pod_cidr6: if dual {
            Some(
                pod_cidr(me, true)
                    .ok_or_else(|| anyhow!("node {} has no IPv6 podCIDR yet", cfg.node))?,
            )
        } else {
            None
        },
        node_addr: internal_ip(me, false)
            .ok_or_else(|| anyhow!("node {} has no InternalIP", cfg.node))?,
        node_addr6: internal_ip(me, true).filter(|_| dual),
    };
    if setup.as_ref() != Some(&want) {
        install_plugin(&cfg.bin_dirs);
        for dir in usable_dirs(&cfg.conf_dirs) {
            let path = format!("{dir}/{MACHINA_CONFLIST}");
            if write_if_changed(
                &path,
                &conflist(&want.pod_cidr, want.pod_cidr6.as_deref(), cfg.mtu),
            )? {
                tracing::info!(
                    "wrote {path} (podCIDR {} {:?})",
                    want.pod_cidr,
                    want.pod_cidr6
                );
            }
        }
        ensure_masquerade(&cfg.cluster_cidr, cfg.cluster_cidr6.as_deref())?;
        if let Err(e) = write_if_changed(SYSCTL_OVERRIDE, &sysctl_override()) {
            tracing::warn!("write {SYSCTL_OVERRIDE}: {e:#}");
        }
        let uplink = iface_with_addr(&want.node_addr);
        let config = CniNodeConfig {
            node_addr: want.node_addr.clone(),
            node_addr6: want.node_addr6.clone(),
            uplink: uplink.clone(),
            lb_mode: cfg.lb_mode.clone(),
            xdp: cfg.xdp,
        };
        bpfd.call(&Request::CniConfigure { config })
            .await
            .context("machina-bpfd cni_configure")?;
        tracing::info!(node_addr = %want.node_addr, node_addr6 = ?want.node_addr6, uplink = ?uplink,
            lb_mode = ?cfg.lb_mode, xdp = cfg.xdp, "node datapath configured");
        *setup = Some(want);
    }

    let others: Vec<&Value> = nodes
        .iter()
        .filter(|n| n["metadata"]["name"].as_str() != Some(cfg.node.as_str()))
        .collect();
    for v6 in [false, true].into_iter().filter(|v6| !v6 || dual) {
        let routes: BTreeSet<(String, String)> = others
            .iter()
            .filter_map(|n| Some((pod_cidr(n, v6)?, internal_ip(n, v6)?)))
            .collect();
        tokio::task::block_in_place(|| sync_routes(&routes, v6))?;
    }
    pin_veth_sysctls();
    let node_ips: BTreeMap<String, Vec<String>> = nodes
        .iter()
        .filter_map(|n| Some((n["metadata"]["name"].as_str()?.to_string(), internal_ips(n))))
        .collect();

    let pods = compile::pods(&of_kind("Pod"));
    let mut compiled = compile::compile(&Inputs {
        pods: &pods,
        namespaces: &of_kind("Namespace"),
        policies: &of_kind("NetworkPolicy"),
        services: &of_kind("Service"),
        endpoint_slices: &of_kind("EndpointSlice"),
        cilium_policies: &cilium_policies,
        node_ips: &node_ips,
        node: &cfg.node,
    });
    compiled.warnings.extend(cilium_warning);
    if compiled.warnings != *last_warnings {
        for w in &compiled.warnings {
            tracing::warn!("{w}");
        }
        *last_warnings = compiled.warnings.clone();
    }
    let due = match last_pushed {
        Some((st, at)) => *st != compiled.state || at.elapsed() >= RESYNC_EVERY,
        None => true,
    };
    if due {
        bpfd.call(&Request::CniSync {
            state: compiled.state.clone(),
        })
        .await
        .context("machina-bpfd cni_sync")?;
        tracing::debug!(
            identities = compiled.state.identities.len(),
            policy = compiled.state.policy.len(),
            services = compiled.state.services.len(),
            "synced"
        );
        *last_pushed = Some((compiled.state, Instant::now()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_cni_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("net.d");
        std::fs::create_dir_all(&dir).unwrap();
        let dirs = vec![
            dir.display().to_string(),
            "/nonexistent/parent/net.d".to_string(),
        ];
        assert!(foreign_cni_configs(&dirs).is_empty());
        std::fs::write(dir.join(MACHINA_CONFLIST), "{}").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        assert!(foreign_cni_configs(&dirs).is_empty());
        std::fs::write(dir.join("10-flannel.conflist"), "{}").unwrap();
        std::fs::write(dir.join("87-podman.conf"), "{}").unwrap();
        let found = foreign_cni_configs(&dirs);
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("10-flannel.conflist"));
        assert!(ForeignCni(found)
            .to_string()
            .contains("MACHINA_CNI_TAKEOVER=1"));
    }

    #[test]
    fn node_fields() {
        let n = json!({
            "spec": {"podCIDRs": ["fd00::/64", "10.42.3.0/24"]},
            "status": {"addresses": [{"type": "Hostname", "address": "n3"}, {"type": "InternalIP", "address": "192.168.1.13"}]}
        });
        assert_eq!(pod_cidr(&n, false).as_deref(), Some("10.42.3.0/24"));
        assert_eq!(pod_cidr(&n, true).as_deref(), Some("fd00::/64"));
        assert_eq!(internal_ip(&n, false).as_deref(), Some("192.168.1.13"));
        assert_eq!(internal_ip(&n, true), None);
    }

    #[test]
    fn generated_config() {
        let c: Value = serde_json::from_str(&conflist("10.42.0.0/24", None, 1450)).unwrap();
        assert_eq!(c["plugins"][0]["type"], "machina-cni");
        assert_eq!(c["plugins"][0]["subnet"], "10.42.0.0/24");
        assert!(c["plugins"][0].get("subnet6").is_none());
        let c: Value =
            serde_json::from_str(&conflist("10.42.0.0/24", Some("fd42:0:0:1::/64"), 1450)).unwrap();
        assert_eq!(c["plugins"][0]["subnet6"], "fd42:0:0:1::/64");
        let nft = nft_rules("10.42.0.0/16", None);
        assert!(nft.starts_with("table inet "));
        assert!(nft.contains("ip saddr 10.42.0.0/16 ip daddr != 10.42.0.0/16 masquerade"));
        assert!(!nft.contains("ip6"));
        let nft = nft_rules("10.42.0.0/16", Some("fd42::/56"));
        assert!(nft.contains("ip6 saddr fd42::/56 ip6 daddr != fd42::/56 masquerade"));
        let sysctl = sysctl_override();
        assert!(sysctl.contains("net.ipv4.conf.mc*.rp_filter = 0"));
        assert!(sysctl.contains("net.ipv4.conf.mc*.accept_local = 1"));
    }
}

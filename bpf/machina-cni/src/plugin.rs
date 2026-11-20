// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! CNI 1.0 plugin. Pods get a veth pair, a /32 (and, dual-stack, a /128)
//! address and default routes via the link-local gateways 169.254.1.1 /
//! fe80::1 (permanent neighbour entries for the host-side MAC), so there is
//! no bridge and no per-node gateway address.

use std::io::Read;
use std::net::IpAddr;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use machina_bpf::api::{CniEndpoint, Request};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ipam::{Ipam, Subnet};

pub const GATEWAY: &str = "169.254.1.1";
pub const GATEWAY6: &str = "fe80::1";
/// Fixed host-side veth MAC. Set explicitly so udev (MACAddressPolicy=persistent)
/// does not re-randomise it after the pod's permanent gateway neighbour entry
/// was written; every veth is point-to-point, so sharing one MAC is fine.
pub const HOST_MAC: &str = "ee:ee:ee:ee:ee:ee";
const IPAM_ROOT: &str = "/var/lib/machina-cni/ipam";
const SUPPORTED: &[&str] = &["0.3.1", "0.4.0", "1.0.0"];

#[derive(Debug, Deserialize)]
struct NetConf {
    #[serde(rename = "cniVersion", default = "default_version")]
    cni_version: String,
    #[serde(default)]
    mtu: Option<u32>,
    /// Node podCIDR (written by `machina-cni agent`).
    subnet: String,
    /// IPv6 podCIDR on dual-stack nodes.
    #[serde(default)]
    subnet6: Option<String>,
    #[serde(default)]
    ipam_dir: Option<String>,
}

fn default_version() -> String {
    "1.0.0".into()
}

struct Args {
    command: String,
    container_id: String,
    netns: String,
    ifname: String,
    pod: Option<String>,
}

impl Args {
    fn from_env() -> Result<Self> {
        let get = |k: &str| std::env::var(k).unwrap_or_default();
        let pod = get("CNI_ARGS")
            .split(';')
            .filter_map(|kv| kv.split_once('='))
            .fold((None, None), |(ns, name), (k, v)| match k {
                "K8S_POD_NAMESPACE" => (Some(v.to_string()), name),
                "K8S_POD_NAME" => (ns, Some(v.to_string())),
                _ => (ns, name),
            });
        Ok(Self {
            command: get("CNI_COMMAND"),
            container_id: get("CNI_CONTAINERID"),
            netns: get("CNI_NETNS"),
            ifname: {
                let i = get("CNI_IFNAME");
                if i.is_empty() {
                    "eth0".into()
                } else {
                    i
                }
            },
            pod: match pod {
                (Some(ns), Some(n)) => Some(format!("{ns}/{n}")),
                _ => None,
            },
        })
    }
}

/// Host-side veth name: `mc` + 12 hex digits of FNV-1a(container id, ifname).
pub fn host_veth(container_id: &str, ifname: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in container_id.bytes().chain([0]).chain(ifname.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("mc{:012x}", h & 0xffff_ffff_ffff)
}

fn sh(prog: &str, args: &[&str]) -> Result<String> {
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

fn ip(args: &[&str]) -> Result<String> {
    sh("ip", args)
}

fn ns_ip(netns: &str, args: &[&str]) -> Result<String> {
    let net = format!("--net={netns}");
    let mut full = vec![net.as_str(), "ip"];
    full.extend_from_slice(args);
    sh("nsenter", &full)
}

fn link_mac(json_out: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json_out).ok()?;
    v.get(0)?.get("address")?.as_str().map(String::from)
}

fn emit_error(version: &str, code: u32, msg: &str) -> i32 {
    println!(
        "{}",
        json!({ "cniVersion": version, "code": code, "msg": msg })
    );
    1
}

pub fn run() -> i32 {
    let args = match Args::from_env() {
        Ok(a) => a,
        Err(e) => return emit_error("1.0.0", 4, &format!("{e:#}")),
    };
    if args.command == "VERSION" {
        println!(
            "{}",
            json!({ "cniVersion": "1.0.0", "supportedVersions": SUPPORTED })
        );
        return 0;
    }
    let mut stdin = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut stdin) {
        return emit_error("1.0.0", 4, &format!("read config: {e}"));
    }
    let conf: NetConf = match serde_json::from_str(&stdin) {
        Ok(c) => c,
        Err(e) => return emit_error("1.0.0", 6, &format!("decode network config: {e}")),
    };
    let version = conf.cni_version.clone();
    let res = match args.command.as_str() {
        "ADD" => add(&conf, &args).map(|v| println!("{v}")),
        "DEL" => del(&conf, &args),
        "CHECK" => check(&args),
        other => Err(anyhow!("unsupported CNI_COMMAND {other}")),
    };
    match res {
        Ok(()) => 0,
        Err(e) => emit_error(&version, 999, &format!("{e:#}")),
    }
}

/// One IPAM per configured family (IPv4 first).
fn ipams(conf: &NetConf) -> Result<Vec<Ipam>> {
    let root = Path::new(conf.ipam_dir.as_deref().unwrap_or(IPAM_ROOT));
    let mut out = vec![Ipam::new(root, Subnet::parse(&conf.subnet)?)?];
    if let Some(s6) = conf.subnet6.as_deref().filter(|s| !s.is_empty()) {
        out.push(Ipam::new(root, Subnet::parse(s6)?)?);
    }
    Ok(out)
}

fn host_prefix(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(a) => format!("{a}/32"),
        IpAddr::V6(a) => format!("{a}/128"),
    }
}

fn bpfd(req: Request) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(machina_bpf::BpfdClient::from_env().call(&req))?;
    Ok(())
}

fn add(conf: &NetConf, a: &Args) -> Result<Value> {
    if a.netns.is_empty() || a.container_id.is_empty() {
        bail!("CNI_NETNS and CNI_CONTAINERID are required for ADD");
    }
    let ipams = ipams(conf)?;
    let host = host_veth(&a.container_id, &a.ifname);
    let mut addrs = Vec::new();
    let mut res = Ok(());
    for ipam in &ipams {
        match ipam.allocate(&a.container_id, &a.ifname) {
            Ok(ip) => addrs.push(ip),
            Err(e) => {
                res = Err(e);
                break;
            }
        }
    }
    let res = res.and_then(|_| plumb(conf, a, &host, &addrs));
    if res.is_err() {
        let _ = ip(&["link", "del", &host]);
        for ipam in &ipams {
            ipam.release(&a.container_id, &a.ifname);
        }
    }
    let (pod_mac, host_mac) = res?;

    // The pod is reachable through the kernel route even if bpfd is down;
    // bpfd adds same-node redirect and NetworkPolicy on top.
    for addr in &addrs {
        if let Err(e) = bpfd(Request::CniAddEndpoint {
            endpoint: CniEndpoint {
                ip: addr.to_string(),
                host_iface: host.clone(),
                pod_mac: pod_mac.clone(),
                host_mac: host_mac.clone(),
                pod: a.pod.clone(),
                container_id: Some(a.container_id.clone()),
            },
        }) {
            eprintln!("machina-cni: machina-bpfd registration failed (policy not enforced): {e:#}");
        }
    }

    let mut ips = Vec::new();
    let mut routes = Vec::new();
    for addr in &addrs {
        let (gw, dst) = if addr.is_ipv6() {
            (GATEWAY6, "::/0")
        } else {
            (GATEWAY, "0.0.0.0/0")
        };
        ips.push(json!({ "address": host_prefix(*addr), "gateway": gw, "interface": 1 }));
        routes.push(json!({ "dst": dst, "gw": gw }));
    }
    Ok(json!({
        "cniVersion": conf.cni_version,
        "interfaces": [
            { "name": host, "mac": host_mac },
            { "name": a.ifname, "mac": pod_mac, "sandbox": a.netns },
        ],
        "ips": ips,
        "routes": routes,
        "dns": {},
    }))
}

/// Create the veth pair and configure both ends. Returns (pod MAC, host MAC).
fn plumb(conf: &NetConf, a: &Args, host: &str, addrs: &[IpAddr]) -> Result<(String, String)> {
    let mtu = conf.mtu.unwrap_or(1500).to_string();
    // Created inside the pod netns with the peer moved to PID 1's (host) netns.
    if ip(&["link", "show", host]).is_err() {
        ns_ip(
            &a.netns,
            &[
                "link", "add", &a.ifname, "mtu", &mtu, "type", "veth", "peer", "name", host,
                "address", HOST_MAC, "mtu", &mtu, "netns", "1",
            ],
        )?;
    }
    ip(&["link", "set", host, "address", HOST_MAC])?;
    let host_mac =
        link_mac(&ip(&["-j", "link", "show", host])?).ok_or_else(|| anyhow!("no MAC on {host}"))?;
    ns_ip(&a.netns, &["link", "set", "lo", "up"])?;
    for addr in addrs {
        let cidr = host_prefix(*addr);
        if addr.is_ipv6() {
            ns_ip(
                &a.netns,
                &["-6", "addr", "replace", &cidr, "dev", &a.ifname, "nodad"],
            )?;
        } else {
            ns_ip(&a.netns, &["addr", "replace", &cidr, "dev", &a.ifname])?;
        }
    }
    ns_ip(&a.netns, &["link", "set", &a.ifname, "up"])?;
    for addr in addrs {
        if addr.is_ipv6() {
            ns_ip(
                &a.netns,
                &[
                    "-6", "route", "replace", "default", "via", GATEWAY6, "dev", &a.ifname,
                ],
            )?;
            ns_ip(
                &a.netns,
                &[
                    "-6",
                    "neigh",
                    "replace",
                    GATEWAY6,
                    "lladdr",
                    &host_mac,
                    "dev",
                    &a.ifname,
                    "nud",
                    "permanent",
                ],
            )?;
        } else {
            ns_ip(
                &a.netns,
                &[
                    "route", "replace", GATEWAY, "dev", &a.ifname, "scope", "link",
                ],
            )?;
            ns_ip(
                &a.netns,
                &[
                    "route", "replace", "default", "via", GATEWAY, "dev", &a.ifname,
                ],
            )?;
            ns_ip(
                &a.netns,
                &[
                    "neigh",
                    "replace",
                    GATEWAY,
                    "lladdr",
                    &host_mac,
                    "dev",
                    &a.ifname,
                    "nud",
                    "permanent",
                ],
            )?;
        }
    }
    let pod_mac = link_mac(&ns_ip(&a.netns, &["-j", "link", "show", &a.ifname])?)
        .ok_or_else(|| anyhow!("no MAC on pod {}", a.ifname))?;

    ip(&["link", "set", host, "up"])?;
    // The pod's IPv6 gateway lives on the host veth, so a host without a
    // global IPv6 address still sources pod-bound traffic from an address
    // the pod can reach (otherwise another link's link-local is picked).
    if addrs.iter().any(IpAddr::is_ipv6) {
        ip(&[
            "-6",
            "addr",
            "replace",
            &format!("{GATEWAY6}/64"),
            "dev",
            host,
            "nodad",
        ])?;
    }
    for addr in addrs {
        let cidr = host_prefix(*addr);
        let fam = if addr.is_ipv6() { "-6" } else { "-4" };
        ip(&[
            fam, "route", "replace", &cidr, "dev", host, "proto", "static",
        ])?;
    }
    let _ = std::fs::write(format!("/proc/sys/net/ipv4/conf/{host}/rp_filter"), "0");
    let _ = std::fs::write(format!("/proc/sys/net/ipv4/conf/{host}/accept_local"), "1");
    Ok((pod_mac, host_mac))
}

fn del(conf: &NetConf, a: &Args) -> Result<()> {
    let host = host_veth(&a.container_id, &a.ifname);
    for ipam in ipams(conf).unwrap_or_default() {
        let Some(addr) = ipam.release(&a.container_id, &a.ifname) else {
            continue;
        };
        let _ = ip(&["route", "del", &host_prefix(addr), "dev", &host]);
        if let Err(e) = bpfd(Request::CniDelEndpoint {
            ip: addr.to_string(),
        }) {
            eprintln!("machina-cni: machina-bpfd unregister failed: {e:#}");
        }
    }
    // Deleting the host end removes the pair; already-gone is fine (DEL is idempotent).
    let _ = ip(&["link", "del", &host]);
    Ok(())
}

fn check(a: &Args) -> Result<()> {
    let host = host_veth(&a.container_id, &a.ifname);
    ip(&["link", "show", &host])
        .map(|_| ())
        .context("host veth missing")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veth_names_fit_ifnamsiz() {
        let n = host_veth(
            "4f1c0b5e9a7d6c3b2a1908f7e6d5c4b3a291807f6e5d4c3b2a19081726354453",
            "eth0",
        );
        assert!(n.len() <= 15, "{n}");
        assert!(n.starts_with("mc"));
        assert_ne!(n, host_veth("other", "eth0"));
        assert_ne!(host_veth("c", "eth0"), host_veth("c", "net1"));
    }

    #[test]
    fn mac_from_ip_json() {
        assert_eq!(
            link_mac(r#"[{"ifindex":5,"ifname":"mc1","address":"aa:bb:cc:dd:ee:ff"}]"#).as_deref(),
            Some("aa:bb:cc:dd:ee:ff")
        );
        assert_eq!(link_mac("[]"), None);
    }
}

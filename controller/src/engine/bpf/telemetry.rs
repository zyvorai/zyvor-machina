// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Fleet views over each host's native eBPF telemetry: anomalies, flows,
//! process / DNS / file activity, hunts and health.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use machina_bpf::api::Request;
use serde_json::{json, Value};
use crate::db::DbPool;

use super::{call, fan_out_items, host, host_statuses, HostRef};

pub const SOURCE: &str = "machina-bpf";

const FLEET_LIMIT: usize = 500;

fn ts_of(v: &Value) -> Option<DateTime<Utc>> {
    v.get("ts")
        .or_else(|| v.get("last_seen"))
        .and_then(|t| t.as_str())
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc))
}

fn within_hours(items: Vec<Value>, hours: u32) -> Vec<Value> {
    if hours == 0 {
        return items;
    }
    let cutoff = Utc::now() - Duration::hours(hours as i64);
    items
        .into_iter()
        .filter(|v| ts_of(v).is_none_or(|t| t >= cutoff))
        .collect()
}

fn newest_first(items: &mut [Value]) {
    items.sort_by_key(|v| std::cmp::Reverse(ts_of(v)));
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

fn u64_of(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64()).unwrap_or(0)
}

async fn host_items(h: &HostRef, req: &Request) -> Vec<Value> {
    let Ok(Value::Array(items)) = call(h, req).await else {
        return Vec::new();
    };
    items
        .into_iter()
        .map(|mut v| {
            if let Some(o) = v.as_object_mut() {
                o.insert("host_id".into(), Value::String(h.id.clone()));
                o.insert("hostname".into(), Value::String(h.hostname.clone()));
            }
            v
        })
        .collect()
}

async fn proc_events(pool: &DbPool, kind: Option<&str>) -> Vec<Value> {
    fan_out_items(
        pool,
        &Request::ProcEvents {
            limit: Some(FLEET_LIMIT),
            kind: kind.map(String::from),
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Anomalies / threats
// ---------------------------------------------------------------------------

pub async fn anomalies(pool: &DbPool) -> Value {
    let mut items = fan_out_items(
        pool,
        &Request::Anomalies {
            limit: Some(FLEET_LIMIT),
        },
    )
    .await;
    newest_first(&mut items);
    for a in items.iter_mut() {
        if let Some(o) = a.as_object_mut() {
            let summary = o.get("summary").cloned().unwrap_or(Value::Null);
            o.entry("description").or_insert(summary);
            o.insert("source".into(), Value::String(SOURCE.into()));
        }
    }
    json!({ "anomalies": items, "total": items.len(), "source": SOURCE })
}

fn severity_weight(s: &str) -> f64 {
    match s {
        "critical" => 15.0,
        "high" => 8.0,
        "medium" => 3.0,
        "low" => 1.0,
        _ => 0.5,
    }
}

/// 100 = calm. Each recent anomaly / kill subtracts by severity.
pub async fn fleet_threat_summary(pool: &DbPool) -> Value {
    let anomalies = within_hours(
        anomalies(pool).await["anomalies"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        24,
    );
    let killed: Vec<Value> = within_hours(proc_events(pool, None).await, 24)
        .into_iter()
        .filter(|p| p["killed"] == true || p["denied"] == true)
        .collect();
    let penalty: f64 = anomalies
        .iter()
        .map(|a| severity_weight(str_of(a, "severity")))
        .sum::<f64>()
        + killed.len() as f64 * 2.0;
    let mut critical: Vec<Value> = anomalies
        .iter()
        .filter(|a| matches!(str_of(a, "severity"), "critical" | "high"))
        .map(|a| {
            json!({
                "kind": a["kind"], "title": a["kind"], "summary": a["summary"],
                "description": a["summary"], "severity": a["severity"],
                "ts": a["ts"], "host_id": a["host_id"], "vm": a["vm"],
            })
        })
        .collect();
    critical.extend(killed.iter().take(20).map(|p| {
        let what = p["path"]
            .as_str()
            .or(p["comm"].as_str())
            .unwrap_or("process");
        json!({
            "kind": format!("{}_denied", str_of(p, "kind")),
            "title": "Runtime policy block",
            "summary": format!("{} blocked on {}", what, str_of(p, "hostname")),
            "severity": "high", "ts": p["ts"], "host_id": p["host_id"], "vm": p["vm"],
        })
    }));
    newest_first(&mut critical);
    json!({
        "fleet_threat_score": (100.0 - penalty).clamp(0.0, 100.0),
        "anomalies_24h": anomalies.len(),
        "blocked_24h": killed.len(),
        "critical_events": critical,
        "source": SOURCE,
    })
}

// ---------------------------------------------------------------------------
// Flows / network pulse
// ---------------------------------------------------------------------------

pub async fn flows(pool: &DbPool, limit: usize) -> Value {
    let mut items = fan_out_items(
        pool,
        &Request::Flows {
            limit: Some(FLEET_LIMIT),
            vm: None,
        },
    )
    .await;
    items.sort_by_key(|f| std::cmp::Reverse(u64_of(f, "tx_bytes") + u64_of(f, "rx_bytes")));
    let total = items.len();
    items.truncate(limit);
    json!({ "flows": items, "total": total, "source": SOURCE })
}

fn talker_key(f: &Value) -> String {
    f["vm"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| str_of(f, "local").to_string())
}

pub async fn flow_stats(pool: &DbPool) -> Value {
    let all = fan_out_items(
        pool,
        &Request::Flows {
            limit: Some(FLEET_LIMIT),
            vm: None,
        },
    )
    .await;
    stats_of(&all)
}

fn stats_of(all: &[Value]) -> Value {
    let mut by_proto: BTreeMap<String, u64> = BTreeMap::new();
    let mut talkers: HashMap<String, (u64, u64, u64)> = HashMap::new();
    let (mut tx, mut rx, mut denied) = (0u64, 0u64, 0u64);
    for f in all {
        let (t, r) = (u64_of(f, "tx_bytes"), u64_of(f, "rx_bytes"));
        tx += t;
        rx += r;
        if str_of(f, "verdict") == "drop" {
            denied += 1;
        }
        *by_proto.entry(str_of(f, "proto").to_string()).or_default() += 1;
        let e = talkers.entry(talker_key(f)).or_default();
        e.0 += t;
        e.1 += r;
        e.2 += 1;
    }
    let mut top: Vec<(String, (u64, u64, u64))> = talkers.into_iter().collect();
    top.sort_by_key(|(_, (t, r, _))| std::cmp::Reverse(t + r));
    json!({
        "total_flows": all.len(),
        "tx_bytes": tx,
        "rx_bytes": rx,
        "denied_flows": denied,
        "by_proto": by_proto,
        "top_talkers": top.into_iter().take(10).map(|(k, (t, r, n))| json!({
            "name": k, "tx_bytes": t, "rx_bytes": r, "flows": n,
        })).collect::<Vec<_>>(),
        "source": SOURCE,
    })
}

/// TLS SNI / HTTP / SSH first-payload records across the fleet, plus the
/// most contacted hosts.
pub async fn l7(pool: &DbPool, limit: usize, protocol: Option<&str>) -> Value {
    let mut items = fan_out_items(
        pool,
        &Request::L7 {
            limit: Some(FLEET_LIMIT),
            vm: None,
            protocol: protocol.map(String::from),
        },
    )
    .await;
    newest_first(&mut items);
    let mut by_proto: BTreeMap<String, u64> = BTreeMap::new();
    let mut hosts: HashMap<String, (u64, BTreeSet<String>)> = HashMap::new();
    for r in &items {
        *by_proto
            .entry(str_of(r, "protocol").to_string())
            .or_default() += 1;
        if let Some(h) = r["host"].as_str() {
            let e = hosts.entry(h.to_string()).or_default();
            e.0 += 1;
            e.1.insert(r["vm"].as_str().unwrap_or(str_of(r, "client")).to_string());
        }
    }
    let mut top: Vec<(String, (u64, BTreeSet<String>))> = hosts.into_iter().collect();
    top.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    let total = items.len();
    items.truncate(limit);
    json!({
        "records": items,
        "total": total,
        "by_protocol": by_proto,
        "top_hosts": top.into_iter().take(20).map(|(h, (n, clients))| json!({
            "host": h, "count": n, "clients": clients,
        })).collect::<Vec<_>>(),
        "source": SOURCE,
    })
}

/// Per-VM traffic totals across the fleet (billing / chargeback).
pub async fn accounting(pool: &DbPool) -> Value {
    let items = fan_out_items(pool, &Request::Accounting { vm: None }).await;
    let (mut tx, mut rx, mut drops) = (0u64, 0u64, 0u64);
    for r in &items {
        tx += u64_of(r, "tx_bytes");
        rx += u64_of(r, "rx_bytes");
        drops += u64_of(r, "drops");
    }
    let mut items = items;
    items.sort_by_key(|r| std::cmp::Reverse(u64_of(r, "tx_bytes") + u64_of(r, "rx_bytes")));
    json!({
        "workloads": items,
        "totals": { "tx_bytes": tx, "rx_bytes": rx, "drops": drops },
        "source": SOURCE,
    })
}

/// Per-host state of the native data plane features (service LB, VM edge,
/// QEMU sandbox, shield, node isolation, TLS sampling), one entry per host.
pub async fn native_dataplane(pool: &DbPool) -> Value {
    let hosts = super::online_hosts(pool).await;
    let per_host = futures_util::future::join_all(hosts.iter().map(|h| async move {
        let reqs = [
            ("cni", Request::CniStatus),
            ("vm_edge", Request::VmEdgeStatus),
            ("vm_sandbox", Request::VmSandboxStatus),
            ("shield", Request::ShieldStatus),
            ("node_iso", Request::NodeIsoStatus),
            ("tls", Request::TlsStatus),
            ("rtnl", Request::RtnlStatus),
            ("l7_sample", Request::L7SampleStatus),
            ("vm_intel", Request::VmIntelStatus),
            ("guard", Request::GuardStatus),
            ("direct", Request::DirectStatus),
            ("quic_lb", Request::QuicLbStatus),
            ("afxdp", Request::AfxdpStatus),
            ("scx", Request::ScxStatus),
        ];
        let results = futures_util::future::join_all(reqs.iter().map(|(_, r)| call(h, r))).await;
        let mut o = serde_json::Map::new();
        o.insert("host_id".into(), json!(h.id));
        o.insert("hostname".into(), json!(h.hostname));
        for ((k, _), r) in reqs.iter().zip(results) {
            o.insert((*k).into(), r.unwrap_or(Value::Null));
        }
        Value::Object(o)
    }))
    .await;
    let isolating: Vec<&Value> = per_host
        .iter()
        .filter(|h| h["node_iso"]["isolating"] == true)
        .collect();
    json!({
        "hosts": per_host,
        "isolating_hosts": isolating.len(),
        "source": SOURCE,
    })
}

/// JA3/JA4 ClientHello fingerprints across the fleet, plus the most common
/// JA4s with the SNIs and workloads that sent them.
pub async fn tls_fingerprints(pool: &DbPool, limit: usize) -> Value {
    let mut items = fan_out_items(
        pool,
        &Request::TlsFingerprints {
            limit: Some(FLEET_LIMIT),
        },
    )
    .await;
    newest_first(&mut items);
    let mut by_ja4: HashMap<String, (u64, BTreeSet<String>, BTreeSet<String>)> = HashMap::new();
    for f in &items {
        let e = by_ja4.entry(str_of(f, "ja4").to_string()).or_default();
        e.0 += 1;
        if let Some(s) = f["sni"].as_str() {
            e.1.insert(s.to_string());
        }
        if let Some(w) = f["workload"]["name"].as_str() {
            e.2.insert(w.to_string());
        }
    }
    let mut top: Vec<_> = by_ja4.into_iter().collect();
    top.sort_by_key(|(_, (n, _, _))| std::cmp::Reverse(*n));
    let total = items.len();
    items.truncate(limit);
    json!({
        "records": items,
        "total": total,
        "top_ja4": top.into_iter().take(20).map(|(ja4, (n, snis, workloads))| json!({
            "ja4": ja4, "count": n, "snis": snis, "workloads": workloads,
        })).collect::<Vec<_>>(),
        "source": SOURCE,
    })
}

/// Newest-first records of one bpfd list request across the fleet.
pub async fn fleet_records(pool: &DbPool, req: Request, limit: usize) -> Value {
    let mut items = fan_out_items(pool, &req).await;
    newest_first(&mut items);
    let total = items.len();
    items.truncate(limit);
    json!({ "records": items, "total": total, "source": SOURCE })
}

/// Who changed links, addresses and routes on each host.
pub async fn rtnl_events(pool: &DbPool, limit: usize) -> Value {
    fleet_records(
        pool,
        Request::RtnlEvents {
            limit: Some(FLEET_LIMIT),
            iface: None,
        },
        limit,
    )
    .await
}

/// VMM guard violations (audited or denied) per host.
pub async fn guard_events(pool: &DbPool, limit: usize) -> Value {
    fleet_records(
        pool,
        Request::GuardEvents {
            limit: Some(FLEET_LIMIT),
        },
        limit,
    )
    .await
}

/// VM runtime reports for every tracked VM on hosts with it enabled.
pub async fn vm_intel(pool: &DbPool) -> Value {
    let hosts = super::online_hosts(pool).await;
    let per_host = futures_util::future::join_all(hosts.iter().map(|h| async move {
        let Ok(st) = call(h, &Request::VmIntelStatus).await else {
            return Vec::new();
        };
        let names: Vec<String> = st["vms"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let reqs: Vec<Request> = names
            .into_iter()
            .map(|name| Request::VmIntelVm { name })
            .collect();
        let reports = futures_util::future::join_all(reqs.iter().map(|r| call(h, r))).await;
        reports
            .into_iter()
            .flatten()
            .map(|mut r| {
                r["host_id"] = json!(h.id);
                r["hostname"] = json!(h.hostname);
                r
            })
            .collect()
    }))
    .await;
    json!({ "vms": per_host.into_iter().flatten().collect::<Vec<Value>>(), "source": SOURCE })
}

/// ICMP error histogram (unreachable / time exceeded / too big …) per host.
pub async fn icmp_errors(pool: &DbPool) -> Value {
    let mut items = fan_out_items(pool, &Request::IcmpErrors).await;
    items.sort_by_key(|r| std::cmp::Reverse(u64_of(r, "count")));
    json!({ "errors": items, "source": SOURCE })
}

/// Workload → peer service map plus threats / timeline, in the shape the
/// Network Canvas renders.
pub async fn network_pulse(pool: &DbPool) -> Value {
    let all = fan_out_items(
        pool,
        &Request::Flows {
            limit: Some(FLEET_LIMIT),
            vm: None,
        },
    )
    .await;
    // node key → (node json, flows out, flows in, denied)
    let mut nodes: BTreeMap<String, (Value, u64, u64, u64)> = BTreeMap::new();
    // (src key, dst key) → (bytes, flows, denied, ports)
    let mut edges: BTreeMap<(String, String), (u64, u64, u64, BTreeSet<String>)> = BTreeMap::new();
    for f in &all {
        let src = talker_key(f);
        let ns = if f["vm"].is_string() { "vm" } else { "host" };
        let skey = format!("{ns}/{src}");
        let dst = str_of(f, "remote").to_string();
        let dkey = format!("external/{dst}");
        let denied = u64::from(str_of(f, "verdict") == "deny");
        let s = nodes.entry(skey.clone()).or_insert_with(|| {
            (
                json!({ "name": src, "namespace": ns, "host_id": f["host_id"] }),
                0,
                0,
                0,
            )
        });
        s.1 += 1;
        s.3 += denied;
        let d = nodes
            .entry(dkey.clone())
            .or_insert_with(|| (json!({ "name": dst, "namespace": "external" }), 0, 0, 0));
        d.2 += 1;
        d.3 += denied;
        let e = edges.entry((skey, dkey)).or_default();
        e.0 += u64_of(f, "tx_bytes") + u64_of(f, "rx_bytes");
        e.1 += 1;
        e.2 += denied;
        e.3.insert(format!("{}/{}", f["remote_port"], str_of(f, "proto")));
    }
    let blocked: u64 = edges.values().map(|e| e.2).sum();
    let edge_list: Vec<Value> = edges
        .iter()
        .map(|((skey, dkey), (bytes, n, denied, ports))| {
            let (sns, sname) = skey.split_once('/').unwrap_or(("host", skey.as_str()));
            let dname = dkey.trim_start_matches("external/");
            json!({
                "id": format!("{skey}->{dkey}"),
                "source": sname, "source_namespace": sns,
                "target": dname, "target_namespace": "external",
                "source_key": skey, "target_key": dkey,
                "ports": ports, "bytes": bytes, "flow_count": n, "dropped_count": denied,
                "health": if *denied > 0 { "degraded" } else { "healthy" },
                "is_external": true,
            })
        })
        .collect();
    let node_list: Vec<Value> = nodes
        .into_values()
        .map(|(mut v, out, inn, denied)| {
            v["connections_out"] = json!(out);
            v["connections_in"] = json!(inn);
            v["blocked_flows"] = json!(denied);
            v["status"] = json!(if denied > 0 { "warning" } else { "healthy" });
            v
        })
        .collect();
    let stats = stats_of(&all);
    let talkers: Vec<Value> = stats["top_talkers"].as_array().cloned().unwrap_or_default();
    let threats = fleet_threat_summary(pool).await;
    json!({
        "enabled": true,
        "overview": stats,
        "service_map": {
            "meta": { "stats": {
                "services": node_list.len(), "connections": edge_list.len(), "blocked": blocked,
            } },
            "nodes": node_list,
            "edges": edge_list,
        },
        "top_talkers": { "talkers": talkers },
        "threats": { "threats": threats["critical_events"], "score": threats["fleet_threat_score"] },
        "timeline": fleet_timeline(pool, 1).await,
        "anomalies": anomalies(pool).await,
        "source": SOURCE,
    })
}

// ---------------------------------------------------------------------------
// Timeline / per-host resources
// ---------------------------------------------------------------------------

fn proc_event_summary(p: &Value) -> String {
    let who = p["path"].as_str().or(p["comm"].as_str()).unwrap_or("?");
    match str_of(p, "kind") {
        "exec" => format!("exec {}", p["cmdline"].as_str().unwrap_or(who)),
        "file_open" => format!("{who} opened {}", p["path"].as_str().unwrap_or("?")),
        "connect" => format!(
            "{} connect {}:{}",
            str_of(p, "comm"),
            p["daddr"].as_str().unwrap_or("?"),
            p["dport"]
        ),
        "cap_denied" => format!(
            "{} denied capability {}",
            str_of(p, "comm"),
            str_of(p, "capability")
        ),
        k => format!("{k} {who}"),
    }
}

fn timeline_of(procs: Vec<Value>, net: Vec<Value>, anomalies: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for p in procs {
        if str_of(&p, "kind") == "exit" || str_of(&p, "kind") == "fork" {
            continue;
        }
        let blocked = p["denied"] == true || p["killed"] == true;
        out.push(json!({
            "ts": p["ts"], "kind": format!("process.{}", str_of(&p, "kind")),
            "type": str_of(&p, "kind"), "summary": proc_event_summary(&p),
            "message": proc_event_summary(&p),
            "severity": if blocked { "high" } else { "info" },
            "host_id": p["host_id"], "vm": p["vm"], "container": p["container"],
        }));
    }
    for n in net.into_iter().filter(|n| str_of(n, "verdict") == "deny") {
        let summary = format!(
            "denied {} {}:{} → {}:{}",
            str_of(&n, "proto"),
            str_of(&n, "local"),
            n["local_port"],
            str_of(&n, "remote"),
            n["remote_port"]
        );
        out.push(json!({
            "ts": n["ts"], "kind": "network.deny", "type": "deny", "summary": summary,
            "message": summary, "severity": "medium", "host_id": n["host_id"], "vm": n["vm"],
        }));
    }
    for a in anomalies {
        out.push(json!({
            "ts": a["ts"], "kind": format!("anomaly.{}", str_of(&a, "kind")), "type": "anomaly",
            "summary": a["summary"], "message": a["summary"], "severity": a["severity"],
            "host_id": a["host_id"], "vm": a["vm"],
        }));
    }
    newest_first(&mut out);
    out
}

pub async fn fleet_timeline(pool: &DbPool, hours: u32) -> Value {
    let (procs, net, anoms) = tokio::join!(
        proc_events(pool, None),
        fan_out_items(
            pool,
            &Request::Events {
                limit: Some(FLEET_LIMIT),
                kind: None
            }
        ),
        fan_out_items(
            pool,
            &Request::Anomalies {
                limit: Some(FLEET_LIMIT)
            }
        ),
    );
    let mut events = within_hours(timeline_of(procs, net, anoms), hours);
    events.truncate(FLEET_LIMIT);
    json!({ "events": events, "hours": hours, "source": SOURCE })
}

fn process_graph(execs: &[Value]) -> Value {
    let mut nodes: BTreeMap<u64, Value> = BTreeMap::new();
    let mut edges = Vec::new();
    for p in execs {
        let pid = u64_of(p, "tgid").max(u64_of(p, "pid"));
        nodes.insert(
            pid,
            json!({
                "id": pid.to_string(), "pid": pid, "label": p["path"].as_str().unwrap_or(str_of(p, "comm")),
                "cmdline": p["cmdline"], "uid": p["uid"], "unit": p["unit"],
                "container": p["container"], "vm": p["vm"], "ts": p["ts"],
            }),
        );
        if let Some(ppid) = p["ppid"].as_u64().filter(|pp| *pp > 0) {
            edges.push(json!({ "source": ppid.to_string(), "target": pid.to_string() }));
        }
    }
    let known: HashSet<String> = nodes.keys().map(|k| k.to_string()).collect();
    for e in &edges {
        let src = str_of(e, "source").to_string();
        if !known.contains(&src) {
            let pid: u64 = src.parse().unwrap_or(0);
            nodes
                .entry(pid)
                .or_insert_with(|| json!({ "id": src, "pid": pid, "label": format!("pid {pid}") }));
        }
    }
    json!({ "nodes": nodes.into_values().collect::<Vec<_>>(), "edges": edges })
}

/// `resource` ∈ summary | processes | connections | dns | files | ports |
/// containers | timeline | process-graph.
pub async fn host_resource(pool: &DbPool, host_id: &str, resource: &str, hours: u32) -> Value {
    let Some(h) = host(pool, host_id).await else {
        return json!({ "error": "host not found", "host_id": host_id });
    };
    let lim = Some(FLEET_LIMIT);
    let procs = |kind: &str| Request::ProcEvents {
        limit: lim,
        kind: Some(kind.into()),
    };
    let body = match resource {
        "summary" => {
            let status = call(&h, &Request::Status).await;
            let anomalies = host_items(&h, &Request::Anomalies { limit: Some(50) }).await;
            json!({
                "reachable": status.is_ok(),
                "status": status.as_ref().ok(),
                "error": status.as_ref().err().map(|e| format!("{e:#}")),
                "recent_anomalies": anomalies,
            })
        }
        "processes" => {
            json!({ "processes": within_hours(host_items(&h, &procs("exec")).await, hours) })
        }
        "connections" => {
            let flows = host_items(
                &h,
                &Request::Flows {
                    limit: lim,
                    vm: None,
                },
            )
            .await;
            let connects = within_hours(host_items(&h, &procs("connect")).await, hours);
            json!({ "connections": flows, "connects": connects })
        }
        "dns" => {
            json!({ "queries": within_hours(host_items(&h, &Request::Dns { limit: lim }).await, hours) })
        }
        "files" => {
            json!({ "files": within_hours(host_items(&h, &procs("file_open")).await, hours) })
        }
        "ports" => {
            let flows = host_items(
                &h,
                &Request::Flows {
                    limit: lim,
                    vm: None,
                },
            )
            .await;
            let mut ports: BTreeMap<(String, u64), (HashSet<String>, u64)> = BTreeMap::new();
            for f in flows.iter().filter(|f| str_of(f, "origin") == "remote") {
                let e = ports
                    .entry((str_of(f, "proto").to_string(), u64_of(f, "local_port")))
                    .or_default();
                e.0.insert(str_of(f, "remote").to_string());
                e.1 += 1;
            }
            json!({ "ports": ports.into_iter().map(|((proto, port), (peers, n))| json!({
                "proto": proto, "port": port, "peers": peers.len(), "flows": n,
            })).collect::<Vec<_>>() })
        }
        "containers" => {
            let mut by: BTreeMap<String, (u64, String)> = BTreeMap::new();
            for p in host_items(
                &h,
                &Request::ProcEvents {
                    limit: lim,
                    kind: None,
                },
            )
            .await
            {
                if let Some(c) = p["container"].as_str() {
                    let e = by.entry(c.to_string()).or_default();
                    e.0 += 1;
                    if let Some(ts) = p["ts"].as_str() {
                        e.1 = e.1.clone().max(ts.to_string());
                    }
                }
            }
            json!({ "containers": by.into_iter().map(|(c, (n, last))| json!({
                "id": c, "events": n, "last_seen": last,
            })).collect::<Vec<_>>() })
        }
        "timeline" => {
            let (rp, rn, ra) = (
                Request::ProcEvents {
                    limit: lim,
                    kind: None,
                },
                Request::Events {
                    limit: lim,
                    kind: None,
                },
                Request::Anomalies { limit: lim },
            );
            let (p, n, a) = tokio::join!(
                host_items(&h, &rp),
                host_items(&h, &rn),
                host_items(&h, &ra)
            );
            json!({ "events": within_hours(timeline_of(p, n, a), hours) })
        }
        "process-graph" | "process_graph" => process_graph(&host_items(&h, &procs("exec")).await),
        other => json!({ "error": format!("unknown resource {other:?}") }),
    };
    let mut out = body;
    if let Some(o) = out.as_object_mut() {
        o.insert("host_id".into(), json!(h.id));
        o.insert("hostname".into(), json!(h.hostname));
        o.insert("source".into(), json!(SOURCE));
    }
    out
}

// ---------------------------------------------------------------------------
// Sensors / health / inventory
// ---------------------------------------------------------------------------

pub async fn sensors(pool: &DbPool) -> Value {
    let statuses = host_statuses(pool).await;
    let sensors: Vec<Value> = statuses
        .iter()
        .map(|s| {
            let st = s.status.as_ref();
            json!({
                "id": s.host_id, "host_id": s.host_id, "hostname": s.hostname,
                "status": match st { Some(x) if x.available && x.programs_compiled => "healthy", Some(_) => "degraded", None => "unreachable" },
                "version": st.map(|x| x.version.clone()),
                "kernel": st.map(|x| x.features.kernel.clone()),
                "mode": st.map(|x| &x.mode),
                "tracepoints": st.map(|x| x.tracepoints.len()),
                "interfaces": st.map(|x| x.interfaces.len()),
                "counters": st.map(|x| &x.counters),
                "notes": st.map(|x| x.notes.clone()),
                "error": s.error,
                "kind": "machina-bpf",
            })
        })
        .collect();
    let healthy = sensors.iter().filter(|s| s["status"] == "healthy").count();
    json!({
        "sensors": sensors,
        "summary": format!("{healthy}/{} native sensor(s) healthy", statuses.len()),
        "source": SOURCE,
    })
}

pub async fn fabric_health(pool: &DbPool) -> Value {
    let statuses = host_statuses(pool).await;
    let mut issues = Vec::new();
    for s in &statuses {
        match &s.status {
            None => issues.push(json!({
                "severity": "high", "kind": "sensor_unreachable", "host_id": s.host_id,
                "summary": format!("machina-bpfd unreachable on {}: {}", s.hostname, s.error.clone().unwrap_or_default()),
            })),
            Some(st) => {
                if !st.programs_compiled {
                    issues.push(json!({
                        "severity": "warning", "kind": "datapath_missing", "host_id": s.host_id,
                        "summary": format!("eBPF datapath not loaded on {}", s.hostname),
                    }));
                }
                if st.mode.lease_expired {
                    issues.push(json!({
                        "severity": "warning", "kind": "enforce_lease_lapsed", "host_id": s.host_id,
                        "summary": format!("Enforce lease lapsed on {} — host reverted to observe", s.hostname),
                    }));
                }
                for n in &st.notes {
                    issues.push(json!({
                        "severity": "info", "kind": "note", "host_id": s.host_id,
                        "summary": format!("{}: {n}", s.hostname),
                    }));
                }
            }
        }
    }
    let bad = issues
        .iter()
        .any(|i| matches!(str_of(i, "severity"), "high" | "critical" | "warning"));
    json!({
        "status": if statuses.is_empty() { "unknown" } else if bad { "degraded" } else { "healthy" },
        "hosts": statuses.len(),
        "issues": issues,
        "source": SOURCE,
    })
}

pub async fn asset_inventory(pool: &DbPool) -> Value {
    let statuses = host_statuses(pool).await;
    let procs = proc_events(pool, None).await;
    let mut vms: BTreeMap<String, Value> = BTreeMap::new();
    let mut hosts = Vec::new();
    for s in &statuses {
        let ifaces: Vec<Value> = s
            .status
            .as_ref()
            .map(|st| {
                st.interfaces
                    .iter()
                    .map(|i| json!({ "name": i.name, "vm": i.vm, "mac": i.mac, "flags": i.flags }))
                    .collect()
            })
            .unwrap_or_default();
        for i in s.status.iter().flat_map(|st| st.interfaces.iter()) {
            if let Some(vm) = &i.vm {
                if let Some(a) = vms
                    .entry(vm.clone())
                    .or_insert_with(|| json!({ "name": vm, "host_id": s.host_id, "taps": [] }))
                    ["taps"]
                    .as_array_mut()
                {
                    a.push(json!(i.name))
                }
            }
        }
        hosts.push(json!({
            "host_id": s.host_id, "hostname": s.hostname, "reachable": s.reachable,
            "kernel": s.status.as_ref().map(|x| x.features.kernel.clone()), "interfaces": ifaces,
        }));
    }
    let containers: BTreeMap<String, Value> = procs
        .iter()
        .filter_map(|p| {
            p["container"]
                .as_str()
                .map(|c| (c.to_string(), json!({ "id": c, "host_id": p["host_id"] })))
        })
        .collect();
    json!({
        "hosts": hosts,
        "vms": vms.into_values().collect::<Vec<_>>(),
        "containers": containers.into_values().collect::<Vec<_>>(),
        "source": SOURCE,
    })
}

/// Anomalies sharing a workload or remote peer, grouped.
pub async fn correlations(pool: &DbPool) -> Value {
    let items = anomalies(pool).await["anomalies"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for a in items {
        let key = a["vm"]
            .as_str()
            .map(|v| format!("vm:{v}"))
            .or_else(|| a["remote"].as_str().map(|r| format!("peer:{r}")))
            .unwrap_or_else(|| format!("host:{}", str_of(&a, "host_id")));
        groups.entry(key).or_default().push(a);
    }
    let correlations: Vec<Value> = groups
        .into_iter()
        .filter(|(_, v)| {
            v.iter()
                .map(|a| str_of(a, "kind"))
                .collect::<HashSet<_>>()
                .len()
                > 1
                || v.len() >= 3
        })
        .map(|(key, v)| {
            let kinds: Vec<String> = v
                .iter()
                .map(|a| str_of(a, "kind").to_string())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let worst = v
                .iter()
                .map(|a| str_of(a, "severity"))
                .max_by(|a, b| severity_weight(a).total_cmp(&severity_weight(b)))
                .unwrap_or("low")
                .to_string();
            json!({
                "id": key, "entity": key, "severity": worst, "kinds": kinds,
                "summary": format!("{} related anomalies on {key}: {}", v.len(), kinds.join(", ")),
                "anomalies": v,
            })
        })
        .collect();
    json!({ "correlations": correlations, "source": SOURCE })
}

// ---------------------------------------------------------------------------
// Search / hunts
// ---------------------------------------------------------------------------

async fn all_events(pool: &DbPool) -> Vec<Value> {
    let (p, n, d, a) = tokio::join!(
        proc_events(pool, None),
        fan_out_items(
            pool,
            &Request::Events {
                limit: Some(FLEET_LIMIT),
                kind: None
            }
        ),
        fan_out_items(
            pool,
            &Request::Dns {
                limit: Some(FLEET_LIMIT)
            }
        ),
        fan_out_items(
            pool,
            &Request::Anomalies {
                limit: Some(FLEET_LIMIT)
            }
        ),
    );
    let tag = |mut v: Value, t: &str| {
        if let Some(o) = v.as_object_mut() {
            o.insert("event_type".into(), json!(t));
        }
        v
    };
    let mut out: Vec<Value> = p.into_iter().map(|v| tag(v, "process")).collect();
    out.extend(n.into_iter().map(|v| tag(v, "network")));
    out.extend(d.into_iter().map(|v| tag(v, "dns")));
    out.extend(a.into_iter().map(|v| tag(v, "anomaly")));
    newest_first(&mut out);
    out
}

/// Every whitespace token must appear (case-insensitive); `key:value` tokens
/// match a specific field.
fn matches_query(v: &Value, tokens: &[String]) -> bool {
    let blob = v.to_string().to_lowercase();
    tokens.iter().all(|t| match t.split_once(':') {
        Some((k, want)) if v.get(k).is_some() => v[k]
            .as_str()
            .map(|s| s.to_lowercase().contains(want))
            .unwrap_or_else(|| v[k].to_string().to_lowercase().contains(want)),
        _ => blob.contains(t.as_str()),
    })
}

pub async fn search(pool: &DbPool, query: &str, host_id: Option<&str>, limit: usize) -> Value {
    let tokens: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();
    let results: Vec<Value> = all_events(pool)
        .await
        .into_iter()
        .filter(|v| host_id.is_none_or(|h| str_of(v, "host_id") == h))
        .filter(|v| matches_query(v, &tokens))
        .take(limit.clamp(1, 1000))
        .collect();
    json!({ "query": query, "results": results, "hit_count": results.len(), "backend": SOURCE })
}

const HUNTS: &[(&str, &str, &str)] = &[
    (
        "blocked-activity",
        "Blocked by runtime policy",
        "Processes killed / denied and connections dropped by native enforcement",
    ),
    (
        "suspicious-exec",
        "Suspicious exec",
        "Executions out of /tmp, /dev/shm or hidden paths, and anomaly-flagged execs",
    ),
    (
        "shell-spawn",
        "Interactive shells",
        "sh / bash / zsh / dash executions",
    ),
    (
        "sensitive-files",
        "Sensitive file access",
        "Opens of watched credential and config files",
    ),
    (
        "capability-denials",
        "Capability denials",
        "Capability use denied by deny_cap policies",
    ),
    (
        "port-scans",
        "Port and host scans",
        "Scan and sweep anomalies from the flow detector",
    ),
    (
        "dns-anomalies",
        "DNS anomalies",
        "DGA-like, tunnelling and NXDOMAIN-burst DNS anomalies",
    ),
    (
        "beaconing",
        "Beaconing / exfil",
        "Periodic outbound and large transfer anomalies",
    ),
];

pub fn hunt_queries() -> Value {
    json!({
        "queries": HUNTS.iter().map(|(id, name, desc)| json!({ "id": id, "name": name, "description": desc })).collect::<Vec<_>>(),
        "source": SOURCE,
    })
}

fn hunt_hit(id: &str, v: &Value) -> bool {
    let kind = str_of(v, "kind");
    let ty = str_of(v, "event_type");
    let path = v["path"].as_str().unwrap_or("");
    match id {
        "blocked-activity" => {
            v["killed"] == true || v["denied"] == true || str_of(v, "verdict") == "deny"
        }
        "suspicious-exec" => {
            (ty == "process"
                && kind == "exec"
                && (path.starts_with("/tmp/")
                    || path.starts_with("/dev/shm/")
                    || path.contains("/.")))
                || (ty == "anomaly" && kind.contains("exec"))
        }
        "shell-spawn" => {
            ty == "process"
                && kind == "exec"
                && matches!(
                    path.rsplit('/').next().unwrap_or(""),
                    "sh" | "bash" | "zsh" | "dash" | "ash"
                )
        }
        "sensitive-files" => ty == "process" && kind == "file_open",
        "capability-denials" => kind == "cap_denied",
        "port-scans" => ty == "anomaly" && (kind.contains("scan") || kind.contains("sweep")),
        "dns-anomalies" => ty == "anomaly" && kind.starts_with("dns"),
        "beaconing" => {
            ty == "anomaly"
                && (kind.contains("beacon") || kind.contains("exfil") || kind.contains("transfer"))
        }
        _ => false,
    }
}

pub async fn run_hunt(pool: &DbPool, query_id: &str, host_id: Option<&str>) -> Value {
    let Some((_, name, _)) = HUNTS.iter().find(|(id, _, _)| *id == query_id) else {
        return json!({ "ok": false, "error": format!("unknown hunt query {query_id:?}") });
    };
    let hits: Vec<Value> = all_events(pool)
        .await
        .into_iter()
        .filter(|v| host_id.is_none_or(|h| str_of(v, "host_id") == h))
        .filter(|v| hunt_hit(query_id, v))
        .take(FLEET_LIMIT)
        .collect();
    json!({ "ok": true, "query_id": query_id, "name": name, "results": hits, "hit_count": hits.len(), "source": SOURCE })
}

// ---------------------------------------------------------------------------
// Firewall-facing helpers
// ---------------------------------------------------------------------------

/// Denied / allowed connection activity for a firewall target (VM name or
/// host id) over the last `hours`.
pub async fn target_activity(pool: &DbPool, target: &str, hours: u32) -> Value {
    let events = within_hours(
        fan_out_items(
            pool,
            &Request::Events {
                limit: Some(FLEET_LIMIT),
                kind: None,
            },
        )
        .await,
        hours,
    );
    let hits: Vec<Value> = events
        .into_iter()
        .filter(|e| str_of(e, "vm") == target || str_of(e, "host_id") == target)
        .collect();
    let denied = hits
        .iter()
        .filter(|e| str_of(e, "verdict") == "deny")
        .count();
    json!({
        "target": target, "hours": hours, "events": hits, "denied": denied,
        "allowed": hits.len() - denied, "source": SOURCE,
    })
}

/// Start a 60s pcapng capture (lockdown evidence) on every tap of `target`:
/// a VM name, or a host id for all of that host's attached interfaces.
pub async fn capture_target(pool: &DbPool, target: &str) -> Value {
    let mut started = Vec::new();
    for (h, res) in super::fan_out(pool, &Request::ListInterfaces).await {
        let Ok(Value::Array(ifaces)) = res else {
            continue;
        };
        let whole_host = h.id == target;
        for i in ifaces
            .iter()
            .filter(|i| whole_host || str_of(i, "vm") == target)
        {
            let req = Request::CaptureStart {
                iface: str_of(i, "name").to_string(),
                duration_secs: Some(60),
                sample: None,
                snaplen: None,
                max_packets: None,
            };
            if let Ok(info) = call(&h, &req).await {
                started.push(json!({ "host_id": h.id, "capture": info }));
            }
        }
    }
    json!({ "target": target, "captures": started, "source": SOURCE })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_tokens() {
        let v = json!({"comm": "curl", "daddr": "203.0.113.5", "kind": "connect"});
        assert!(matches_query(&v, &["curl".into()]));
        assert!(matches_query(&v, &["kind:conn".into(), "203.0".into()]));
        assert!(!matches_query(&v, &["kind:exec".into()]));
    }

    #[test]
    fn graph_links_parents() {
        let g = process_graph(&[json!({"pid": 10, "tgid": 10, "ppid": 1, "comm": "sh"})]);
        assert_eq!(g["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(g["edges"][0]["source"], "1");
    }

    #[test]
    fn hunts_classify() {
        let shell = json!({"event_type": "process", "kind": "exec", "path": "/bin/bash"});
        assert!(hunt_hit("shell-spawn", &shell));
        let tmp = json!({"event_type": "process", "kind": "exec", "path": "/tmp/x"});
        assert!(hunt_hit("suspicious-exec", &tmp));
    }
}

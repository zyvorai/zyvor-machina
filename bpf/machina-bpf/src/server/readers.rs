// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Ring-buffer readers: decode kernel events into records, run detectors,
//! feed the event store and live subscribers.

use std::net::IpAddr;

use aya::maps::{MapData, RingBuf};
use tokio::sync::Notify;

use super::*;

fn decode<T: Copy>(b: &[u8]) -> Option<T> {
    (b.len() >= std::mem::size_of::<T>())
        .then(|| unsafe { std::ptr::read_unaligned(b.as_ptr() as *const T) })
}

fn cstr(b: &[u8]) -> String {
    let n = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..n]).into_owned()
}

fn wall_now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

/// Drive one ring buffer until the process exits.
pub(super) fn spawn_reader<F>(rb: RingBuf<MapData>, name: &'static str, mut on_item: F)
where
    F: FnMut(&[u8]) + Send + 'static,
{
    tokio::spawn(async move {
        let mut fd = match AsyncFd::new(rb) {
            Ok(fd) => fd,
            Err(e) => {
                tracing::error!("ringbuf {name}: {e}");
                return;
            }
        };
        loop {
            let mut guard = match fd.readable_mut().await {
                Ok(g) => g,
                Err(e) => {
                    tracing::error!("ringbuf {name}: {e}");
                    return;
                }
            };
            let rb = guard.get_inner_mut();
            while let Some(item) = rb.next() {
                on_item(&item);
            }
            guard.clear_ready();
        }
    });
}

fn net_kind(k: u32) -> &'static str {
    match k {
        NET_EV_FLOW_OPEN => "flow_open",
        NET_EV_FLOW_CLOSE => "flow_close",
        NET_EV_DENY => "deny",
        NET_EV_ALLOW_MISS => "allow_miss",
        NET_EV_QOS_DROP => "qos_drop",
        NET_EV_RATE_LIMIT => "rate_limit",
        _ => "unknown",
    }
}

fn verdict_name(v: u32) -> &'static str {
    match v {
        VERDICT_DROP => "drop",
        VERDICT_OBSERVED => "observed",
        _ => "pass",
    }
}

pub(super) fn on_net(sh: &SharedState, bus: &broadcast::Sender<StreamEvent>, b: &[u8]) {
    let Some(ev) = decode::<NetEvent>(b) else {
        return;
    };
    let k = ev.key;
    let local = fmt_addr(&k.local);
    let remote = fmt_addr(&k.remote);
    let mut anomalies = Vec::new();
    let rec = {
        let mut s = lock(sh);
        let (iface, vm) = s.iface(k.ifindex);
        let is_open = ev.kind == NET_EV_FLOW_OPEN;
        let rec = NetEventRecord {
            workload: s.iface_workload(k.ifindex),
            ts: mono_to_rfc3339(ev.ts_ns),
            kind: net_kind(ev.kind).into(),
            verdict: verdict_name(ev.verdict).into(),
            policy_id: if is_open {
                None
            } else {
                s.policy_labels.get(&ev.policy_id).cloned()
            },
            iface: iface.clone(),
            vm: vm.clone(),
            proto: proto_name(k.proto).into(),
            local: local.clone(),
            local_port: k.local_port,
            remote: remote.clone(),
            remote_port: k.remote_port,
            pkt_len: ev.pkt_len,
            tx_bytes: ev.tx_bytes,
            rx_bytes: ev.rx_bytes,
        };
        s.counters.net_events += 1;
        if ev.kind == NET_EV_RATE_LIMIT {
            s.counters.rate_limited += 1;
        }
        match ev.verdict {
            VERDICT_DROP => s.counters.drops += 1,
            VERDICT_OBSERVED => s.counters.observed += 1,
            _ => {}
        }
        let ctx = Ctx { vm, iface };
        let now = wall_now();
        if is_open {
            s.counters.flows_opened += 1;
            let origin_local = ev.policy_id == ORIGIN_LOCAL as u32;
            anomalies.extend(s.detector.on_flow_open(
                now,
                &ctx,
                &local,
                &remote,
                k.remote_port,
                k.local_port,
                origin_local,
            ));
        } else if matches!(ev.kind, NET_EV_DENY | NET_EV_ALLOW_MISS) {
            anomalies.extend(s.detector.on_deny(now, &ctx, &local, &remote));
        }
        Shared::push_capped(&mut s.net, rec.clone(), NET_STORE_CAP);
        for a in &anomalies {
            s.counters.anomalies += 1;
            Shared::push_capped(&mut s.anomalies, a.clone(), ANOMALY_STORE_CAP);
        }
        rec
    };
    publish(bus, "net", &rec);
    for a in anomalies {
        publish(bus, "anomaly", &a);
    }
}

pub(super) fn on_dns(
    sh: &SharedState,
    bus: &broadcast::Sender<StreamEvent>,
    wake: &Notify,
    b: &[u8],
) {
    let Some(ev) = decode::<DnsEvent>(b) else {
        return;
    };
    let n = (ev.payload_len as usize).min(DNS_PAYLOAD_LEN);
    let start = if ev.flags & DNS_EV_TCP != 0 {
        2.min(n)
    } else {
        0
    };
    let Some(msg) = dns::parse(&ev.payload[start..n]) else {
        return;
    };
    let local = fmt_addr(&ev.local);
    let remote = fmt_addr(&ev.remote);
    let mut anomaly = None;
    let mut queued = false;
    let mut alerts = Vec::new();
    let rec = {
        let mut s = lock(sh);
        let (iface, vm) = s.iface(ev.ifindex);
        let rec = DnsRecord {
            workload: s.iface_workload(ev.ifindex),
            ts: mono_to_rfc3339(ev.ts_ns),
            iface: iface.clone(),
            vm: vm.clone(),
            client: local.clone(),
            server: remote,
            id: msg.id,
            is_response: msg.is_response,
            qname: msg.qname.clone(),
            qtype: dns::rtype_name(msg.qtype),
            rcode: dns::rcode_name(msg.rcode).into(),
            answers: msg.answers.clone(),
        };
        s.counters.dns_events += 1;
        if !msg.is_response {
            anomaly = s
                .detector
                .on_dns_query(wall_now(), &Ctx { vm, iface }, &local, &msg.qname);
        } else {
            let hits: Vec<(u32, u32)> = s
                .dns_denies
                .iter()
                .filter(|(_, (suffix, _))| {
                    policy::dns_suffix_match(&msg.qname, suffix)
                        || msg
                            .answers
                            .iter()
                            .any(|a| policy::dns_suffix_match(&a.name, suffix))
                })
                .map(|(num, (_, scope))| (*num, *scope))
                .collect();
            for (num, scope) in hits {
                for a in &msg.answers {
                    if a.rtype != "A" && a.rtype != "AAAA" {
                        continue;
                    }
                    if let Ok(ip) = a.data.parse::<IpAddr>() {
                        let prefix = Prefix {
                            addr: policy::ip_to_addr(ip),
                            bits: 128,
                        };
                        s.dns_block_queue.push((scope, prefix, num));
                        queued = true;
                    }
                }
            }
        }
        if msg.is_response
            && msg.rcode == 0
            && vm::fqdn_learn(&mut s, &msg, rec.vm.as_deref(), ev.ifindex)
        {
            queued = true;
        }
        if let Some(name) = rec.vm.as_deref() {
            let (q, a) = vm::threat_observe(&mut s, &msg, name, &local);
            queued |= q;
            alerts = a;
        }
        Shared::push_capped(&mut s.dns, rec.clone(), DNS_STORE_CAP);
        if let Some(a) = &anomaly {
            s.counters.anomalies += 1;
            Shared::push_capped(&mut s.anomalies, a.clone(), ANOMALY_STORE_CAP);
        }
        rec
    };
    if queued {
        wake.notify_one();
    }
    publish(bus, "dns", &rec);
    if let Some(a) = anomaly {
        publish(bus, "anomaly", &a);
    }
    for a in alerts {
        tracing::warn!(kind = %a.kind, src = %a.src, "{}", a.detail);
        publish(bus, "alert", &a);
    }
}

/// Build an [`L7Record`] from a kernel event. `None` for unrecognised payloads.
pub(super) fn l7_record(
    ev: &L7Event,
    iface: Option<String>,
    vm: Option<String>,
) -> Option<L7Record> {
    let n = (ev.payload_len as usize).min(L7_PAYLOAD_LEN);
    let parsed = crate::l7::classify(&ev.payload[..n])?;
    let k = ev.key;
    // The client sent this payload: workload-side for outbound, remote for inbound.
    let outbound = ev.dir == DIR_FROM_WORKLOAD;
    let (client, client_port, server, server_port) = if outbound {
        (
            fmt_addr(&k.local),
            k.local_port,
            fmt_addr(&k.remote),
            k.remote_port,
        )
    } else {
        (
            fmt_addr(&k.remote),
            k.remote_port,
            fmt_addr(&k.local),
            k.local_port,
        )
    };
    let mut rec = L7Record {
        ts: mono_to_rfc3339(ev.ts_ns),
        iface,
        vm,
        direction: if outbound { "outbound" } else { "inbound" }.into(),
        client,
        client_port,
        server,
        server_port,
        ..L7Record::default()
    };
    match parsed {
        crate::l7::L7::Tls(t) => {
            rec.protocol = "tls".into();
            rec.host = t.sni;
            rec.alpn = t.alpn;
            rec.tls_version = Some(t.version);
        }
        crate::l7::L7::Http(h) => {
            rec.protocol = "http".into();
            rec.host = h.host;
            rec.method = Some(h.method);
            rec.path = Some(h.path);
            rec.user_agent = h.user_agent;
        }
        crate::l7::L7::Ssh { banner } => {
            rec.protocol = "ssh".into();
            rec.banner = Some(banner);
        }
    }
    Some(rec)
}

pub(super) fn on_l7(sh: &SharedState, bus: &broadcast::Sender<StreamEvent>, b: &[u8]) {
    let Some(ev) = decode::<L7Event>(b) else {
        return;
    };
    let rec = {
        let mut s = lock(sh);
        let (iface, vm) = s.iface(ev.key.ifindex);
        let Some(mut rec) = l7_record(&ev, iface, vm) else {
            return;
        };
        rec.workload = s.iface_workload(ev.key.ifindex);
        if s.fp_from_l7 && rec.protocol == "tls" {
            let n = (ev.payload_len as usize).min(L7_PAYLOAD_LEN);
            if let Some(mut fp) = tls_fingerprint(&ev.payload[..n]) {
                fp.ts = rec.ts.clone();
                fp.source = "tap".into();
                fp.iface = rec.iface.clone();
                fp.workload = rec.workload.clone();
                fp.client = format!("{}:{}", rec.client, rec.client_port);
                fp.server = rec.server.clone();
                fp.server_port = rec.server_port;
                s.counters.tls_fingerprints += 1;
                Shared::push_capped(&mut s.tls_fp, fp, TLS_STORE_CAP);
            }
        }
        s.counters.l7_events += 1;
        Shared::push_capped(&mut s.l7, rec.clone(), L7_STORE_CAP);
        rec
    };
    publish(bus, "l7", &rec);
}

fn proc_kind(k: u32) -> &'static str {
    match k {
        PROC_EV_EXEC => "exec",
        PROC_EV_EXIT => "exit",
        PROC_EV_FORK => "fork",
        PROC_EV_FILE_OPEN => "file_open",
        PROC_EV_CONNECT => "connect",
        PROC_EV_CAP_DENIED => "cap_denied",
        _ => "unknown",
    }
}

pub(super) fn on_proc(sh: &SharedState, bus: &broadcast::Sender<StreamEvent>, b: &[u8]) {
    let Some(ev) = decode::<ProcEvent>(b) else {
        return;
    };
    let path = cstr(&ev.path);
    let comm = cstr(&ev.comm);
    let (ppid, cmdline) = if ev.kind == PROC_EV_EXEC {
        attribution::proc_details(ev.tgid)
    } else {
        (None, None)
    };
    let mut anomaly = None;
    let rec = {
        let mut s = lock(sh);
        let cgroup = s.cgroups.lookup(ev.cgroup_id);
        let info = cgroup
            .as_deref()
            .map(attribution::classify_cgroup)
            .unwrap_or_default();
        let denied = ev.flags & PROC_FLAG_EXEC_DENIED != 0;
        let is_connect = ev.kind == PROC_EV_CONNECT;
        let rec = ProcRecord {
            workload: cgroup.as_deref().and_then(|p| s.cgroup_workload(p)),
            ts: mono_to_rfc3339(ev.ts_ns),
            kind: proc_kind(ev.kind).into(),
            pid: ev.pid,
            tgid: ev.tgid,
            ppid,
            child_pid: (ev.kind == PROC_EV_FORK).then_some(ev.child_pid),
            uid: ev.uid,
            gid: ev.gid,
            comm: comm.clone(),
            path: (!path.is_empty()).then(|| path.clone()),
            cmdline,
            cgroup_id: ev.cgroup_id,
            cgroup,
            unit: info.unit,
            vm: info.vm.clone(),
            container: info.container,
            denied,
            killed: ev.flags & PROC_FLAG_EXEC_KILLED != 0,
            policy_id: denied
                .then(|| s.policy_labels.get(&ev.policy_id).cloned())
                .flatten(),
            open_flags: (ev.kind == PROC_EV_FILE_OPEN).then_some(ev.open_flags),
            capability: (ev.kind == PROC_EV_CAP_DENIED).then(|| policy::cap_name(ev.open_flags)),
            proto: is_connect.then(|| proto_name(ev.proto as u8).to_string()),
            saddr: is_connect.then(|| fmt_addr(&ev.saddr)),
            sport: is_connect.then_some(ev.sport),
            daddr: is_connect.then(|| fmt_addr(&ev.daddr)),
            dport: is_connect.then_some(ev.dport),
        };
        s.counters.proc_events += 1;
        if ev.kind == PROC_EV_EXEC {
            let ctx = Ctx {
                vm: info.vm,
                iface: None,
            };
            anomaly = s.detector.on_exec(wall_now(), &ctx, &path, &comm, ev.tgid);
        }
        Shared::push_capped(&mut s.procs, rec.clone(), PROC_STORE_CAP);
        if let Some(a) = &anomaly {
            s.counters.anomalies += 1;
            Shared::push_capped(&mut s.anomalies, a.clone(), ANOMALY_STORE_CAP);
        }
        rec
    };
    publish(bus, "proc", &rec);
    if let Some(a) = anomaly {
        publish(bus, "anomaly", &a);
    }
}

pub(super) fn on_capture(sh: &SharedState, b: &[u8]) {
    let Some(ev) = decode::<CaptureEvent>(b) else {
        return;
    };
    let n = (ev.cap_len as usize).min(CAPTURE_SNAPLEN);
    let mut s = lock(sh);
    let Some(c) = s
        .captures
        .values_mut()
        .find(|c| c.ifindex == ev.ifindex && !c.info.done)
    else {
        return;
    };
    if c.writer.packets() as usize >= c.info.max_packets {
        return;
    }
    c.writer
        .push(mono_to_epoch_us(ev.ts_ns), ev.pkt_len, &ev.data[..n]);
    c.info.packets = c.writer.packets();
    c.info.bytes = c.writer.bytes();
    s.counters.capture_packets += 1;
}

/// JA3/JA4 for a captured ClientHello (`None` if it is not one).
pub(super) fn tls_fingerprint(data: &[u8]) -> Option<TlsFingerprint> {
    let h = crate::l7::parse_hello(data)?;
    let (ja3, ja3_hash) = crate::l7::ja3(&h);
    let ja4 = crate::l7::ja4(&h);
    let version = crate::l7::parse_client_hello(data)
        .map(|t| t.version)
        .unwrap_or_default();
    Some(TlsFingerprint {
        truncated: !h.complete,
        sni: h.sni,
        alpn: h.alpn,
        tls_version: version,
        ja3,
        ja3_hash,
        ja4,
        ..TlsFingerprint::default()
    })
}

pub(super) fn on_tlsfp(sh: &SharedState, bus: &broadcast::Sender<StreamEvent>, b: &[u8]) {
    let Some(ev) = decode::<TlsFpEvent>(b) else {
        return;
    };
    let n = (ev.cap_len as usize).min(TLSFP_LEN);
    let Some(mut fp) = tls_fingerprint(&ev.data[..n]) else {
        return;
    };
    fp.ts = mono_to_rfc3339(ev.ts_ns);
    fp.source = "host".into();
    fp.client = format!("{}:{}", fmt_addr(&ev.src), ev.sport);
    fp.server = fmt_addr(&ev.dst);
    fp.server_port = ev.dport;
    fp.truncated |= ev.len as usize > n;
    {
        let mut s = lock(sh);
        let path = s.cgroups.lookup(ev.cgroup);
        fp.workload = path.as_deref().and_then(|p| s.cgroup_workload(p));
        fp.cgroup = path;
        s.counters.tls_fingerprints += 1;
        Shared::push_capped(&mut s.tls_fp, fp.clone(), TLS_STORE_CAP);
    }
    publish(bus, "tls", &fp);
}

/// HTTP metadata only; the plaintext itself is dropped here.
pub(super) fn ssl_record(ev: &SslEvent) -> SslRecord {
    let n = (ev.cap_len as usize).min(SSL_DATA_LEN);
    let data = &ev.data[..n];
    let mut rec = SslRecord {
        ts: mono_to_rfc3339(ev.ts_ns),
        pid: (ev.pid_tgid >> 32) as u32,
        comm: cstr(&ev.comm),
        direction: if ev.dir == SSL_DIR_WRITE {
            "write"
        } else {
            "read"
        }
        .into(),
        bytes: ev.len,
        protocol: "other".into(),
        ..SslRecord::default()
    };
    if let Some(h) = crate::l7::parse_http(data) {
        rec.protocol = "http1".into();
        rec.method = Some(h.method);
        rec.path = Some(h.path);
        rec.host = h.host;
    } else if let Some(rest) = data.strip_prefix(b"HTTP/1.") {
        rec.protocol = "http1".into();
        rec.status = rest
            .get(2..5)
            .and_then(|c| std::str::from_utf8(c).ok())
            .and_then(|c| c.parse().ok());
    } else if data.starts_with(b"PRI * HTTP/2.0") {
        rec.protocol = "http2".into();
    }
    rec
}

pub(super) fn on_ssl(sh: &SharedState, bus: &broadcast::Sender<StreamEvent>, b: &[u8]) {
    let Some(ev) = decode::<SslEvent>(b) else {
        return;
    };
    let mut rec = ssl_record(&ev);
    let cgroup = attribution::pid_cgroup(rec.pid);
    {
        let mut s = lock(sh);
        rec.workload = cgroup.as_deref().and_then(|p| s.cgroup_workload(p));
        s.counters.ssl_events += 1;
        Shared::push_capped(&mut s.ssl, rec.clone(), SSL_STORE_CAP);
    }
    publish(bus, "ssl", &rec);
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Engine operations behind each [`Request`].

use super::*;

const DEFAULT_LEASE_SECS: u64 = 15 * 60;
const MIN_LEASE_SECS: u64 = 60;
const MAX_LEASE_SECS: u64 = 24 * 3600;
const DNS_BLOCK_CAP: usize = 16384;

fn flag_names(flags: u32) -> Vec<String> {
    [
        (IF_GUEST_SIDE, "guest_side"),
        (IF_DENY, "deny"),
        (IF_ALLOW, "allow"),
        (IF_FLOWS, "flows"),
        (IF_CAPTURE, "capture"),
        (IF_QOS, "qos"),
        (IF_DNS, "dns"),
        (IF_RATE, "rate_limit"),
        (IF_L7, "l7"),
    ]
    .iter()
    .filter(|(f, _)| flags & f != 0)
    .map(|(_, n)| n.to_string())
    .collect()
}

impl Engine {
    // ---- policies --------------------------------------------------------

    fn install(&mut self, num: u32, scope: u32, rules: &[Rule]) -> Result<()> {
        for r in rules {
            match r {
                Rule::DenyCidr(p) => self.dp.deny_insert(scope, p, num)?,
                Rule::Allow {
                    proto,
                    port,
                    prefix,
                } => self.dp.allow_insert(scope, *proto, *port, prefix, num)?,
                Rule::DenyPort { proto, port } => self.dp.port_insert(scope, *proto, *port, num)?,
                Rule::ExecDeny { hash, .. } => self.dp.exec_insert(*hash, num)?,
                Rule::CapDeny { cap } => self.dp.cap_insert(scope, *cap, num)?,
                Rule::FileDeny { .. } => {}
                Rule::DnsDeny { suffix } => {
                    lock(&self.shared)
                        .dns_denies
                        .insert(num, (suffix.clone(), scope));
                }
                Rule::ConnRate { per_sec, burst } => {
                    self.dp.rate_set(scope, *per_sec, *burst, num)?
                }
            }
        }
        Ok(())
    }

    /// One RATE_CFG slot per scope: after removing a limit, reinstate any
    /// other enabled limit on the same scope.
    fn reinstate_rate(&mut self, scope: u32) {
        let other = self.policies.values().find_map(|c| {
            if !c.policy.enabled || c.scope_id != scope {
                return None;
            }
            c.rules.iter().find_map(|r| match r {
                Rule::ConnRate { per_sec, burst } => Some((
                    *per_sec,
                    *burst,
                    self.policy_nums.get(&c.policy.id).copied().unwrap_or(0),
                )),
                _ => None,
            })
        });
        match other {
            Some((p, b, n)) => {
                let _ = self.dp.rate_set(scope, p, b, n);
            }
            None => self.dp.rate_remove(scope),
        }
    }

    fn uninstall(&mut self, num: u32, scope: u32, rules: &[Rule]) {
        for r in rules {
            match r {
                Rule::DenyCidr(p) => self.dp.deny_remove(scope, p),
                Rule::Allow {
                    proto,
                    port,
                    prefix,
                } => self.dp.allow_remove(scope, *proto, *port, prefix),
                Rule::DenyPort { proto, port } => self.dp.port_remove(scope, *proto, *port),
                Rule::ExecDeny { hash, .. } => self.dp.exec_remove(*hash),
                Rule::CapDeny { cap } => self.dp.cap_remove(scope, *cap),
                Rule::FileDeny { .. } => {}
                Rule::ConnRate { .. } => self.reinstate_rate(scope),
                Rule::DnsDeny { .. } => {
                    lock(&self.shared).dns_denies.remove(&num);
                    let gone: Vec<(u32, Prefix)> = self
                        .dns_blocked
                        .iter()
                        .filter(|(_, n)| **n == num)
                        .map(|(k, _)| *k)
                        .collect();
                    for (s, p) in gone {
                        self.dns_blocked.remove(&(s, p));
                        if !self.static_deny_has(s, &p) {
                            self.dp.deny_remove(s, &p);
                        }
                    }
                }
            }
        }
    }

    /// A static deny_ip rule owns this exact prefix (don't remove it with DNS state).
    fn static_deny_has(&self, scope: u32, p: &Prefix) -> bool {
        self.policies.values().any(|c| {
            c.policy.enabled
                && c.scope_id == scope
                && c.rules
                    .iter()
                    .any(|r| matches!(r, Rule::DenyCidr(x) if x == p))
        })
    }

    fn after_policy_change(&mut self) -> Result<()> {
        self.push_file_watch()?;
        self.refresh_global()?;
        let scopes: Vec<u32> = std::iter::once(0)
            .chain(self.scopes.values().map(|s| s.id))
            .collect();
        for s in scopes {
            self.refresh_scope_flags(s)?;
        }
        self.reprogram_all_ifaces()
    }

    pub(super) fn apply_policy(&mut self, mut p: Policy) -> Result<Policy> {
        if !POLICY_KINDS.contains(&p.kind.as_str()) {
            return Err(anyhow!(
                "policy kind {:?} is not supported natively (supported: {})",
                p.kind,
                POLICY_KINDS.join(", ")
            ));
        }
        let rules = policy::compile(&p.kind, &p.match_value).map_err(|e| anyhow!(e))?;
        if p.id.is_empty() {
            p.id = format!("bpf-{:x}", loader::monotonic_ns());
        }
        if p.created_at.is_none() {
            p.created_at = Some(chrono::Utc::now().to_rfc3339());
        }
        if let Some(old) = self.policies.remove(&p.id) {
            let num = self.policy_nums[&old.policy.id];
            if old.policy.enabled {
                self.uninstall(num, old.scope_id, &old.rules);
            }
        }
        let scope_id = self.scope_id_for(&p.scope)?;
        let num = match self.policy_nums.get(&p.id) {
            Some(n) => *n,
            None => {
                let n = self.next_policy_num;
                self.next_policy_num += 1;
                self.policy_nums.insert(p.id.clone(), n);
                n
            }
        };
        lock(&self.shared).policy_labels.insert(num, p.id.clone());
        if p.enabled {
            self.install(num, scope_id, &rules)?;
        }
        self.policies.insert(
            p.id.clone(),
            CompiledPolicy {
                policy: p.clone(),
                scope_id,
                rules,
            },
        );
        self.after_policy_change()?;
        Ok(p)
    }

    pub(super) fn remove_policy(&mut self, id: &str) -> Result<bool> {
        let Some(old) = self.policies.remove(id) else {
            return Ok(false);
        };
        let num = self.policy_nums[id];
        if old.policy.enabled {
            self.uninstall(num, old.scope_id, &old.rules);
        }
        self.after_policy_change()?;
        Ok(true)
    }

    pub(super) fn list_policies(&self) -> Vec<Policy> {
        let mut v: Vec<Policy> = self.policies.values().map(|c| c.policy.clone()).collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v
    }

    /// Drain resolved DNS-deny answers queued by the DNS reader into the deny trie.
    pub(super) fn drain_dns_blocks(&mut self) -> Result<()> {
        let queue = std::mem::take(&mut lock(&self.shared).dns_block_queue);
        if queue.is_empty() {
            return Ok(());
        }
        let mut changed = false;
        for (scope, prefix, num) in queue {
            if self.dns_blocked.len() >= DNS_BLOCK_CAP
                || self.dns_blocked.contains_key(&(scope, prefix))
            {
                continue;
            }
            self.dp.deny_insert(scope, &prefix, num)?;
            self.dns_blocked.insert((scope, prefix), num);
            changed = true;
        }
        if changed {
            let scopes: Vec<u32> = std::iter::once(0)
                .chain(self.scopes.values().map(|s| s.id))
                .collect();
            for s in scopes {
                self.refresh_scope_flags(s)?;
            }
            self.reprogram_all_ifaces()?;
        }
        Ok(())
    }

    // ---- mode / lease ----------------------------------------------------

    pub(super) fn set_mode(&mut self, mode: Mode, lease_secs: Option<u64>) -> Result<ModeState> {
        match mode {
            Mode::Enforce => {
                let secs = lease_secs
                    .unwrap_or(DEFAULT_LEASE_SECS)
                    .clamp(MIN_LEASE_SECS, MAX_LEASE_SECS);
                self.lease_deadline_mono = loader::monotonic_ns() + secs * 1_000_000_000;
                self.lease_wall = Some(chrono::Utc::now() + chrono::Duration::seconds(secs as i64));
            }
            Mode::Observe => {
                self.lease_deadline_mono = 0;
                self.lease_wall = None;
            }
        }
        self.mode = mode;
        self.lease_lapsed = false;
        self.refresh_global()?;
        self.direct_push_cfg()?;
        Ok(self.mode_state())
    }

    pub(super) fn mode_state(&self) -> ModeState {
        let now = loader::monotonic_ns();
        let active = self.mode == Mode::Enforce && now < self.lease_deadline_mono;
        ModeState {
            mode: if active { Mode::Enforce } else { Mode::Observe },
            lease_expires_at: active
                .then(|| self.lease_wall.map(|w| w.to_rfc3339()))
                .flatten(),
            lease_remaining_secs: active.then(|| (self.lease_deadline_mono - now) / 1_000_000_000),
            lease_expired: self.lease_lapsed || (self.mode == Mode::Enforce && !active),
            covers: [
                "policies",
                "shield",
                "vm_edge",
                "vm_sandbox",
                "node_isolation",
                "direct",
            ]
            .map(String::from)
            .to_vec(),
        }
    }

    /// The datapath fails open on its own at the deadline; this mirrors it in
    /// engine state so status reflects reality.
    pub(super) fn expire_lease(&mut self) -> Result<()> {
        if self.mode == Mode::Enforce && loader::monotonic_ns() >= self.lease_deadline_mono {
            tracing::warn!("enforcement lease lapsed; reverting to observe");
            self.mode = Mode::Observe;
            self.lease_deadline_mono = 0;
            self.lease_wall = None;
            self.lease_lapsed = true;
            self.refresh_global()?;
            self.direct_push_cfg()?;
        }
        Ok(())
    }

    // ---- interfaces / QoS ------------------------------------------------

    pub(super) fn interfaces(&mut self) -> Vec<IfaceStatus> {
        let mut out: Vec<IfaceStatus> = Vec::new();
        let entries: Vec<(u32, IfaceRuntime)> =
            self.ifaces.iter().map(|(i, r)| (*i, r.clone())).collect();
        for (idx, r) in entries {
            let cfg = self.dp.iface_cfg(idx).unwrap_or_default();
            out.push(IfaceStatus {
                name: r.name.clone(),
                ifindex: idx,
                vm: r.vm.clone(),
                mac: r.mac.clone(),
                scope: cfg.scope,
                flags: flag_names(cfg.flags),
                guest_side: r.guest_side,
                xdp: r.xdp,
                qos_egress_bps: r.qos_egress_bps,
                qos_ingress_bps: r.qos_ingress_bps,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub(super) fn attach_interface(
        &mut self,
        name: &str,
        guest_side: bool,
        xdp: bool,
    ) -> Result<()> {
        let vms = attribution::scan_libvirt();
        let vm = vms.get(name);
        self.add_iface(
            name,
            guest_side,
            xdp,
            vm.map(|v| v.vm.clone()),
            vm.and_then(|v| v.mac.clone()),
        )?;
        self.explicit.insert(name.to_string(), (guest_side, xdp));
        Ok(())
    }

    pub(super) fn detach_interface(&mut self, name: &str) {
        self.explicit.remove(name);
        self.remove_iface(name);
    }

    pub(super) fn set_qos(
        &mut self,
        iface: Option<&str>,
        vm: Option<&str>,
        egress: u64,
        ingress: u64,
    ) -> Result<Vec<String>> {
        if iface.is_none() && vm.is_none() {
            return Err(anyhow!("set_qos needs iface or vm"));
        }
        let targets: Vec<u32> = self
            .ifaces
            .iter()
            .filter(|(_, r)| {
                iface.is_some_and(|i| i == r.name) || (vm.is_some() && r.vm.as_deref() == vm)
            })
            .map(|(i, _)| *i)
            .collect();
        if targets.is_empty() {
            return Err(anyhow!("no attached interface matches"));
        }
        let mut names = Vec::new();
        for idx in targets {
            if let Some(r) = self.ifaces.get_mut(&idx) {
                r.qos_egress_bps = egress;
                r.qos_ingress_bps = ingress;
                names.push(r.name.clone());
                // EDT timestamps are only honoured by the fq qdisc.
                if ingress > 0 || egress > 0 {
                    let _ = std::process::Command::new("tc")
                        .args(["qdisc", "replace", "dev", &r.name, "root", "fq"])
                        .status();
                }
            }
            if let Some(vm) = vm {
                self.qos_by_vm.insert(vm.to_string(), (egress, ingress));
            }
            self.program_iface(idx)?;
        }
        Ok(names)
    }

    /// Re-scan links: attach new taps matching the patterns, forget vanished
    /// ones; then follow VMs for the VM edge and the QEMU sandbox.
    pub(super) fn rescan_ifaces(&mut self) {
        self.rescan_links();
        self.quarantine_expire();
        self.vm_edge_refresh();
        self.sandbox_refresh();
        self.tls_refresh();
        self.vmi_refresh();
        if self.guard.config.enabled {
            self.guard_refresh();
        }
    }

    fn rescan_links(&mut self) {
        let links: std::collections::HashSet<String> = list_links().into_iter().collect();
        let gone: Vec<(u32, String)> = self
            .ifaces
            .iter()
            .filter(|(_, r)| !links.contains(&r.name))
            .map(|(i, r)| (*i, r.name.clone()))
            .collect();
        for (idx, name) in gone {
            self.retire_iface_stats(idx);
            self.dp.forget_tc(&name);
            self.dp.remove_iface_cfg(idx);
            self.ifaces.remove(&idx);
            lock(&self.shared).ifaces.remove(&idx);
        }
        let known: std::collections::HashSet<String> =
            self.ifaces.values().map(|r| r.name.clone()).collect();
        let fresh: Vec<String> = links
            .into_iter()
            .filter(|n| !known.contains(n))
            .filter(|n| {
                self.explicit.contains_key(n)
                    || self
                        .telemetry
                        .iface_patterns
                        .iter()
                        .any(|p| policy::glob_match(p, n))
            })
            .collect();
        if fresh.is_empty() {
            return;
        }
        let vms = attribution::scan_libvirt();
        for name in fresh {
            let (guest_side, xdp) = self.explicit.get(&name).copied().unwrap_or((true, false));
            let vm = vms.get(&name);
            let vm_name = vm.map(|v| v.vm.clone());
            if let Err(e) = self.add_iface(
                &name,
                guest_side,
                xdp,
                vm_name.clone(),
                vm.and_then(|v| v.mac.clone()),
            ) {
                tracing::debug!("attach {name}: {e:#}");
                continue;
            }
            if let Some((eg, ing)) = vm_name.and_then(|v| self.qos_by_vm.get(&v).copied()) {
                if let Some(idx) = if_nametoindex(&name) {
                    if let Some(r) = self.ifaces.get_mut(&idx) {
                        r.qos_egress_bps = eg;
                        r.qos_ingress_bps = ing;
                    }
                    let _ = self.program_iface(idx);
                }
            }
        }
    }

    // ---- telemetry -------------------------------------------------------

    pub(super) fn set_telemetry(&mut self, t: TelemetryConfig) -> Result<TelemetryConfig> {
        self.telemetry = t;
        self.push_file_watch()?;
        self.refresh_global()?;
        self.reprogram_all_ifaces()?;
        self.rescan_ifaces();
        self.sync_sockops();
        Ok(self.telemetry.clone())
    }

    // ---- flows / health --------------------------------------------------

    pub(super) fn flows(&mut self, limit: usize, vm: Option<&str>) -> Result<Vec<FlowRecord>> {
        let raw = self.dp.dump_flows()?;
        let sh = lock(&self.shared);
        let mut out: Vec<FlowRecord> = raw
            .into_iter()
            .filter_map(|(k, v)| {
                let r = self.ifaces.get(&k.ifindex);
                if vm.is_some() && r.and_then(|r| r.vm.as_deref()) != vm {
                    return None;
                }
                Some(FlowRecord {
                    workload: sh.iface_workload(k.ifindex),
                    iface: r
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| format!("if{}", k.ifindex)),
                    ifindex: k.ifindex,
                    vm: r.and_then(|r| r.vm.clone()),
                    proto: proto_name(k.proto).to_string(),
                    local: fmt_addr(&k.local),
                    local_port: k.local_port,
                    remote: fmt_addr(&k.remote),
                    remote_port: k.remote_port,
                    origin: if v.origin == ORIGIN_LOCAL {
                        "local"
                    } else {
                        "remote"
                    }
                    .into(),
                    tx_pkts: v.tx_pkts,
                    tx_bytes: v.tx_bytes,
                    rx_pkts: v.rx_pkts,
                    rx_bytes: v.rx_bytes,
                    first_seen: mono_to_rfc3339(v.first_ns),
                    last_seen: mono_to_rfc3339(v.last_ns),
                    verdict: match v.verdict as u32 {
                        VERDICT_DROP => "drop",
                        VERDICT_OBSERVED => "observed",
                        _ => "pass",
                    }
                    .into(),
                    tcp_flags: v.tcp_flags,
                })
            })
            .collect();
        out.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        out.truncate(limit);
        Ok(out)
    }

    pub(super) fn net_health(&mut self) -> Result<NetHealth> {
        let mut drop_reasons: Vec<DropReason> = self
            .dp
            .drop_reasons()?
            .into_iter()
            .map(|(reason, count)| DropReason {
                reason,
                name: self
                    .drop_names
                    .get(&reason)
                    .cloned()
                    .unwrap_or_else(|| format!("reason_{reason}")),
                count,
            })
            .collect();
        drop_reasons.sort_by_key(|d| std::cmp::Reverse(d.count));
        let mut tcp: Vec<TcpHealth> = self
            .dp
            .tcp_health()?
            .into_iter()
            .map(|(k, count)| TcpHealth {
                kind: match k.kind {
                    HEALTH_RETRANSMIT => "retransmit",
                    HEALTH_RST_SENT => "rst_sent",
                    HEALTH_RST_RECV => "rst_recv",
                    _ => "other",
                }
                .into(),
                addr: fmt_addr(&k.addr),
                count,
            })
            .collect();
        tcp.sort_by_key(|t| std::cmp::Reverse(t.count));
        tcp.truncate(200);
        Ok(NetHealth {
            drop_reasons,
            tcp,
            connect: self.connect_health(),
            pressure: self.tcp_pressure(),
            sockops: self.dp.sockops_attached().map(String::from),
        })
    }

    /// Age out idle flows and feed byte counters to the volume detector.
    /// Rebuild pod attribution from CNI endpoints and the kubepods cgroups.
    pub(super) fn refresh_workloads(&mut self) {
        let mut sandboxes = HashMap::new();
        let mut ifaces = HashMap::new();
        for ep in self.cni.endpoints.values() {
            let Some(pod) = ep.pod.as_deref().and_then(attribution::split_pod) else {
                continue;
            };
            if let Some(id) = &ep.container_id {
                sandboxes.insert(id.clone(), pod.clone());
            }
            ifaces.insert(ep.host_iface.clone(), pod);
        }
        let mut pods = attribution::pod_log_index(Path::new(attribution::KUBELET_POD_LOGS));
        pods.extend(attribution::pod_index(
            Path::new(attribution::CGROUP_ROOT),
            &sandboxes,
        ));
        let mut sh = lock(&self.shared);
        sh.pods = pods;
        sh.pod_ifaces = ifaces;
    }

    pub(super) fn sweep_flows(&mut self, idle_ns: u64) -> Result<()> {
        let now = loader::monotonic_ns();
        let raw = self.dp.dump_flows()?;
        let wall = chrono::Utc::now().timestamp() as f64;
        let mut stale = Vec::new();
        let mut anomalies = Vec::new();
        {
            let mut sh = lock(&self.shared);
            for (k, v) in &raw {
                if now.saturating_sub(v.last_ns) > idle_ns {
                    stale.push(*k);
                    continue;
                }
                if v.origin != ORIGIN_LOCAL {
                    continue;
                }
                let (iface, vm) = sh.iface(k.ifindex);
                let ctx = Ctx { vm, iface };
                let local = fmt_addr(&k.local);
                let remote = fmt_addr(&k.remote);
                let fk = format!(
                    "{}|{}|{}:{}|{}:{}",
                    k.ifindex, k.proto, local, k.local_port, remote, k.remote_port
                );
                if let Some(a) = sh
                    .detector
                    .on_flow_bytes(wall, &ctx, &fk, &local, &remote, v.tx_bytes)
                {
                    anomalies.push(a);
                }
            }
            sh.detector.prune(wall);
            for a in &anomalies {
                sh.counters.anomalies += 1;
                Shared::push_capped(&mut sh.anomalies, a.clone(), ANOMALY_STORE_CAP);
            }
        }
        for a in anomalies {
            publish(&self.bus, "anomaly", &a);
        }
        for k in stale {
            self.dp.remove_flow(&k);
        }
        Ok(())
    }

    // ---- accounting ------------------------------------------------------

    /// Move datapath counter deltas into the persisted per-workload totals.
    pub(super) fn fold_accounting(&mut self) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        for (idx, s) in self.dp.iface_stats()? {
            let Some(r) = self.ifaces.get(&idx) else {
                continue;
            };
            let key = acct_key(r);
            let before = self.acct_offset.get(&idx).copied().unwrap_or_default();
            self.acct_base
                .entry(key.clone())
                .or_default()
                .add_delta(&s, &before);
            self.acct_offset.insert(idx, s);
            self.acct_since.entry(key).or_insert_with(|| now.clone());
        }
        Ok(())
    }

    pub(super) fn retire_iface_stats(&mut self, idx: u32) {
        if let Err(e) = self.fold_accounting() {
            tracing::debug!("fold accounting: {e:#}");
        }
        self.acct_offset.remove(&idx);
        self.dp.remove_iface_stats(idx);
    }

    pub(super) fn accounting(&mut self, vm: Option<&str>) -> Result<Vec<AccountingRecord>> {
        self.fold_accounting()?;
        let mut out: Vec<AccountingRecord> = self
            .acct_base
            .iter()
            .filter(|(k, _)| vm.is_none_or(|v| k.as_str() == v))
            .map(|(k, t)| {
                let mut interfaces: Vec<String> = self
                    .ifaces
                    .values()
                    .filter(|r| acct_key(r) == *k)
                    .map(|r| r.name.clone())
                    .collect();
                interfaces.sort();
                let iface_only = k.strip_prefix("iface:");
                if interfaces.is_empty() {
                    if let Some(i) = iface_only {
                        interfaces.push(i.to_string());
                    }
                }
                AccountingRecord {
                    vm: iface_only.is_none().then(|| k.clone()),
                    interfaces,
                    tx_bytes: t.tx_bytes,
                    rx_bytes: t.rx_bytes,
                    tx_pkts: t.tx_pkts,
                    rx_pkts: t.rx_pkts,
                    drops: t.drops,
                    since: self.acct_since.get(k).cloned().unwrap_or_default(),
                }
            })
            .collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.tx_bytes + a.rx_bytes));
        Ok(out)
    }

    /// Zero the totals (all workloads, or one VM) and restart their window.
    pub(super) fn reset_accounting(&mut self, vm: Option<&str>) -> Result<usize> {
        self.fold_accounting()?;
        let now = chrono::Utc::now().to_rfc3339();
        let keys: Vec<String> = self
            .acct_base
            .keys()
            .filter(|k| vm.is_none_or(|v| k.as_str() == v))
            .cloned()
            .collect();
        for k in &keys {
            self.acct_base.insert(k.clone(), AcctTotals::default());
            self.acct_since.insert(k.clone(), now.clone());
        }
        Ok(keys.len())
    }

    // ---- capture ---------------------------------------------------------

    pub(super) fn capture_start(
        &mut self,
        iface: &str,
        duration_secs: Option<u64>,
        sample: Option<u32>,
        snaplen: Option<u32>,
        max_packets: Option<usize>,
    ) -> Result<CaptureInfo> {
        let idx = self
            .ifaces
            .iter()
            .find(|(_, r)| r.name == iface)
            .map(|(i, _)| *i)
            .ok_or_else(|| anyhow!("interface {iface} is not attached"))?;
        let dur = duration_secs.unwrap_or(30).clamp(1, 600);
        let snap = snaplen
            .unwrap_or(CAPTURE_SNAPLEN as u32)
            .clamp(64, CAPTURE_SNAPLEN as u32);
        let sample = sample.unwrap_or(1).max(1);
        let maxp = max_packets.unwrap_or(10_000).clamp(1, 200_000);
        self.capture_seq += 1;
        let now = chrono::Utc::now();
        let info = CaptureInfo {
            id: format!("cap-{}-{}", now.timestamp(), self.capture_seq),
            iface: iface.to_string(),
            vm: self.ifaces[&idx].vm.clone(),
            started_at: now.to_rfc3339(),
            ends_at: (now + chrono::Duration::seconds(dur as i64)).to_rfc3339(),
            packets: 0,
            bytes: 0,
            max_packets: maxp,
            sample,
            snaplen: snap,
            done: false,
        };
        {
            let mut sh = lock(&self.shared);
            if sh
                .captures
                .values()
                .any(|c| c.ifindex == idx && !c.info.done)
            {
                return Err(anyhow!("a capture is already running on {iface}"));
            }
            sh.captures.insert(
                info.id.clone(),
                ActiveCapture {
                    info: info.clone(),
                    ifindex: idx,
                    writer: PcapngWriter::new(iface, snap),
                    deadline: Instant::now() + Duration::from_secs(dur),
                },
            );
        }
        self.program_iface(idx)?;
        let mut cfg = self.dp.iface_cfg(idx)?;
        cfg.capture_sample = sample;
        cfg.capture_snaplen = snap;
        self.dp.set_iface_cfg(idx, cfg)?;
        Ok(info)
    }

    /// Finish captures past their deadline/packet budget; drop old finished ones.
    pub(super) fn sweep_captures(&mut self) -> Result<()> {
        let now = Instant::now();
        let mut stop = Vec::new();
        {
            let mut sh = lock(&self.shared);
            for c in sh.captures.values_mut() {
                if !c.info.done
                    && (now >= c.deadline || c.writer.packets() as usize >= c.info.max_packets)
                {
                    c.info.done = true;
                    stop.push(c.ifindex);
                }
            }
            let expired: Vec<String> = sh
                .captures
                .iter()
                .filter(|(_, c)| {
                    c.info.done && now.duration_since(c.deadline) > Duration::from_secs(3600)
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in expired {
                sh.captures.remove(&k);
            }
        }
        for idx in stop {
            self.program_iface(idx)?;
        }
        Ok(())
    }

    // ---- status ----------------------------------------------------------

    pub(super) fn status(&mut self) -> BpfStatus {
        let interfaces = self.interfaces();
        let counters = lock(&self.shared).counters.clone();
        BpfStatus {
            available: true,
            programs_compiled: self.programs_compiled,
            version: crate::VERSION.to_string(),
            features: self.features.clone(),
            mode: self.mode_state(),
            policies_total: self.policies.len(),
            policies_enabled: self.policies.values().filter(|p| p.policy.enabled).count(),
            interfaces,
            cgroups: self.dp.cgroups(),
            tracepoints: self.dp.tracepoints.clone(),
            counters,
            telemetry: self.telemetry.clone(),
            notes: self.dp.notes.clone(),
        }
    }
}

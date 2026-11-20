// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Network change audit: `mn_rtnl` (kprobe on rtnetlink_rcv_msg).

use super::*;
use crate::rtnl;

const PROG: &str = "mn_rtnl";
const KFUNC: &str = "rtnetlink_rcv_msg";
pub(super) const RTNL_STORE_CAP: usize = 2000;

#[derive(Default)]
pub(super) struct RtnlRuntime {
    pub config: RtnlConfig,
    note: Option<String>,
    /// (skb.sk, sock net, net inum) from BTF; resolved once.
    offsets: Option<Option<(u32, u32, u32)>>,
}

pub(super) fn self_netns() -> Option<u64> {
    std::fs::read_link("/proc/self/ns/net")
        .ok()
        .and_then(|l| rtnl::ns_inode(&l.to_string_lossy()))
}

fn layout() -> Option<(u32, u32, u32)> {
    let btf = crate::btf::Btf::from_sys_fs().ok()?;
    Some((
        btf.offset("sk_buff", "sk")?,
        btf.offset("sock", "__sk_common.skc_net.net")?,
        btf.offset("net", "ns.inum")?,
    ))
}

fn ifname(idx: u32) -> Option<String> {
    let mut buf = [0 as libc::c_char; libc::IF_NAMESIZE];
    let p = unsafe { libc::if_indextoname(idx, buf.as_mut_ptr()) };
    if p.is_null() {
        return None;
    }
    Some(
        unsafe { std::ffi::CStr::from_ptr(p) }
            .to_string_lossy()
            .into_owned(),
    )
}

impl Engine {
    pub(super) fn rtnl_configure(&mut self, cfg: RtnlConfig) -> Result<RtnlStatus> {
        let mask = rtnl::kinds_mask(&cfg.kinds).map_err(|e| anyhow!(e))?;
        self.rtnl.note = None;
        if cfg.enabled {
            if let Err(e) = self.dp.attach_kprobe(PROG, KFUNC) {
                let n = format!("network change audit unavailable: {e:#}");
                self.rtnl.note = Some(n.clone());
                self.rtnl.config = cfg;
                return Err(anyhow!(n));
            }
        } else {
            self.dp.detach_traces(&[PROG]);
        }
        let offs = *self.rtnl.offsets.get_or_insert_with(layout);
        if offs.is_none() {
            self.rtnl.note = Some(
                "kernel BTF lacks the netns layout: requests from every namespace are kept".into(),
            );
        }
        let (sk, net, inum) = offs.unwrap_or_default();
        let host = self_netns().unwrap_or(0) as u32;
        self.dp.array_set(
            "RTNL_CFG",
            0,
            RtnlCfg {
                enabled: cfg.enabled as u32,
                netns: if cfg.host_netns_only { host } else { 0 },
                type_mask: mask,
                off_skb_sk: sk,
                off_sk_net: net,
                off_net_inum: inum,
                _pad: 0,
            },
        )?;
        lock(&self.shared).rtnl_host_netns = (host != 0).then_some(host as u64);
        self.rtnl.config = cfg;
        Ok(self.rtnl_status())
    }

    pub(super) fn rtnl_status(&mut self) -> RtnlStatus {
        let mut sum = |i| {
            self.dp
                .percpu_array_sum::<u64>("RTNL_STATS", i, |a, b| *a += *b)
                .unwrap_or(0)
        };
        let (events, dropped) = (sum(RTNL_STAT_EVENTS), sum(RTNL_STAT_DROPPED));
        RtnlStatus {
            config: self.rtnl.config.clone(),
            attached: self.dp.trace_attached(&format!("{PROG}@{KFUNC}")),
            events,
            dropped,
            stored: lock(&self.shared).rtnl.len(),
            notes: self.rtnl.note.iter().cloned().collect(),
        }
    }
}

pub(super) fn on_rtnl(sh: &SharedState, bus: &broadcast::Sender<StreamEvent>, b: &[u8]) {
    if b.len() < std::mem::size_of::<RtnlEvent>() {
        return;
    }
    let ev: RtnlEvent = unsafe { std::ptr::read_unaligned(b.as_ptr() as *const RtnlEvent) };
    let netns = (ev.netns != 0).then_some(ev.netns as u64);
    let (kind, action) = rtnl::name(ev.nlmsg_type);
    let cstr = |b: &[u8]| {
        let n = b.iter().position(|c| *c == 0).unwrap_or(b.len());
        String::from_utf8_lossy(&b[..n]).into_owned()
    };
    let name_from_req = cstr(&ev.ifname);
    let (_, cmdline) = attribution::proc_details(ev.tgid);
    let rec = {
        let mut s = lock(sh);
        let host_netns = netns.zip(s.rtnl_host_netns).map(|(a, b)| a == b);
        let iface = if !name_from_req.is_empty() {
            Some(name_from_req)
        } else if ev.ifindex != 0 && host_netns != Some(false) {
            ifname(ev.ifindex).or_else(|| s.ifaces.get(&ev.ifindex).map(|m| m.name.clone()))
        } else {
            None
        };
        let cgroup = s.cgroups.lookup(ev.cgroup_id);
        let rec = RtnlRecord {
            ts: mono_to_rfc3339(ev.ts_ns),
            kind: kind.into(),
            action: action.into(),
            create: ev.nlmsg_flags & rtnl::NLM_F_CREATE != 0,
            ifindex: (ev.ifindex != 0).then_some(ev.ifindex),
            iface,
            dst: (kind == "route")
                .then(|| rtnl::route_dst(ev.family, &ev.dst, ev.dst_len))
                .flatten(),
            pid: ev.pid,
            tgid: ev.tgid,
            uid: ev.uid,
            comm: cstr(&ev.comm),
            cmdline,
            workload: cgroup.as_deref().and_then(|p| s.cgroup_workload(p)),
            cgroup,
            netns,
            host_netns,
        };
        Shared::push_capped(&mut s.rtnl, rec.clone(), RTNL_STORE_CAP);
        rec
    };
    publish(bus, "rtnl", &rec);
}

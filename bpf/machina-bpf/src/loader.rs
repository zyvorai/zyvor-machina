// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! aya loader for the embedded `machina-bpf-ebpf` object: program attach.
//! Map helpers live in `loader_maps.rs`.

use std::collections::{HashMap as StdHashMap, HashSet};
use std::fs::File;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use aya::programs::{
    cgroup_device::CgroupDeviceLinkId,
    cgroup_skb::CgroupSkbLinkId,
    cgroup_sock_addr::CgroupSockAddrLinkId,
    fentry::FEntryLinkId,
    kprobe::KProbeLinkId,
    lsm::LsmLinkId,
    sock_ops::SockOpsLinkId,
    tc::SchedClassifierLinkId,
    trace_point::TracePointLinkId,
    uprobe::{UProbeLinkId, UProbeScope},
    xdp::XdpLinkId,
    CgroupAttachMode, CgroupDevice, CgroupSkb, CgroupSkbAttachType, CgroupSockAddr, FEntry, KProbe,
    Lsm, SchedClassifier, SockOps, TcAttachType, TracePoint, UProbe, Xdp, XdpMode,
};
use aya::util::KernelVersion;
use aya::{Ebpf, EbpfLoader, VerifierLogLevel};

use crate::api::KernelFeatures;

static BPF_OBJECT: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/machina-bpf.o"));

/// True when the build staged a real eBPF object (nightly + bpf-linker were available).
pub fn programs_compiled() -> bool {
    !BPF_OBJECT.is_empty()
}

pub fn kernel_features() -> KernelFeatures {
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let tcx = KernelVersion::current()
        .map(|v| v.code() >= KernelVersion::new(6, 6, 0).code())
        .unwrap_or(false);
    let lsm_bpf = std::fs::read_to_string("/sys/kernel/security/lsm")
        .map(|s| s.split(',').any(|l| l.trim() == "bpf"))
        .unwrap_or(false);
    let btf = Path::new("/sys/kernel/btf/vmlinux").exists();
    let fentry = btf
        && KernelVersion::current()
            .map(|v| v.code() >= KernelVersion::new(5, 5, 0).code())
            .unwrap_or(false);
    KernelFeatures {
        kernel,
        btf,
        tcx,
        lsm_bpf,
        tracefs: crate::tracefs::tracefs_root().map(|p| p.display().to_string()),
        cgroup2: Path::new("/sys/fs/cgroup/cgroup.controllers").exists(),
        fentry,
        sched_ext: Path::new("/sys/kernel/sched_ext").is_dir(),
        sched_ext_state: machina_scx::kernel_state(),
        xsk: xsk_supported(),
    }
}

/// AF_XDP socket creation probe (CONFIG_XDP_SOCKETS; needs CAP_NET_RAW).
fn xsk_supported() -> bool {
    const AF_XDP: libc::c_int = 44;
    let fd = unsafe { libc::socket(AF_XDP, libc::SOCK_RAW | libc::SOCK_CLOEXEC, 0) };
    if fd >= 0 {
        unsafe { libc::close(fd) };
        return true;
    }
    // The bpfd unit's RestrictAddressFamilies hides AF_XDP from bpfd itself;
    // consumers open the sockets, so fall back to the kernel symbol table.
    std::fs::read_to_string("/proc/kallsyms")
        .map(|s| s.lines().any(|l| l.ends_with(" xsk_map_ops")))
        .unwrap_or(false)
}

/// Raise RLIMIT_MEMLOCK for pre-5.11 kernels (memcg accounting makes it moot later).
pub fn bump_memlock() {
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    unsafe {
        libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim);
    }
}

/// CLOCK_MONOTONIC in ns (same clock as `bpf_ktime_get_ns`).
pub fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Convert a `bpf_ktime_get_ns` timestamp to Unix epoch microseconds.
pub fn mono_to_epoch_us(mono_ns: u64) -> u64 {
    let now_mono = monotonic_ns();
    let now_wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    now_wall.saturating_sub(now_mono.saturating_sub(mono_ns)) / 1000
}

pub fn mono_to_rfc3339(mono_ns: u64) -> String {
    let us = mono_to_epoch_us(mono_ns) as i64;
    chrono::DateTime::from_timestamp_micros(us)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

struct TcLinks {
    ingress: SchedClassifierLinkId,
    egress: SchedClassifierLinkId,
}

enum TraceLink {
    FEntry(FEntryLinkId),
    KProbe(KProbeLinkId),
    Tp(TracePointLinkId),
    Lsm(LsmLinkId),
}

#[derive(Default)]
struct CgroupLinks {
    sock: Vec<(String, CgroupSockAddrLinkId)>,
    skb: Vec<(String, CgroupSkbLinkId)>,
}

pub struct Datapath {
    pub(crate) ebpf: Ebpf,
    loaded: HashSet<String>,
    tc: StdHashMap<String, (String, TcLinks)>,
    uplink: StdHashMap<String, (String, SchedClassifierLinkId)>,
    xdp: StdHashMap<String, (String, XdpLinkId)>,
    cgroups: StdHashMap<String, CgroupLinks>,
    sandboxes: StdHashMap<String, (CgroupDeviceLinkId, CgroupSkbLinkId)>,
    sockops: Option<(String, SockOpsLinkId)>,
    tlsfp: Option<(String, CgroupSkbLinkId)>,
    /// libssl path → (program, link) per attached symbol.
    ssl: StdHashMap<String, Vec<(&'static str, UProbeLinkId)>>,
    /// Opt-in tracing programs: key (`prog` or `prog@target`) → link.
    traces: StdHashMap<String, (&'static str, TraceLink)>,
    btf: Option<aya::Btf>,
    pub tracepoints: Vec<String>,
    pub notes: Vec<String>,
    tcx: bool,
}

impl Datapath {
    pub fn load() -> Result<Self> {
        if !programs_compiled() {
            bail!(
                "machina-bpf was built without eBPF programs (install nightly + bpf-linker and rebuild)"
            );
        }
        bump_memlock();
        let mut loader = EbpfLoader::new();
        if std::env::var_os("MACHINA_BPF_VERIFIER_LOG").is_some() {
            loader.verifier_log_level(VerifierLogLevel::VERBOSE | VerifierLogLevel::STATS);
        }
        let ebpf = loader
            .load(BPF_OBJECT)
            .context("load machina-bpf eBPF object")?;
        Ok(Self {
            ebpf,
            loaded: HashSet::new(),
            tc: StdHashMap::new(),
            uplink: StdHashMap::new(),
            xdp: StdHashMap::new(),
            cgroups: StdHashMap::new(),
            sandboxes: StdHashMap::new(),
            sockops: None,
            tlsfp: None,
            ssl: StdHashMap::new(),
            traces: StdHashMap::new(),
            btf: None,
            tracepoints: Vec::new(),
            notes: Vec::new(),
            tcx: kernel_features().tcx,
        })
    }

    fn classifier(&mut self, name: &str) -> Result<&mut SchedClassifier> {
        let first = !self.loaded.contains(name);
        let p: &mut SchedClassifier = self
            .ebpf
            .program_mut(name)
            .ok_or_else(|| anyhow!("program {name} missing"))?
            .try_into()?;
        if first {
            p.load()
                .with_context(|| format!("verifier rejected {name}"))?;
            self.loaded.insert(name.to_string());
        }
        Ok(p)
    }

    fn ensure_clsact(&self, iface: &str) {
        if !self.tcx {
            let _ = aya::programs::tc::qdisc_add_clsact(iface);
        }
    }

    /// Attach a TCX ingress+egress program pair to `iface`.
    pub fn attach_tc_pair(&mut self, iface: &str, ingress: &str, egress: &str) -> Result<()> {
        if self.tc.contains_key(iface) {
            return Ok(());
        }
        self.ensure_clsact(iface);
        let i = self
            .classifier(ingress)?
            .attach(iface, TcAttachType::Ingress)
            .with_context(|| format!("attach {ingress} to {iface}"))?;
        let e = match self.classifier(egress)?.attach(iface, TcAttachType::Egress) {
            Ok(e) => e,
            Err(err) => {
                let _ = self.classifier(ingress)?.detach(i);
                return Err(anyhow!(err).context(format!("attach {egress} to {iface}")));
            }
        };
        self.tc.insert(
            iface.to_string(),
            (
                format!("{ingress}|{egress}"),
                TcLinks {
                    ingress: i,
                    egress: e,
                },
            ),
        );
        Ok(())
    }

    /// Host datapath on a workload tap.
    pub fn attach_tc(&mut self, iface: &str) -> Result<()> {
        self.attach_tc_pair(iface, "mn_tc_ingress", "mn_tc_egress")
    }

    pub fn detach_tc(&mut self, iface: &str) {
        let Some((names, links)) = self.tc.remove(iface) else {
            return;
        };
        let (ing, eg) = names
            .split_once('|')
            .unwrap_or(("mn_tc_ingress", "mn_tc_egress"));
        let (ing, eg) = (ing.to_string(), eg.to_string());
        if let Ok(p) = self.classifier(&ing) {
            let _ = p.detach(links.ingress);
        }
        if let Ok(p) = self.classifier(&eg) {
            let _ = p.detach(links.egress);
        }
    }

    /// Forget links for an interface that disappeared (kernel already dropped them).
    pub fn forget_tc(&mut self, iface: &str) {
        self.tc.remove(iface);
        let prefix = format!("{iface}:");
        self.uplink.retain(|k, _| !k.starts_with(&prefix));
        self.xdp.remove(iface);
    }

    pub fn detach_tc_one(&mut self, iface: &str, prog: &str) {
        if let Some((name, id)) = self.uplink.remove(&format!("{iface}:{prog}")) {
            if let Ok(p) = self.classifier(&name) {
                let _ = p.detach(id);
            }
        }
    }

    pub fn tc_one_attached(&self, iface: &str, prog: &str) -> bool {
        self.uplink.contains_key(&format!("{iface}:{prog}"))
    }

    /// `mn_sockops` (connect latency, TCP pressure) on a cgroup; observe only.
    pub fn attach_sockops(&mut self, cg_path: &Path) -> Result<()> {
        let key = cg_path.display().to_string();
        if self.sockops.as_ref().is_some_and(|(k, _)| *k == key) {
            return Ok(());
        }
        self.detach_sockops();
        let first = !self.loaded.contains("mn_sockops");
        let p: &mut SockOps = self
            .ebpf
            .program_mut("mn_sockops")
            .ok_or_else(|| anyhow!("program mn_sockops missing"))?
            .try_into()?;
        if first {
            p.load().context("verifier rejected mn_sockops")?;
            self.loaded.insert("mn_sockops".into());
        }
        let f = File::open(cg_path).with_context(|| format!("open {}", cg_path.display()))?;
        let id = p
            .attach(f, CgroupAttachMode::default())
            .context("attach mn_sockops")?;
        self.sockops = Some((key, id));
        Ok(())
    }

    pub fn detach_sockops(&mut self) {
        let Some((_, id)) = self.sockops.take() else {
            return;
        };
        if let Some(p) = self.ebpf.program_mut("mn_sockops") {
            if let Ok(p) = <&mut SockOps>::try_from(p) {
                let _ = p.detach(id);
            }
        }
    }

    pub fn sockops_attached(&self) -> Option<&str> {
        self.sockops.as_ref().map(|(k, _)| k.as_str())
    }

    fn load_once(
        &mut self,
        name: &'static str,
        load: impl FnOnce(&mut aya::programs::Program) -> Result<()>,
    ) -> Result<()> {
        if self.loaded.contains(name) {
            return Ok(());
        }
        let p = self
            .ebpf
            .program_mut(name)
            .ok_or_else(|| anyhow!("program {name} missing"))?;
        load(p).with_context(|| format!("verifier rejected {name}"))?;
        self.loaded.insert(name.into());
        Ok(())
    }

    /// `mn_tlsfp` (ClientHello sampler) on a cgroup's egress.
    pub fn attach_tlsfp(&mut self, cg_path: &Path) -> Result<()> {
        let key = cg_path.display().to_string();
        if self.tlsfp.as_ref().is_some_and(|(k, _)| *k == key) {
            return Ok(());
        }
        self.detach_tlsfp();
        self.load_once("mn_tlsfp", |p| Ok(<&mut CgroupSkb>::try_from(p)?.load()?))?;
        let p: &mut CgroupSkb = self
            .ebpf
            .program_mut("mn_tlsfp")
            .expect("loaded")
            .try_into()?;
        let f = File::open(cg_path).with_context(|| format!("open {}", cg_path.display()))?;
        let id = p
            .attach(f, CgroupSkbAttachType::Egress, CgroupAttachMode::default())
            .context("attach mn_tlsfp")?;
        self.tlsfp = Some((key, id));
        Ok(())
    }

    pub fn detach_tlsfp(&mut self) {
        let Some((_, id)) = self.tlsfp.take() else {
            return;
        };
        if let Some(p) = self.ebpf.program_mut("mn_tlsfp") {
            if let Ok(p) = <&mut CgroupSkb>::try_from(p) {
                let _ = p.detach(id);
            }
        }
    }

    pub fn tlsfp_attached(&self) -> Option<&str> {
        self.tlsfp.as_ref().map(|(k, _)| k.as_str())
    }

    /// Uprobes on one libssl object. Symbols the library lacks (e.g. the
    /// `_ex` variants before OpenSSL 1.1.1) are skipped; at least one of
    /// SSL_write / SSL_read must attach.
    pub fn attach_ssl(&mut self, lib: &str) -> Result<()> {
        if self.ssl.contains_key(lib) {
            return Ok(());
        }
        const PLAN: [(&str, &str); 6] = [
            ("mn_ssl_write", "SSL_write"),
            ("mn_ssl_write", "SSL_write_ex"),
            ("mn_ssl_read_enter", "SSL_read"),
            ("mn_ssl_read_ret", "SSL_read"),
            ("mn_ssl_read_ex_enter", "SSL_read_ex"),
            ("mn_ssl_read_ret", "SSL_read_ex"),
        ];
        let mut links = Vec::new();
        let mut errs = Vec::new();
        for (prog, sym) in PLAN {
            let res = self
                .load_once(prog, |p| Ok(<&mut UProbe>::try_from(p)?.load()?))
                .and_then(|_| {
                    let p: &mut UProbe = self.ebpf.program_mut(prog).expect("loaded").try_into()?;
                    Ok(p.attach(sym, lib, UProbeScope::AllProcesses)?)
                });
            match res {
                Ok(id) => links.push((prog, id)),
                Err(e) => errs.push(format!("{prog}@{sym}: {e:#}")),
            }
        }
        if links.is_empty() {
            bail!("no libssl symbol attached in {lib}: {}", errs.join("; "));
        }
        self.ssl.insert(lib.to_string(), links);
        Ok(())
    }

    pub fn detach_ssl(&mut self, lib: &str) {
        let Some(links) = self.ssl.remove(lib) else {
            return;
        };
        for (prog, id) in links {
            if let Some(p) = self.ebpf.program_mut(prog) {
                if let Ok(p) = <&mut UProbe>::try_from(p) {
                    let _ = p.detach(id);
                }
            }
        }
    }

    pub fn ssl_libraries(&self) -> Vec<String> {
        let mut v: Vec<String> = self.ssl.keys().cloned().collect();
        v.sort();
        v
    }

    /// QEMU sandbox on a machine scope: device allowlist + IP egress filter.
    /// bpf_link attaches are multi-mode, so libvirt's own device program
    /// keeps running and the kernel ANDs both verdicts.
    pub fn attach_sandbox(&mut self, cg_path: &Path) -> Result<()> {
        let key = cg_path.display().to_string();
        if self.sandboxes.contains_key(&key) {
            return Ok(());
        }
        let first = !self.loaded.contains("mn_qemu_device");
        let dev: &mut CgroupDevice = self
            .ebpf
            .program_mut("mn_qemu_device")
            .ok_or_else(|| anyhow!("program mn_qemu_device missing"))?
            .try_into()?;
        if first {
            dev.load().context("verifier rejected mn_qemu_device")?;
            self.loaded.insert("mn_qemu_device".into());
        }
        let f = File::open(cg_path).with_context(|| format!("open {}", cg_path.display()))?;
        let dev_id = dev
            .attach(f, CgroupAttachMode::default())
            .context("attach mn_qemu_device")?;

        let first = !self.loaded.contains("mn_qemu_egress");
        let skb: &mut CgroupSkb = self
            .ebpf
            .program_mut("mn_qemu_egress")
            .ok_or_else(|| anyhow!("program mn_qemu_egress missing"))?
            .try_into()?;
        let res = (|| -> Result<CgroupSkbLinkId> {
            if first {
                skb.load().context("verifier rejected mn_qemu_egress")?;
            }
            let f = File::open(cg_path)?;
            Ok(skb.attach(f, CgroupSkbAttachType::Egress, CgroupAttachMode::default())?)
        })();
        if first && res.is_ok() {
            self.loaded.insert("mn_qemu_egress".into());
        }
        let skb_id = match res {
            Ok(id) => id,
            Err(e) => {
                if let Some(p) = self.ebpf.program_mut("mn_qemu_device") {
                    if let Ok(p) = <&mut CgroupDevice>::try_from(p) {
                        let _ = p.detach(dev_id);
                    }
                }
                return Err(e.context("attach mn_qemu_egress"));
            }
        };
        self.sandboxes.insert(key, (dev_id, skb_id));
        Ok(())
    }

    pub fn detach_sandbox(&mut self, cg_path: &str) {
        let Some((dev_id, skb_id)) = self.sandboxes.remove(cg_path) else {
            return;
        };
        if let Some(p) = self.ebpf.program_mut("mn_qemu_device") {
            if let Ok(p) = <&mut CgroupDevice>::try_from(p) {
                let _ = p.detach(dev_id);
            }
        }
        if let Some(p) = self.ebpf.program_mut("mn_qemu_egress") {
            if let Ok(p) = <&mut CgroupSkb>::try_from(p) {
                let _ = p.detach(skb_id);
            }
        }
    }

    /// Drop links of a scope whose cgroup is gone (the kernel released them).
    pub fn forget_sandbox(&mut self, cg_path: &str) {
        self.sandboxes.remove(cg_path);
    }

    pub fn tc_attached(&self) -> Vec<String> {
        self.tc.keys().cloned().collect()
    }

    /// Single-direction classifier (e.g. CNI NodePort on the uplink).
    pub fn attach_tc_one(&mut self, iface: &str, prog: &str, ingress: bool) -> Result<()> {
        let key = format!("{iface}:{prog}");
        if self.uplink.contains_key(&key) {
            return Ok(());
        }
        self.ensure_clsact(iface);
        let dir = if ingress {
            TcAttachType::Ingress
        } else {
            TcAttachType::Egress
        };
        let id = self
            .classifier(prog)?
            .attach(iface, dir)
            .with_context(|| format!("attach {prog} to {iface}"))?;
        self.uplink.insert(key, (prog.to_string(), id));
        Ok(())
    }

    /// Like [`attach_tc_one`](Self::attach_tc_one) but first in the TCX chain,
    /// so no earlier classifier can redirect around it.
    pub fn attach_tc_first(&mut self, iface: &str, prog: &str, ingress: bool) -> Result<()> {
        let key = format!("{iface}:{prog}");
        if self.uplink.contains_key(&key) {
            return Ok(());
        }
        self.ensure_clsact(iface);
        let dir = if ingress {
            TcAttachType::Ingress
        } else {
            TcAttachType::Egress
        };
        let id = self
            .classifier(prog)?
            .attach_with_options(
                iface,
                dir,
                aya::programs::tc::TcAttachOptions::TcxOrder(aya::programs::LinkOrder::first()),
            )
            .with_context(|| format!("attach {prog} first on {iface}"))?;
        self.uplink.insert(key, (prog.to_string(), id));
        Ok(())
    }

    pub fn attach_xdp(&mut self, iface: &str) -> Result<()> {
        self.attach_xdp_prog(iface, "mn_xdp_deny")
    }

    pub(crate) fn xdp_program(&mut self, name: &str) -> Result<&mut Xdp> {
        let first = !self.loaded.contains(name);
        let p: &mut Xdp = self
            .ebpf
            .program_mut(name)
            .ok_or_else(|| anyhow!("program {name} missing"))?
            .try_into()?;
        if first {
            p.load()
                .with_context(|| format!("verifier rejected {name}"))?;
            self.loaded.insert(name.to_string());
        }
        Ok(p)
    }

    /// Attach an XDP program (native, falling back to generic/skb mode).
    /// One XDP program per interface; a different one replaces it.
    pub fn attach_xdp_prog(&mut self, iface: &str, prog: &str) -> Result<()> {
        match self.xdp.get(iface) {
            Some((p, _)) if p == prog => return Ok(()),
            Some(_) => self.detach_xdp(iface),
            None => {}
        }
        let p = self.xdp_program(prog)?;
        // Generic mode for netns tests: native XDP_TX on a veth needs NAPI on the peer.
        let skb_only = std::env::var_os("MACHINA_BPF_XDP_SKB").is_some();
        let id = (if skb_only {
            p.attach(iface, XdpMode::Skb)
        } else {
            p.attach(iface, XdpMode::default())
        })
        .or_else(|_| p.attach(iface, XdpMode::Skb))
        .with_context(|| format!("attach {prog} to {iface}"))?;
        self.xdp.insert(iface.to_string(), (prog.to_string(), id));
        Ok(())
    }

    pub fn xdp_attached(&self, iface: &str) -> Option<&str> {
        self.xdp.get(iface).map(|(p, _)| p.as_str())
    }

    pub fn detach_xdp(&mut self, iface: &str) {
        if let Some((prog, id)) = self.xdp.remove(iface) {
            if let Some(p) = self.ebpf.program_mut(&prog) {
                if let Ok(p) = <&mut Xdp>::try_from(p) {
                    let _ = p.detach(id);
                }
            }
        }
    }

    /// Attach sock_addr + cgroup_skb programs to a cgroup v2 directory.
    pub fn attach_cgroup(&mut self, cg_path: &Path, sock_progs: &[&str], skb: bool) -> Result<()> {
        let key = cg_path.display().to_string();
        if self.cgroups.contains_key(&key) {
            return Ok(());
        }
        let mut links = CgroupLinks::default();
        for name in sock_progs {
            let first = !self.loaded.contains(*name);
            let p: &mut CgroupSockAddr = self
                .ebpf
                .program_mut(name)
                .ok_or_else(|| anyhow!("program {name} missing"))?
                .try_into()?;
            if first {
                p.load()
                    .with_context(|| format!("verifier rejected {name}"))?;
                self.loaded.insert(name.to_string());
            }
            let f = File::open(cg_path).with_context(|| format!("open {}", cg_path.display()))?;
            let id = p.attach(f, CgroupAttachMode::default())?;
            links.sock.push((name.to_string(), id));
        }
        if skb {
            for (name, ty) in [
                ("mn_cg_skb_ingress", CgroupSkbAttachType::Ingress),
                ("mn_cg_skb_egress", CgroupSkbAttachType::Egress),
            ] {
                let first = !self.loaded.contains(name);
                let p: &mut CgroupSkb = self
                    .ebpf
                    .program_mut(name)
                    .ok_or_else(|| anyhow!("program {name} missing"))?
                    .try_into()?;
                if first {
                    p.load()
                        .with_context(|| format!("verifier rejected {name}"))?;
                    self.loaded.insert(name.to_string());
                }
                let f = File::open(cg_path)?;
                let id = p.attach(f, ty, CgroupAttachMode::default())?;
                links.skb.push((name.to_string(), id));
            }
        }
        self.cgroups.insert(key, links);
        Ok(())
    }

    /// Attach sock_addr programs next to whatever else runs on `cg_path`,
    /// tracked under `<path>#<tag>`. The default mode uses a bpf_link, which
    /// the kernel always attaches multi-mode; passing `AllowMultiple` sets
    /// BPF_F_ALLOW_MULTI on link_create and fails with EINVAL.
    pub fn attach_cgroup_tagged(
        &mut self,
        cg_path: &Path,
        tag: &str,
        sock_progs: &[&str],
    ) -> Result<()> {
        let key = format!("{}#{tag}", cg_path.display());
        if self.cgroups.contains_key(&key) {
            return Ok(());
        }
        let mut links = CgroupLinks::default();
        for name in sock_progs {
            let first = !self.loaded.contains(*name);
            let p: &mut CgroupSockAddr = self
                .ebpf
                .program_mut(name)
                .ok_or_else(|| anyhow!("program {name} missing"))?
                .try_into()?;
            if first {
                p.load()
                    .with_context(|| format!("verifier rejected {name}"))?;
                self.loaded.insert(name.to_string());
            }
            let f = File::open(cg_path).with_context(|| format!("open {}", cg_path.display()))?;
            let id = p.attach(f, CgroupAttachMode::default())?;
            links.sock.push((name.to_string(), id));
        }
        self.cgroups.insert(key, links);
        Ok(())
    }

    /// cgroup_skb programs (ingress, egress) tracked under `<path>#<tag>`.
    pub fn attach_cgroup_skb_tagged(
        &mut self,
        cg_path: &Path,
        tag: &str,
        ingress: &'static str,
        egress: &'static str,
    ) -> Result<()> {
        let key = format!("{}#{tag}", cg_path.display());
        if self.cgroups.contains_key(&key) {
            return Ok(());
        }
        let mut links = CgroupLinks::default();
        for (name, ty) in [
            (ingress, CgroupSkbAttachType::Ingress),
            (egress, CgroupSkbAttachType::Egress),
        ] {
            self.load_once(name, |p| Ok(<&mut CgroupSkb>::try_from(p)?.load()?))?;
            let p: &mut CgroupSkb = self.ebpf.program_mut(name).expect("loaded").try_into()?;
            let f = File::open(cg_path).with_context(|| format!("open {}", cg_path.display()))?;
            match p.attach(f, ty, CgroupAttachMode::default()) {
                Ok(id) => links.skb.push((name.to_string(), id)),
                Err(e) => {
                    self.cgroups.insert(key.clone(), links);
                    self.detach_cgroup_key(&key);
                    return Err(
                        anyhow!(e).context(format!("attach {name} to {}", cg_path.display()))
                    );
                }
            }
        }
        self.cgroups.insert(key, links);
        Ok(())
    }

    /// Cgroup keys (path or `path#tag`) carrying a given tag.
    pub fn cgroup_tagged(&self, tag: &str) -> Option<String> {
        let suffix = format!("#{tag}");
        self.cgroups
            .keys()
            .find(|k| k.ends_with(&suffix))
            .map(|k| k.trim_end_matches(&suffix).to_string())
    }

    pub fn detach_cgroup_tag(&mut self, tag: &str) {
        let suffix = format!("#{tag}");
        let keys: Vec<String> = self
            .cgroups
            .keys()
            .filter(|k| k.ends_with(&suffix))
            .cloned()
            .collect();
        for k in keys {
            self.detach_cgroup_key(&k);
        }
    }

    pub fn detach_cgroup(&mut self, cg_path: &Path) {
        self.detach_cgroup_key(&cg_path.display().to_string());
    }

    fn detach_cgroup_key(&mut self, key: &str) {
        let Some(links) = self.cgroups.remove(key) else {
            return;
        };
        for (name, id) in links.sock {
            if let Some(p) = self.ebpf.program_mut(&name) {
                if let Ok(p) = <&mut CgroupSockAddr>::try_from(p) {
                    let _ = p.detach(id);
                }
            }
        }
        for (name, id) in links.skb {
            if let Some(p) = self.ebpf.program_mut(&name) {
                if let Ok(p) = <&mut CgroupSkb>::try_from(p) {
                    let _ = p.detach(id);
                }
            }
        }
    }

    pub fn cgroups(&self) -> Vec<String> {
        self.cgroups.keys().cloned().collect()
    }

    fn ensure_btf(&mut self) -> Result<()> {
        if self.btf.is_none() {
            self.btf =
                Some(aya::Btf::from_sys_fs().context("kernel BTF (/sys/kernel/btf/vmlinux)")?);
        }
        Ok(())
    }

    /// fentry program on a kernel function (keyed by program name).
    pub fn attach_fentry(&mut self, prog: &'static str, func: &str) -> Result<()> {
        if self.traces.contains_key(prog) {
            return Ok(());
        }
        self.ensure_btf()?;
        let btf = self.btf.as_ref().expect("btf");
        let p: &mut FEntry = self
            .ebpf
            .program_mut(prog)
            .ok_or_else(|| anyhow!("program {prog} missing"))?
            .try_into()?;
        if !self.loaded.contains(prog) {
            p.load(func, btf)
                .with_context(|| format!("verifier rejected {prog}"))?;
            self.loaded.insert(prog.into());
        }
        let id = p
            .attach()
            .with_context(|| format!("attach {prog} to {func}"))?;
        self.traces
            .insert(prog.into(), (prog, TraceLink::FEntry(id)));
        Ok(())
    }

    /// BPF-LSM program on a security hook (keyed by program name). Loads and
    /// links even when `bpf` is not an active LSM; it then never runs.
    pub fn attach_lsm(&mut self, prog: &'static str, hook: &str) -> Result<()> {
        if self.traces.contains_key(prog) {
            return Ok(());
        }
        self.ensure_btf()?;
        let btf = self.btf.as_ref().expect("btf");
        let p: &mut Lsm = self
            .ebpf
            .program_mut(prog)
            .ok_or_else(|| anyhow!("program {prog} missing"))?
            .try_into()?;
        if !self.loaded.contains(prog) {
            p.load(hook, btf)
                .with_context(|| format!("verifier rejected {prog}"))?;
            self.loaded.insert(prog.into());
        }
        let id = p
            .attach()
            .with_context(|| format!("attach {prog} to lsm/{hook}"))?;
        self.traces.insert(prog.into(), (prog, TraceLink::Lsm(id)));
        Ok(())
    }

    /// kprobe or kretprobe (keyed `prog@func`).
    pub fn attach_kprobe(&mut self, prog: &'static str, func: &str) -> Result<()> {
        let key = format!("{prog}@{func}");
        if self.traces.contains_key(&key) {
            return Ok(());
        }
        self.load_once(prog, |p| Ok(<&mut KProbe>::try_from(p)?.load()?))?;
        let p: &mut KProbe = self.ebpf.program_mut(prog).expect("loaded").try_into()?;
        let id = p
            .attach(func, 0)
            .with_context(|| format!("attach {prog} to {func}"))?;
        self.traces.insert(key, (prog, TraceLink::KProbe(id)));
        Ok(())
    }

    /// Tracepoint (keyed by program name).
    pub fn attach_tracepoint(&mut self, prog: &'static str, cat: &str, ev: &str) -> Result<()> {
        if self.traces.contains_key(prog) {
            return Ok(());
        }
        self.load_once(prog, |p| Ok(<&mut TracePoint>::try_from(p)?.load()?))?;
        let p: &mut TracePoint = self.ebpf.program_mut(prog).expect("loaded").try_into()?;
        let id = p
            .attach(cat, ev)
            .with_context(|| format!("attach {prog} to {cat}/{ev}"))?;
        self.traces.insert(prog.into(), (prog, TraceLink::Tp(id)));
        Ok(())
    }

    pub fn detach_trace(&mut self, key: &str) {
        let Some((prog, link)) = self.traces.remove(key) else {
            return;
        };
        let Some(p) = self.ebpf.program_mut(prog) else {
            return;
        };
        let _ = match link {
            TraceLink::FEntry(id) => <&mut FEntry>::try_from(p).map(|p| p.detach(id)),
            TraceLink::KProbe(id) => <&mut KProbe>::try_from(p).map(|p| p.detach(id)),
            TraceLink::Tp(id) => <&mut TracePoint>::try_from(p).map(|p| p.detach(id)),
            TraceLink::Lsm(id) => <&mut Lsm>::try_from(p).map(|p| p.detach(id)),
        };
    }

    /// Detach every trace link whose key starts with one of `progs`.
    pub fn detach_traces(&mut self, progs: &[&str]) {
        let keys: Vec<String> = self
            .traces
            .keys()
            .filter(|k| {
                progs
                    .iter()
                    .any(|p| k.as_str() == *p || k.starts_with(&format!("{p}@")))
            })
            .cloned()
            .collect();
        for k in keys {
            self.detach_trace(&k);
        }
    }

    pub fn trace_attached(&self, key: &str) -> bool {
        self.traces.contains_key(key)
    }

    pub fn traces(&self) -> Vec<String> {
        let mut v: Vec<String> = self.traces.keys().cloned().collect();
        v.sort();
        v
    }

    /// Attach telemetry tracepoints and the capability kprobe. Failures are
    /// recorded in `notes` (a missing tracepoint disables only that feature).
    pub fn attach_telemetry(&mut self) {
        for (prog, cat, ev) in crate::tracefs::TRACEPOINTS {
            let res: Result<()> = (|| {
                let p: &mut TracePoint = self
                    .ebpf
                    .program_mut(prog)
                    .ok_or_else(|| anyhow!("program {prog} missing"))?
                    .try_into()?;
                p.load()?;
                p.attach(cat, ev)?;
                Ok(())
            })();
            match res {
                Ok(()) => self.tracepoints.push(format!("{cat}/{ev}")),
                Err(e) => self
                    .notes
                    .push(format!("tracepoint {cat}/{ev} unavailable: {e:#}")),
            }
        }
        let res: Result<()> = (|| {
            let p: &mut KProbe = self
                .ebpf
                .program_mut("mn_kp_cap_capable")
                .ok_or_else(|| anyhow!("program mn_kp_cap_capable missing"))?
                .try_into()?;
            p.load()?;
            p.attach("cap_capable", 0)?;
            Ok(())
        })();
        match res {
            Ok(()) => self.tracepoints.push("kprobe/cap_capable".into()),
            Err(e) => self.notes.push(format!(
                "kprobe cap_capable unavailable (deny_cap disabled): {e:#}"
            )),
        }
    }
}

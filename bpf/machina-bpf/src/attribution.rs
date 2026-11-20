// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Workload attribution without libvirt bindings: tap → VM from libvirt's
//! live status XML, and cgroup path → VM / systemd unit / container.

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VmIface {
    pub vm: String,
    pub uuid: Option<String>,
    pub mac: Option<String>,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    for q in ['\'', '"'] {
        let pat = format!("{name}={q}");
        if let Some(i) = tag.find(&pat) {
            let rest = &tag[i + pat.len()..];
            if let Some(j) = rest.find(q) {
                return Some(rest[..j].to_string());
            }
        }
    }
    None
}

fn element_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let i = xml.find(&open)? + open.len();
    let j = xml[i..].find(&format!("</{tag}>"))?;
    Some(xml[i..i + j].trim().to_string())
}

/// Parse one libvirt domain (or domstatus) XML document into tap → VM entries.
pub fn parse_domain_xml(xml: &str) -> HashMap<String, VmIface> {
    let mut out = HashMap::new();
    let dom = match xml.find("<domain") {
        Some(i) => &xml[i..],
        None => return out,
    };
    let Some(name) = element_text(dom, "name") else {
        return out;
    };
    let uuid = element_text(dom, "uuid");
    let mut rest = dom;
    while let Some(i) = rest.find("<interface") {
        let block_start = &rest[i..];
        let end = block_start
            .find("</interface>")
            .unwrap_or(block_start.len());
        let block = &block_start[..end];
        let target = block.find("<target").and_then(|t| {
            attr(
                &block[t..block[t..].find('>').map(|e| t + e).unwrap_or(block.len())],
                "dev",
            )
        });
        let mac = block.find("<mac").and_then(|t| {
            attr(
                &block[t..block[t..].find('>').map(|e| t + e).unwrap_or(block.len())],
                "address",
            )
        });
        if let Some(dev) = target {
            out.insert(
                dev,
                VmIface {
                    vm: name.clone(),
                    uuid: uuid.clone(),
                    mac,
                },
            );
        }
        rest = &block_start[end.min(block_start.len())..];
        if end == block_start.len() {
            break;
        }
    }
    out
}

/// Scan libvirt's runtime directories for running domains.
pub fn scan_libvirt() -> HashMap<String, VmIface> {
    let mut out = HashMap::new();
    for dir in ["/run/libvirt/qemu", "/var/run/libvirt/qemu"] {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("xml") {
                continue;
            }
            if let Ok(xml) = std::fs::read_to_string(&p) {
                out.extend(parse_domain_xml(&xml));
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// systemd escapes '-' as `\x2d` in unit names.
fn unescape_systemd(s: &str) -> String {
    s.replace("\\x2d", "-").replace("\\x5c", "\\")
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CgroupInfo {
    pub vm: Option<String>,
    pub unit: Option<String>,
    pub container: Option<String>,
}

/// Classify a cgroup v2 path (relative, e.g. `machine.slice/machine-qemu\x2d3\x2dweb.scope/libvirt/emulator`).
pub fn classify_cgroup(path: &str) -> CgroupInfo {
    let mut info = CgroupInfo::default();
    for seg in path.split('/') {
        if let Some(rest) = seg.strip_prefix("machine-qemu\\x2d") {
            let rest = rest.trim_end_matches(".scope");
            // "<id>\x2d<name>"
            let name = rest.split_once("\\x2d").map(|(_, n)| n).unwrap_or(rest);
            info.vm = Some(unescape_systemd(name));
        } else if let Some(id) = seg
            .strip_prefix("libpod-")
            .or_else(|| seg.strip_prefix("docker-"))
            .or_else(|| seg.strip_prefix("cri-containerd-"))
            .or_else(|| seg.strip_prefix("crio-"))
        {
            let id = id.trim_end_matches(".scope");
            if !id.starts_with("conmon") {
                info.container = Some(id.chars().take(12).collect());
            }
        } else if seg.ends_with(".service") && info.unit.is_none() {
            info.unit = Some(unescape_systemd(seg));
        }
    }
    info
}

/// Pod UID from a kubepods cgroup path (systemd `kubepods-…-pod<uid>.slice`
/// or cgroupfs `pod<uid>`), with systemd's `_` turned back into `-`.
pub fn pod_uid(path: &str) -> Option<String> {
    if !path.contains("kubepods") {
        return None;
    }
    path.split('/').find_map(|seg| {
        let s = seg.trim_end_matches(".slice");
        let uid = s
            .rsplit_once("-pod")
            .map(|(_, u)| u)
            .or_else(|| s.strip_prefix("pod"))?;
        (uid.len() >= 32
            && uid
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b'_' || b == b'-'))
        .then(|| uid.replace('_', "-"))
    })
}

/// Container id of a runtime scope (`cri-containerd-<id>.scope`, `crio-…`,
/// `docker-…`) or a bare 64-hex cgroupfs directory.
fn container_seg(seg: &str) -> Option<&str> {
    let id = seg
        .strip_prefix("cri-containerd-")
        .or_else(|| seg.strip_prefix("crio-"))
        .or_else(|| seg.strip_prefix("docker-"))
        .map(|s| s.trim_end_matches(".scope"))
        .unwrap_or(seg);
    (id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// Pod UID → (namespace, name): find each pod's sandbox container
/// (`sandboxes`: CNI container id → pod) under the kubepods trees of `root`.
pub fn pod_index(
    root: &Path,
    sandboxes: &HashMap<String, (String, String)>,
) -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    if sandboxes.is_empty() {
        return out;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    let mut stack: Vec<(std::path::PathBuf, usize)> = rd
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("kubepods"))
        .map(|e| (e.path(), 0))
        .collect();
    while let Some((dir, depth)) = stack.pop() {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(id) = container_seg(&name) {
            if let (Some(pod), Some(uid)) = (sandboxes.get(id), pod_uid(&dir.to_string_lossy())) {
                out.insert(uid, pod.clone());
            }
            continue;
        }
        if depth >= 4 {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push((e.path(), depth + 1));
                }
            }
        }
    }
    out
}

pub const KUBELET_POD_LOGS: &str = "/var/log/pods";

/// Pod UID → (namespace, name) from kubelet's `<ns>_<name>_<uid>` log
/// directories; covers hostNetwork pods and pods without a CNI endpoint.
pub fn pod_log_index(dir: &Path) -> HashMap<String, (String, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let mut it = n.splitn(3, '_');
            let (ns, name, uid) = (it.next()?, it.next()?, it.next()?);
            (uid.len() >= 32 && !ns.is_empty() && !name.is_empty())
                .then(|| (uid.to_string(), (ns.to_string(), name.to_string())))
        })
        .collect()
}

/// Workload for a cgroup path; `pods` is [`pod_index`]. Pods whose UID is not
/// indexed (no machina-cni endpoint) are named by UID.
pub fn cgroup_workload(
    path: &str,
    pods: &HashMap<String, (String, String)>,
) -> Option<crate::api::Workload> {
    use crate::api::Workload;
    if let Some(uid) = pod_uid(path) {
        return Some(match pods.get(&uid) {
            Some((ns, name)) => Workload {
                kind: "pod".into(),
                ns: Some(ns.clone()),
                name: name.clone(),
            },
            None => Workload {
                kind: "pod".into(),
                ns: None,
                name: uid,
            },
        });
    }
    let c = classify_cgroup(path);
    let (kind, name) = if let Some(v) = c.vm {
        ("vm", v)
    } else if let Some(v) = c.container {
        ("container", v)
    } else {
        ("service", c.unit?)
    };
    Some(Workload {
        kind: kind.into(),
        ns: None,
        name,
    })
}

/// cgroup v2 path of a live process (relative to the cgroup root).
pub fn pid_cgroup(pid: u32) -> Option<String> {
    let cg = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    cg.lines()
        .find_map(|l| l.strip_prefix("0::"))
        .map(|p| p.trim_start_matches('/').to_string())
}

/// `ns/name` → (ns, name).
pub fn split_pod(pod: &str) -> Option<(String, String)> {
    pod.split_once('/')
        .map(|(n, p)| (n.to_string(), p.to_string()))
}

/// cgroup v2 id (== inode of the cgroup directory) → relative path cache.
#[derive(Default)]
pub struct CgroupCache {
    map: HashMap<u64, String>,
    last_scan: Option<std::time::Instant>,
}

pub const CGROUP_ROOT: &str = "/sys/fs/cgroup";

#[cfg(unix)]
pub fn cgroup_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.ino())
}

#[cfg(not(unix))]
pub fn cgroup_id(_path: &Path) -> Option<u64> {
    None
}

impl CgroupCache {
    pub fn lookup(&mut self, id: u64) -> Option<String> {
        if let Some(p) = self.map.get(&id) {
            return Some(p.clone());
        }
        let stale = self
            .last_scan
            .map(|t| t.elapsed() > std::time::Duration::from_secs(5))
            .unwrap_or(true);
        if stale {
            self.rescan();
        }
        self.map.get(&id).cloned()
    }

    fn rescan(&mut self) {
        self.last_scan = Some(std::time::Instant::now());
        self.map.clear();
        let root = Path::new(CGROUP_ROOT);
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if let Some(id) = cgroup_id(&dir) {
                let rel = dir
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.map.insert(id, rel);
            }
            if depth >= 10 || self.map.len() > 50_000 {
                continue;
            }
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        stack.push((e.path(), depth + 1));
                    }
                }
            }
        }
    }
}

/// `/proc/<pid>/stat` ppid and `/proc/<pid>/cmdline`.
pub fn proc_details(pid: u32) -> (Option<u32>, Option<String>) {
    let ppid = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            let after = &s[s.rfind(')')? + 1..];
            after.split_whitespace().nth(1)?.parse().ok()
        });
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))
        .ok()
        .filter(|b| !b.is_empty())
        .map(|b| {
            b.split(|c| *c == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        });
    (ppid, cmdline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_xml() {
        let xml = r#"<domstatus state='running'><monitor path='/x'/><domain type='kvm' id='3'>
          <name>web-01</name><uuid>abcd</uuid>
          <devices>
            <interface type='network'><mac address='52:54:00:aa:bb:cc'/><source network='default'/><target dev='vnet3'/></interface>
            <interface type="bridge"><mac address="52:54:00:11:22:33"/><target dev="vnet4"/></interface>
          </devices></domain></domstatus>"#;
        let m = parse_domain_xml(xml);
        assert_eq!(m.len(), 2);
        assert_eq!(m["vnet3"].vm, "web-01");
        assert_eq!(m["vnet3"].mac.as_deref(), Some("52:54:00:aa:bb:cc"));
        assert_eq!(m["vnet4"].uuid.as_deref(), Some("abcd"));
    }

    #[test]
    fn cgroups() {
        let c = classify_cgroup(
            "machine.slice/machine-qemu\\x2d3\\x2dweb\\x2d01.scope/libvirt/emulator",
        );
        assert_eq!(c.vm.as_deref(), Some("web-01"));
        let c = classify_cgroup("system.slice/machina\\x2ddaemon.service");
        assert_eq!(c.unit.as_deref(), Some("machina-daemon.service"));
        let c = classify_cgroup("machine.slice/libpod-0123456789abcdef.scope/container");
        assert_eq!(c.container.as_deref(), Some("0123456789ab"));
    }

    const UID: &str = "0f4a2b1c_1111_2222_3333_444455556666";
    const SANDBOX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn pod_uids_and_workloads() {
        let p = format!("kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod{UID}.slice/cri-containerd-{SANDBOX}.scope");
        assert_eq!(
            pod_uid(&p).as_deref(),
            Some("0f4a2b1c-1111-2222-3333-444455556666")
        );
        assert_eq!(
            pod_uid("kubepods/burstable/pod0f4a2b1c-1111-2222-3333-444455556666/abc").as_deref(),
            Some("0f4a2b1c-1111-2222-3333-444455556666")
        );
        assert_eq!(pod_uid("system.slice/podman.service"), None);

        let mut pods = HashMap::new();
        assert_eq!(
            cgroup_workload(&p, &pods).unwrap().name,
            "0f4a2b1c-1111-2222-3333-444455556666"
        );
        pods.insert(
            "0f4a2b1c-1111-2222-3333-444455556666".to_string(),
            ("prod".to_string(), "web-0".to_string()),
        );
        let w = cgroup_workload(&p, &pods).unwrap();
        assert_eq!(
            (w.kind.as_str(), w.ns.as_deref(), w.name.as_str()),
            ("pod", Some("prod"), "web-0")
        );
        let w = cgroup_workload(
            "machine.slice/machine-qemu\\x2d3\\x2dweb.scope/libvirt/emulator",
            &pods,
        )
        .unwrap();
        assert_eq!((w.kind.as_str(), w.name.as_str()), ("vm", "web"));
        let w = cgroup_workload("system.slice/sshd.service", &pods).unwrap();
        assert_eq!(
            (w.kind.as_str(), w.name.as_str()),
            ("service", "sshd.service")
        );
        assert!(cgroup_workload("user.slice", &pods).is_none());
        assert_eq!(split_pod("ns/n"), Some(("ns".into(), "n".into())));
    }

    #[test]
    fn pod_index_joins_sandbox_scopes() {
        let root = std::env::temp_dir().join(format!("mnpodidx-{}", std::process::id()));
        let pod_dir = root.join(format!(
            "kubepods.slice/kubepods-burstable.slice/kubepods-burstable-pod{UID}.slice"
        ));
        std::fs::create_dir_all(pod_dir.join(format!("cri-containerd-{SANDBOX}.scope"))).unwrap();
        std::fs::create_dir_all(pod_dir.join(format!("cri-containerd-{}.scope", "b".repeat(64))))
            .unwrap();
        let sandboxes = HashMap::from([(
            SANDBOX.to_string(),
            ("prod".to_string(), "web-0".to_string()),
        )]);
        let idx = pod_index(&root, &sandboxes);
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(
            idx.get("0f4a2b1c-1111-2222-3333-444455556666"),
            Some(&("prod".to_string(), "web-0".to_string()))
        );
        assert_eq!(idx.len(), 1);

        let logs = std::env::temp_dir().join(format!("mnpodlogs-{}", std::process::id()));
        std::fs::create_dir_all(
            logs.join("kube-system_coredns-5d8_0f4a2b1c-1111-2222-3333-444455556666"),
        )
        .unwrap();
        std::fs::create_dir_all(logs.join("junk")).unwrap();
        let idx = pod_log_index(&logs);
        std::fs::remove_dir_all(&logs).ok();
        assert_eq!(
            idx.get("0f4a2b1c-1111-2222-3333-444455556666"),
            Some(&("kube-system".to_string(), "coredns-5d8".to_string()))
        );
        assert_eq!(idx.len(), 1);
    }
}

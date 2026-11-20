// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Opt-in TLS visibility: `mn_tlsfp` ClientHello sampling (JA3/JA4) and
//! libssl uprobes (HTTP metadata from plaintext heads).

use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;

use super::*;

const MAX_RATE: u32 = 10_000;
const MAX_COMMS: usize = 64;
const SCAN_EVERY: Duration = Duration::from_secs(30);
const DEFAULT_LIMIT: usize = 200;

/// Well-known libssl locations, attached even before any process maps them.
const SYSTEM_LIBSSL: &[&str] = &[
    "/usr/lib/x86_64-linux-gnu/libssl.so.3",
    "/usr/lib/aarch64-linux-gnu/libssl.so.3",
    "/usr/lib64/libssl.so.3",
    "/usr/lib/libssl.so.3",
    "/usr/lib/x86_64-linux-gnu/libssl.so.1.1",
    "/usr/lib64/libssl.so.1.1",
];

#[derive(Default)]
pub(super) struct TlsRuntime {
    pub config: TlsConfig,
    comms: HashSet<[u8; 16]>,
    /// (dev, inode) of libssl objects already probed.
    inodes: HashSet<(u64, u64)>,
    last_scan: Option<Instant>,
    notes: Vec<String>,
}

fn comm_key(s: &str) -> Result<[u8; 16]> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 15 {
        return Err(anyhow!(
            "ssl_comms entry `{s}` must be 1..=15 bytes (the kernel comm length)"
        ));
    }
    let mut k = [0u8; 16];
    k[..b.len()].copy_from_slice(b);
    Ok(k)
}

fn ino(path: &str) -> Option<(u64, u64)> {
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// libssl objects mapped by running processes (via their mount namespace
/// root) plus the system copies. One path per inode.
fn libssl_targets() -> Vec<(String, (u64, u64))> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut add = |target: String| {
        if let Some(i) = ino(&target) {
            if seen.insert(i) {
                out.push((target, i));
            }
        }
    };
    for p in SYSTEM_LIBSSL {
        add(p.to_string());
    }
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(pid) = name
            .to_str()
            .filter(|s| s.bytes().all(|c| c.is_ascii_digit()))
        else {
            continue;
        };
        let Ok(maps) = std::fs::read_to_string(format!("/proc/{pid}/maps")) else {
            continue;
        };
        let mut libs: HashSet<&str> = HashSet::new();
        for line in maps.lines() {
            if let Some(path) = line.split_whitespace().nth(5) {
                if path.contains("/libssl.so") {
                    libs.insert(path);
                }
            }
        }
        for path in libs {
            // Same file on the host: use the plain path; else go through the
            // process's root (containers).
            let via_root = format!("/proc/{pid}/root{path}");
            match (ino(path), ino(&via_root)) {
                (Some(a), Some(b)) if a == b => add(path.to_string()),
                (_, Some(_)) => add(via_root),
                _ => {}
            }
        }
    }
    out
}

impl Engine {
    pub(super) fn tls_configure(&mut self, config: TlsConfig) -> Result<TlsStatus> {
        for (name, v) in [
            ("fingerprint_rate", config.fingerprint_rate),
            ("ssl_rate", config.ssl_rate),
        ] {
            if !(1..=MAX_RATE).contains(&v) {
                return Err(anyhow!("{name} must be 1..={MAX_RATE} per second"));
            }
        }
        if config.ssl_comms.len() > MAX_COMMS {
            return Err(anyhow!("at most {MAX_COMMS} ssl_comms"));
        }
        let comms: HashSet<[u8; 16]> = config
            .ssl_comms
            .iter()
            .map(|c| comm_key(c))
            .collect::<Result<_>>()?;
        self.dp.array_set(
            "TLSFP_CFG",
            0,
            SampleCfg {
                enabled: config.fingerprints as u32,
                rate: config.fingerprint_rate,
                all: 0,
                _pad: 0,
            },
        )?;
        for k in self.tls.comms.clone() {
            if !comms.contains(&k) {
                self.dp.cni_hash_remove::<[u8; 16], u8>("SSL_COMMS", &k);
            }
        }
        for k in &comms {
            self.dp.cni_hash_insert("SSL_COMMS", *k, 1u8)?;
        }
        self.dp.array_set(
            "SSL_CFG",
            0,
            SampleCfg {
                enabled: config.ssl_uprobes as u32,
                rate: config.ssl_rate,
                all: config.ssl_all_processes as u32,
                _pad: 0,
            },
        )?;
        lock(&self.shared).fp_from_l7 = config.fingerprints;
        self.tls.comms = comms;
        self.tls.config = config;
        self.tls.last_scan = None;
        self.tls_refresh();
        Ok(self.tls_status())
    }

    fn tls_note(&mut self, n: String) {
        if !self.tls.notes.contains(&n) {
            tracing::warn!("{n}");
            self.tls.notes.push(n);
            if self.tls.notes.len() > 20 {
                self.tls.notes.remove(0);
            }
        }
    }

    /// Follow the config: attach/detach `mn_tlsfp`, and (every 30 s while
    /// enabled) probe newly seen libssl objects.
    pub(super) fn tls_refresh(&mut self) {
        if self.tls.config.fingerprints && self.features.cgroup2 {
            let cg = std::env::var_os("MACHINA_BPF_TLSFP_CGROUP")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(attribution::CGROUP_ROOT));
            if let Err(e) = self.dp.attach_tlsfp(&cg) {
                self.tls_note(format!("tls fingerprints unavailable: {e:#}"));
            }
        } else {
            self.dp.detach_tlsfp();
        }
        if !self.tls.config.ssl_uprobes {
            for lib in self.dp.ssl_libraries() {
                self.dp.detach_ssl(&lib);
            }
            self.tls.inodes.clear();
            return;
        }
        if self.tls.last_scan.is_some_and(|t| t.elapsed() < SCAN_EVERY) {
            return;
        }
        self.tls.last_scan = Some(Instant::now());
        for (target, i) in libssl_targets() {
            if self.tls.inodes.contains(&i) {
                continue;
            }
            match self.dp.attach_ssl(&target) {
                Ok(()) => {
                    self.tls.inodes.insert(i);
                }
                Err(e) => self.tls_note(format!("libssl {target}: {e:#}")),
            }
        }
    }

    pub(super) fn tls_status(&mut self) -> TlsStatus {
        let c = lock(&self.shared).counters.clone();
        let mut notes = self
            .dp
            .notes
            .iter()
            .filter(|n| n.contains("tls") || n.contains("ssl"))
            .cloned()
            .collect::<Vec<_>>();
        notes.extend(self.tls.notes.iter().cloned());
        TlsStatus {
            config: self.tls.config.clone(),
            fingerprint_cgroup: self.dp.tlsfp_attached().map(String::from),
            ssl_libraries: self.dp.ssl_libraries(),
            fingerprints_seen: c.tls_fingerprints,
            ssl_events_seen: c.ssl_events,
            notes,
        }
    }
}

pub(super) fn recent<T: Clone>(q: &VecDeque<T>, limit: Option<usize>) -> Vec<T> {
    q.iter()
        .rev()
        .take(limit.unwrap_or(DEFAULT_LIMIT))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comm_keys_and_defaults() {
        assert_eq!(&comm_key("curl").unwrap()[..5], b"curl\0");
        assert!(comm_key("").is_err());
        assert!(comm_key("a-very-long-process-name").is_err());
        let c = TlsConfig::default();
        assert!(!c.fingerprints && !c.ssl_uprobes);
        assert_eq!((c.fingerprint_rate, c.ssl_rate), (50, 200));
    }
}

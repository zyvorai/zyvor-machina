// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Host-local IPAM (IPv4 and IPv6): one file per allocated address, created
//! with `O_EXCL` so concurrent CNI invocations never hand out the same IP.

use std::fs::OpenOptions;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

/// IPv6 subnets are huge; only the first slots are handed out.
const MAX_V6_HOSTS: u128 = 1 << 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subnet {
    pub net: u128,
    pub bits: u32,
    pub v6: bool,
}

impl Subnet {
    pub fn parse(s: &str) -> Result<Self> {
        let (a, l) = s
            .split_once('/')
            .ok_or_else(|| anyhow!("subnet `{s}` needs a prefix length"))?;
        let ip: IpAddr = a.parse().map_err(|_| anyhow!("invalid subnet `{s}`"))?;
        let bits: u32 = l.parse().map_err(|_| anyhow!("invalid prefix in `{s}`"))?;
        let (v6, width, ok) = match ip {
            IpAddr::V4(_) => (false, 32, (8..=30).contains(&bits)),
            IpAddr::V6(_) => (true, 128, (48..=120).contains(&bits)),
        };
        if !ok {
            return Err(anyhow!(
                "unsupported prefix in `{s}` (IPv4 8..30, IPv6 48..120)"
            ));
        }
        let raw = match ip {
            IpAddr::V4(a) => u32::from(a) as u128,
            IpAddr::V6(a) => u128::from(a),
        };
        let mask = (u128::MAX >> (128 - width)) & !(u128::MAX >> (128 - width + bits));
        Ok(Self {
            net: raw & mask,
            bits,
            v6,
        })
    }

    fn width(&self) -> u32 {
        if self.v6 {
            128
        } else {
            32
        }
    }

    fn addr(&self, raw: u128) -> IpAddr {
        if self.v6 {
            IpAddr::V6(Ipv6Addr::from(raw))
        } else {
            IpAddr::V4(Ipv4Addr::from(raw as u32))
        }
    }

    /// Number of usable slots: IPv4 skips network, the `.1` gateway slot and
    /// broadcast; IPv6 skips `::0`/`::1` and is capped at [`MAX_V6_HOSTS`].
    pub fn host_count(&self) -> u128 {
        let size = 1u128 << (self.width() - self.bits);
        if self.v6 {
            (size - 2).min(MAX_V6_HOSTS)
        } else {
            size - 3
        }
    }

    pub fn host(&self, i: u128) -> IpAddr {
        self.addr(self.net + 2 + i)
    }

    #[cfg(test)]
    pub fn hosts(&self) -> impl Iterator<Item = IpAddr> + '_ {
        (0..self.host_count()).map(|i| self.host(i))
    }

    fn index_of(&self, ip: IpAddr) -> Option<u128> {
        let raw = match ip {
            IpAddr::V4(a) if !self.v6 => u32::from(a) as u128,
            IpAddr::V6(a) if self.v6 => u128::from(a),
            _ => return None,
        };
        raw.checked_sub(self.net + 2)
            .filter(|i| *i < self.host_count())
    }

    #[cfg(test)]
    pub fn contains(&self, ip: IpAddr) -> bool {
        let raw = match ip {
            IpAddr::V4(a) if !self.v6 => u32::from(a) as u128,
            IpAddr::V6(a) if self.v6 => u128::from(a),
            _ => return false,
        };
        raw >> (self.width() - self.bits) == self.net >> (self.width() - self.bits)
    }

    pub fn dir_name(&self) -> String {
        format!(
            "{}-{}",
            self.addr(self.net).to_string().replace(':', "_"),
            self.bits
        )
    }
}

pub struct Ipam {
    dir: PathBuf,
    subnet: Subnet,
}

fn owner(container_id: &str, ifname: &str) -> String {
    format!("{container_id}\n{ifname}\n")
}

/// Allocation files are named by address; `:` is not portable in names.
fn file_name(ip: IpAddr) -> String {
    ip.to_string().replace(':', "_")
}

impl Ipam {
    pub fn new(root: &Path, subnet: Subnet) -> Result<Self> {
        let dir = root.join(subnet.dir_name());
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        Ok(Self { dir, subnet })
    }

    /// Existing allocation for this container/interface, if any (CNI ADD is retried).
    pub fn find(&self, container_id: &str, ifname: &str) -> Option<IpAddr> {
        let want = owner(container_id, ifname);
        std::fs::read_dir(&self.dir).ok()?.flatten().find_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let ip: IpAddr = name.replace('_', ":").parse().ok()?;
            (std::fs::read_to_string(e.path()).ok()? == want).then_some(ip)
        })
    }

    pub fn allocate(&self, container_id: &str, ifname: &str) -> Result<IpAddr> {
        if let Some(ip) = self.find(container_id, ifname) {
            return Ok(ip);
        }
        // Start after the most recent allocation so freed IPs are not reused at once.
        let last_path = self.dir.join("last");
        let last: Option<IpAddr> = std::fs::read_to_string(&last_path)
            .ok()
            .and_then(|s| s.trim().parse().ok());
        let n = self.subnet.host_count();
        let start = last
            .and_then(|l| self.subnet.index_of(l))
            .map(|p| p + 1)
            .unwrap_or(0);
        for i in 0..n {
            let ip = self.subnet.host((start + i) % n);
            let path = self.dir.join(file_name(ip));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    f.write_all(owner(container_id, ifname).as_bytes())?;
                    let _ = std::fs::write(&last_path, ip.to_string());
                    return Ok(ip);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).with_context(|| format!("reserve {}", path.display())),
            }
        }
        Err(anyhow!(
            "no free addresses in {}/{}",
            self.subnet.addr(self.subnet.net),
            self.subnet.bits
        ))
    }

    /// Release this container's address. Missing allocations are not an error.
    pub fn release(&self, container_id: &str, ifname: &str) -> Option<IpAddr> {
        let ip = self.find(container_id, ifname)?;
        let _ = std::fs::remove_file(self.dir.join(file_name(ip)));
        Some(ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn subnet_hosts() {
        let s = Subnet::parse("10.42.1.0/30").unwrap();
        assert_eq!(s.hosts().collect::<Vec<_>>(), vec![v4(10, 42, 1, 2)]);
        let s = Subnet::parse("10.42.1.77/24").unwrap();
        assert_eq!(s.net, u32::from(Ipv4Addr::new(10, 42, 1, 0)) as u128);
        assert_eq!(s.host_count(), 253);
        assert!(s.contains(v4(10, 42, 1, 200)));
        assert!(!s.contains(v4(10, 42, 2, 1)));
        assert!(Subnet::parse("10.0.0.0").is_err());
        assert!(Subnet::parse("10.0.0.0/31").is_err());

        let s6 = Subnet::parse("fd42:0:0:1::5/64").unwrap();
        assert!(s6.v6);
        assert_eq!(s6.host(0), "fd42:0:0:1::2".parse::<IpAddr>().unwrap());
        assert_eq!(s6.host_count(), MAX_V6_HOSTS);
        assert!(s6.contains("fd42:0:0:1::ffff".parse().unwrap()));
        assert!(!s6.contains("fd42:0:0:2::2".parse().unwrap()));
        assert!(!s6.contains(v4(10, 0, 0, 1)));
        assert!(Subnet::parse("fd00::/32").is_err());
        assert_eq!(s6.dir_name(), "fd42_0_0_1__-64");
    }

    #[test]
    fn allocate_release_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let ipam = Ipam::new(tmp.path(), Subnet::parse("10.42.0.0/29").unwrap()).unwrap();
        let a = ipam.allocate("c1", "eth0").unwrap();
        assert_eq!(a, v4(10, 42, 0, 2));
        assert_eq!(
            ipam.allocate("c1", "eth0").unwrap(),
            a,
            "retry returns the same IP"
        );
        let b = ipam.allocate("c2", "eth0").unwrap();
        assert_ne!(a, b);
        assert_eq!(ipam.release("c1", "eth0"), Some(a));
        assert_eq!(ipam.release("c1", "eth0"), None);
        // .2–.6 usable in a /29; c2 holds one, so four more fit, then exhaustion.
        for i in 0..4 {
            ipam.allocate(&format!("x{i}"), "eth0").unwrap();
        }
        assert!(ipam.allocate("full", "eth0").is_err());
    }

    #[test]
    fn allocate_v6() {
        let tmp = tempfile::tempdir().unwrap();
        let ipam = Ipam::new(tmp.path(), Subnet::parse("fd42::/64").unwrap()).unwrap();
        let a = ipam.allocate("c1", "eth0").unwrap();
        assert_eq!(a, "fd42::2".parse::<IpAddr>().unwrap());
        assert_eq!(ipam.find("c1", "eth0"), Some(a));
        assert_eq!(
            ipam.allocate("c2", "eth0").unwrap(),
            "fd42::3".parse::<IpAddr>().unwrap()
        );
        assert_eq!(ipam.release("c1", "eth0"), Some(a));
    }
}

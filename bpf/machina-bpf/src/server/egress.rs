// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Per-project egress IPs: the `ip`/`ip6 machina_egress` nftables tables. They are
//! address translation, not a security control, so it stays installed while
//! bpfd is stopped and is re-applied from the persisted config on start.
//! Egress IPs missing on the host are added to the uplink as /32 or /128 and
//! announced; only addresses bpfd added are ever removed.

use std::io::Write as _;
use std::net::IpAddr;
use std::process::{Command, Stdio};

use crate::netpol::snat;

use super::*;

#[derive(Default)]
pub(super) struct EgressRuntime {
    pub config: VmEgressSnat,
    skipped: Vec<String>,
    error: Option<String>,
    active: bool,
    /// Addresses bpfd added: (address, interface).
    pub managed: Vec<(IpAddr, String)>,
}

fn nft(script: &str) -> Result<()> {
    let mut child = Command::new("nft")
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("run nft")?;
    child
        .stdin
        .take()
        .context("nft stdin")?
        .write_all(script.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(anyhow!(
            "nft: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

fn local_addresses() -> Vec<IpAddr> {
    Command::new("ip")
        .args(["-o", "addr", "show"])
        .output()
        .map(|o| snat::local_addrs(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

fn uplink(v6: bool) -> Option<String> {
    let fam = if v6 { "-6" } else { "-4" };
    let out = Command::new("ip")
        .args([fam, "-o", "route", "show", "default"])
        .output()
        .ok()?;
    snat::default_dev(&String::from_utf8_lossy(&out.stdout))
}

fn ip_addr(op: &str, ip: IpAddr, dev: &str) -> Result<()> {
    let cidr = snat::host_cidr(ip);
    let mut args = vec![
        if ip.is_ipv6() { "-6" } else { "-4" },
        "addr",
        op,
        &cidr,
        "dev",
        dev,
    ];
    if ip.is_ipv6() && op == "add" {
        args.push("nodad");
    }
    let out = Command::new("ip").args(&args).output().context("run ip")?;
    if !out.status.success() {
        return Err(anyhow!(
            "ip addr {op} {cidr} dev {dev}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

fn iface_mac(dev: &str) -> Option<[u8; 6]> {
    let s = std::fs::read_to_string(format!("/sys/class/net/{dev}/address")).ok()?;
    let v: Vec<u8> = s
        .trim()
        .split(':')
        .filter_map(|h| u8::from_str_radix(h, 16).ok())
        .collect();
    v.try_into().ok()
}

#[cfg(target_os = "linux")]
fn announce(ip: IpAddr, dev: &str) -> Result<()> {
    let mac = iface_mac(dev).ok_or_else(|| anyhow!("{dev}: no MAC address"))?;
    let ifindex: i32 = std::fs::read_to_string(format!("/sys/class/net/{dev}/ifindex"))?
        .trim()
        .parse()?;
    match ip {
        IpAddr::V4(v4) => {
            let pkt = snat::garp(mac, v4);
            // SAFETY: plain socket calls on a local fd, closed below.
            unsafe {
                let fd = libc::socket(
                    libc::AF_PACKET,
                    libc::SOCK_DGRAM,
                    (libc::ETH_P_ARP as u16).to_be() as i32,
                );
                if fd < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                let mut sa: libc::sockaddr_ll = std::mem::zeroed();
                sa.sll_family = libc::AF_PACKET as u16;
                sa.sll_protocol = (libc::ETH_P_ARP as u16).to_be();
                sa.sll_ifindex = ifindex;
                sa.sll_halen = 6;
                sa.sll_addr[..6].copy_from_slice(&[0xff; 6]);
                let n = libc::sendto(
                    fd,
                    pkt.as_ptr().cast(),
                    pkt.len(),
                    0,
                    (&sa as *const libc::sockaddr_ll).cast(),
                    std::mem::size_of::<libc::sockaddr_ll>() as u32,
                );
                let err = std::io::Error::last_os_error();
                libc::close(fd);
                if n < 0 {
                    return Err(err.into());
                }
            }
        }
        IpAddr::V6(v6) => {
            let pkt = snat::unsolicited_na(mac, v6);
            // SAFETY: plain socket calls on a local fd, closed below.
            unsafe {
                let fd = libc::socket(libc::AF_INET6, libc::SOCK_RAW, libc::IPPROTO_ICMPV6);
                if fd < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                let hops: libc::c_int = 255;
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_IPV6,
                    libc::IPV6_MULTICAST_HOPS,
                    (&hops as *const libc::c_int).cast(),
                    4,
                );
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_IPV6,
                    libc::IPV6_MULTICAST_IF,
                    (&ifindex as *const i32).cast(),
                    4,
                );
                let mut sa: libc::sockaddr_in6 = std::mem::zeroed();
                sa.sin6_family = libc::AF_INET6 as u16;
                sa.sin6_addr.s6_addr = "ff02::1".parse::<std::net::Ipv6Addr>().unwrap().octets();
                sa.sin6_scope_id = ifindex as u32;
                let n = libc::sendto(
                    fd,
                    pkt.as_ptr().cast(),
                    pkt.len(),
                    0,
                    (&sa as *const libc::sockaddr_in6).cast(),
                    std::mem::size_of::<libc::sockaddr_in6>() as u32,
                );
                let err = std::io::Error::last_os_error();
                libc::close(fd);
                if n < 0 {
                    return Err(err.into());
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn announce(_ip: IpAddr, _dev: &str) -> Result<()> {
    Ok(())
}

impl Engine {
    /// Bring the managed addresses in line with `cfg`; returns the host's
    /// addresses afterwards and problems to report.
    fn egress_addresses(&mut self, cfg: &VmEgressSnat) -> (Vec<IpAddr>, Vec<String>) {
        let wanted = snat::wanted_addrs(cfg);
        let mut notes = Vec::new();
        let mut keep = Vec::new();
        for (ip, dev) in std::mem::take(&mut self.egress.managed) {
            if wanted.contains(&ip) {
                keep.push((ip, dev));
            } else if let Err(e) = ip_addr("del", ip, &dev) {
                // Already gone (interface removed, or deleted by hand).
                tracing::info!("egress IP {ip}: {e:#}");
            } else {
                tracing::info!(%ip, %dev, "egress IP removed");
            }
        }
        self.egress.managed = keep;
        let mut local = local_addresses();
        for ip in wanted {
            if local.contains(&ip) {
                continue;
            }
            let Some(dev) = cfg
                .interface
                .clone()
                .or_else(|| uplink(ip.is_ipv6()).or_else(|| uplink(false)))
            else {
                notes.push(format!(
                    "{ip}: no default route to pick an interface; set `interface`"
                ));
                continue;
            };
            match ip_addr("add", ip, &dev) {
                Ok(()) => {
                    tracing::info!(%ip, %dev, "egress IP added");
                    if let Err(e) = announce(ip, &dev) {
                        notes.push(format!("{ip}: added to {dev}, announcement failed: {e:#}"));
                    }
                    self.egress.managed.push((ip, dev));
                    local.push(ip);
                }
                Err(e) => notes.push(format!("{e:#}")),
            }
        }
        (local, notes)
    }

    pub(super) fn vm_egress_set(&mut self, cfg: VmEgressSnat) -> Result<VmEgressSnatStatus> {
        snat::validate(&cfg).map_err(|e| anyhow!(e))?;
        let (local, notes) = self.egress_addresses(&cfg);
        let (script, mut skipped) = snat::render(&cfg, &local);
        skipped.extend(notes);
        let idle = script.is_none() && !self.egress.active && self.egress.error.is_none();
        let res = if idle {
            Ok(())
        } else {
            nft(&snat::transaction(script.as_deref()))
        };
        if let Err(e) = &res {
            tracing::warn!("egress SNAT: {e:#}");
        } else if !idle {
            tracing::info!(
                rules = cfg.rules.len(),
                skipped = skipped.len(),
                "egress SNAT applied"
            );
        }
        let managed = std::mem::take(&mut self.egress.managed);
        self.egress = EgressRuntime {
            active: script.is_some() && res.is_ok(),
            error: res.err().map(|e| format!("{e:#}")),
            config: cfg,
            skipped,
            managed,
        };
        Ok(self.vm_egress_status())
    }

    pub(super) fn vm_egress_status(&self) -> VmEgressSnatStatus {
        VmEgressSnatStatus {
            rules: self.egress.config.rules.clone(),
            exclude: snat::excludes(&self.egress.config),
            active: self.egress.active,
            skipped: self.egress.skipped.clone(),
            error: self.egress.error.clone(),
            hostname: None,
            managed: self
                .egress
                .managed
                .iter()
                .map(|(ip, dev)| format!("{}@{dev}", snat::host_cidr(*ip)))
                .collect(),
        }
    }
}

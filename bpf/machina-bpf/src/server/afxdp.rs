// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! AF_XDP fast path: `mn_xdp_afxdp` on one dedicated interface, with XSK
//! sockets registered per queue over the bpfd socket (SCM_RIGHTS). bpfd only
//! owns the map slot and the gate; the consumer owns its rings, and closing
//! its socket drops the slot (frames then pass to the stack).

use std::collections::BTreeSet;
use std::os::fd::{AsRawFd, OwnedFd};

use super::*;

const PROG: &str = "mn_xdp_afxdp";
const AF_XDP: libc::c_int = 44;

#[derive(Default)]
pub(super) struct AfxdpRuntime {
    iface: Option<String>,
    queues: BTreeSet<u32>,
}

/// Interfaces with an IPv4 or IPv6 default route carry host traffic.
fn has_default_route(iface: &str) -> bool {
    let v4 = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    let v4 = v4.lines().skip(1).any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() > 7 && f[0] == iface && f[1] == "00000000" && f[7] == "00000000"
    });
    let v6 = std::fs::read_to_string("/proc/net/ipv6_route").unwrap_or_default();
    let v6 = v6.lines().any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() > 9 && f[9] == iface && f[0] == "00000000000000000000000000000000" && f[1] == "00"
    });
    v4 || v6
}

fn is_xsk(fd: &OwnedFd) -> bool {
    let mut domain: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_DOMAIN,
            &mut domain as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    rc == 0 && domain == AF_XDP
}

impl Engine {
    pub(super) fn afxdp_configure(&mut self, c: AfxdpConfig) -> Result<AfxdpStatus> {
        if !c.enabled {
            if let Some(iface) = self.afxdp.iface.take() {
                for q in std::mem::take(&mut self.afxdp.queues) {
                    self.dp.array_set("AFXDP_QUEUE_ENABLED", q, 0u32)?;
                    self.dp.xsk_unset(q);
                }
                if self.dp.xdp_attached(&iface) == Some(PROG) {
                    self.dp.detach_xdp(&iface);
                }
            }
            return Ok(self.afxdp_status());
        }
        if c.iface.is_empty() {
            return Err(anyhow!("iface is required (a dedicated interface)"));
        }
        if let Some(cur) = &self.afxdp.iface {
            if *cur != c.iface {
                return Err(anyhow!("AF_XDP already runs on {cur}; disable it first"));
            }
            return Ok(self.afxdp_status());
        }
        if if_nametoindex(&c.iface).is_none() {
            return Err(anyhow!("interface {} not found", c.iface));
        }
        if has_default_route(&c.iface) {
            return Err(anyhow!(
                "{} carries a default route; AF_XDP needs a dedicated interface",
                c.iface
            ));
        }
        if self.uplink.iface.as_deref() == Some(c.iface.as_str()) && self.uplink.flags != 0 {
            return Err(anyhow!(
                "{} runs the uplink XDP dispatcher; AF_XDP needs a dedicated interface",
                c.iface
            ));
        }
        match self.dp.xdp_attached(&c.iface) {
            None => {}
            Some(other) => return Err(anyhow!("{} already has XDP program {other}", c.iface)),
        }
        for q in 0..AFXDP_MAX_QUEUES {
            self.dp.array_set("AFXDP_QUEUE_ENABLED", q, 0u32)?;
        }
        self.dp.attach_xdp_prog(&c.iface, PROG)?;
        self.afxdp.iface = Some(c.iface);
        Ok(self.afxdp_status())
    }

    pub(super) fn afxdp_register(
        &mut self,
        iface: &str,
        queue: u32,
        fd: Option<OwnedFd>,
    ) -> Result<AfxdpStatus> {
        let fd = fd.ok_or_else(|| {
            anyhow!("no socket attached; send the AF_XDP fd as SCM_RIGHTS with this request")
        })?;
        if self.afxdp.iface.as_deref() != Some(iface) {
            return Err(anyhow!("AF_XDP is not enabled on {iface}"));
        }
        if queue >= AFXDP_MAX_QUEUES {
            return Err(anyhow!("queue must be < {AFXDP_MAX_QUEUES}"));
        }
        if !is_xsk(&fd) {
            return Err(anyhow!("the attached fd is not an AF_XDP socket"));
        }
        self.dp.xsk_set(queue, fd.as_raw_fd())?;
        drop(fd);
        self.dp.array_set("AFXDP_QUEUE_ENABLED", queue, 1u32)?;
        self.afxdp.queues.insert(queue);
        Ok(self.afxdp_status())
    }

    pub(super) fn afxdp_unregister(&mut self, iface: &str, queue: u32) -> Result<AfxdpStatus> {
        if self.afxdp.iface.as_deref() != Some(iface) {
            return Err(anyhow!("AF_XDP is not enabled on {iface}"));
        }
        if self.afxdp.queues.remove(&queue) {
            self.dp.array_set("AFXDP_QUEUE_ENABLED", queue, 0u32)?;
            self.dp.xsk_unset(queue);
        }
        Ok(self.afxdp_status())
    }

    pub(super) fn afxdp_status(&mut self) -> AfxdpStatus {
        let queues = self.afxdp.queues.clone();
        let mut out = Vec::with_capacity(queues.len());
        for q in queues {
            let mut s = |i| {
                self.dp
                    .percpu_array_sum::<u64>("AFXDP_STATS", q * AFXDP_STAT_SLOTS + i, |a, b| {
                        *a += *b
                    })
                    .unwrap_or(0)
            };
            out.push(AfxdpQueue {
                queue: q,
                enabled: true,
                redirected: s(AFXDP_STAT_REDIRECT),
                no_socket: s(AFXDP_STAT_NOSOCK),
            });
        }
        let attached = self
            .afxdp
            .iface
            .as_deref()
            .is_some_and(|i| self.dp.xdp_attached(i) == Some(PROG));
        AfxdpStatus {
            iface: self.afxdp.iface.clone(),
            attached,
            queues: out,
        }
    }
}

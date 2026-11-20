// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Machina native eBPF.
//!
//! * [`loader`] (Linux) — loads the embedded `machina-bpf-ebpf` object with aya,
//!   attaches TCX/XDP/cgroup/tracepoint/kprobe programs and manages maps.
//! * [`server`] (Linux) — the `machina-bpfd` service: single owner of the
//!   host datapath, serving newline-delimited JSON on a Unix socket.
//! * [`client`] — what machina-daemon / machina-agent / machina-cni use to talk
//!   to machina-bpfd.
//! * Pure helpers (policy compiler, DNS decoder, pcapng writer, anomaly
//!   detectors, tracefs parser, workload attribution) usable on any OS.

pub mod anomaly;
pub mod api;
pub mod attribution;
pub mod authca;
pub mod btf;
pub mod client;
pub mod dns;
pub mod l7;
pub mod l7sample;
pub mod netpol;
pub mod pcapng;
pub mod policy;
pub mod rtnl;
pub mod startup;
pub mod tracefs;
pub mod vmintel;

#[cfg(target_os = "linux")]
pub mod loader;
#[cfg(target_os = "linux")]
mod loader_maps;
#[cfg(target_os = "linux")]
pub mod server;

pub use api::*;
pub use client::BpfdClient;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Format a datapath address (16-byte, IPv4-mapped) for display.
pub fn fmt_addr(a: &[u8; 16]) -> String {
    if a[..10].iter().all(|b| *b == 0) && a[10] == 0xff && a[11] == 0xff {
        std::net::Ipv4Addr::new(a[12], a[13], a[14], a[15]).to_string()
    } else {
        std::net::Ipv6Addr::from(*a).to_string()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn addr_display() {
        let v4 = machina_bpf_common::v4_mapped([192, 168, 1, 2]);
        assert_eq!(super::fmt_addr(&v4), "192.168.1.2");
        let mut v6 = [0u8; 16];
        v6[15] = 1;
        assert_eq!(super::fmt_addr(&v6), "::1");
    }
}

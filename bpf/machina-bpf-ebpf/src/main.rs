// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Machina native eBPF programs (one object, loaded by machina-bpfd and machina-cni).

#![no_std]
#![no_main]

mod afxdp;
mod cni;
mod direct;
mod quiclb;
mod health;
mod l7sample;
mod maps;
mod vmintel;
mod vmmguard;
mod net;
mod nodeiso;
mod parse;
mod proc;
mod rtnl;
mod shield;
mod tcp;
mod tls;
mod vm;
mod xdp;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// GPL-compatible license is required for bpf_probe_read_*, bpf_send_signal
// and the tracepoint helpers.
#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 4] = *b"GPL\0";

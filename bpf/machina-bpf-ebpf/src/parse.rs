// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Packet parsing through `bpf_skb_load_bytes` (TC, cgroup_skb) and
//! `bpf_xdp_load_bytes` (XDP, so the parse can run in a subprogram).

use core::ffi::c_void;

use aya_ebpf::bindings::__sk_buff;
use aya_ebpf::helpers::{bpf_skb_load_bytes, generated::bpf_xdp_load_bytes};
use aya_ebpf::programs::{SkBuffContext, TcContext, XdpContext};
use aya_ebpf::EbpfContext;
use machina_bpf_common::ADDR_LEN;

pub const ETH_HLEN: usize = 14;
pub const ETH_P_IP: u16 = 0x0800;
pub const ETH_P_IPV6: u16 = 0x86dd;
pub const IPPROTO_TCP: u8 = 6;
pub const IPPROTO_UDP: u8 = 17;

const IPV6_HOPOPTS: u8 = 0;
const IPV6_ROUTING: u8 = 43;
const IPV6_FRAGMENT: u8 = 44;
const IPV6_AH: u8 = 51;
const IPV6_DSTOPTS: u8 = 60;
const MAX_V6_EXT: usize = 4;

pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_ACK: u8 = 0x10;

pub trait Pkt {
    fn ld<T>(&self, off: usize) -> Option<T>;
    /// Load straight into `dst` (no stack temporary). The length must be a
    /// verifier-visible constant, hence the array type.
    fn ld_into<const N: usize>(&self, off: usize, dst: &mut [u8; N]) -> bool;
}

#[inline(always)]
fn skb_load<const N: usize>(skb: *mut __sk_buff, off: usize, dst: &mut [u8; N]) -> bool {
    unsafe { bpf_skb_load_bytes(skb as *const c_void, off as u32, dst.as_mut_ptr().cast(), N as u32) == 0 }
}

/// The last four bytes of a 16-byte address (IPv4-mapped slot).
#[inline(always)]
fn v4_slot(a: &mut [u8; ADDR_LEN]) -> &mut [u8; 4] {
    unsafe { &mut *(a.as_mut_ptr().add(12) as *mut [u8; 4]) }
}

impl Pkt for TcContext {
    #[inline(always)]
    fn ld<T>(&self, off: usize) -> Option<T> {
        self.load::<T>(off).ok()
    }
    #[inline(always)]
    fn ld_into<const N: usize>(&self, off: usize, dst: &mut [u8; N]) -> bool {
        skb_load(self.skb.skb, off, dst)
    }
}

impl Pkt for SkBuffContext {
    #[inline(always)]
    fn ld<T>(&self, off: usize) -> Option<T> {
        self.load::<T>(off).ok()
    }
    #[inline(always)]
    fn ld_into<const N: usize>(&self, off: usize, dst: &mut [u8; N]) -> bool {
        skb_load(self.skb.skb, off, dst)
    }
}

impl Pkt for XdpContext {
    #[inline(always)]
    fn ld<T>(&self, off: usize) -> Option<T> {
        let mut v = core::mem::MaybeUninit::<T>::uninit();
        let r = unsafe {
            bpf_xdp_load_bytes(self.as_ptr().cast(), off as u32, v.as_mut_ptr().cast(), core::mem::size_of::<T>() as u32)
        };
        (r == 0).then(|| unsafe { v.assume_init() })
    }
    #[inline(always)]
    fn ld_into<const N: usize>(&self, off: usize, dst: &mut [u8; N]) -> bool {
        unsafe { bpf_xdp_load_bytes(self.as_ptr().cast(), off as u32, dst.as_mut_ptr().cast(), N as u32) == 0 }
    }
}

#[derive(Clone, Copy)]
pub struct Tuple {
    pub l3_off: usize,
    pub l4_off: usize,
    /// Offset of the L4 payload (UDP/TCP data); 0 when unknown.
    pub payload_off: usize,
    pub v6: bool,
    pub proto: u8,
    pub tcp_flags: u8,
    pub sport: u16,
    pub dport: u16,
    pub src: [u8; ADDR_LEN],
    pub dst: [u8; ADDR_LEN],
}

/// Ethertype of an Ethernet frame (TC programs).
#[inline(always)]
pub fn ethertype<P: Pkt>(p: &P) -> Option<u16> {
    let b: [u8; 2] = p.ld(12)?;
    Some(u16::from_be_bytes(b))
}

/// Parse IPv4/IPv6 + TCP/UDP starting at `l3_off` into a zeroed `t`.
/// Writes in place: returning `Option<Tuple>` materialises several copies
/// of the tuple and blows the 512-byte BPF stack budget.
#[inline(always)]
fn parse_l3_into<P: Pkt>(p: &P, l3_off: usize, t: &mut Tuple) -> bool {
    let Some(vihl) = p.ld::<u8>(l3_off) else {
        return false;
    };
    t.l3_off = l3_off;
    let mut fragmented = false;
    match vihl >> 4 {
        4 => {
            let ihl = ((vihl & 0x0f) as usize) * 4;
            if ihl < 20 {
                return false;
            }
            let Some(proto) = p.ld::<u8>(l3_off + 9) else {
                return false;
            };
            t.proto = proto;
            let Some(frag) = p.ld::<[u8; 2]>(l3_off + 6) else {
                return false;
            };
            fragmented = (u16::from_be_bytes(frag) & 0x1fff) != 0;
            t.src[10] = 0xff;
            t.src[11] = 0xff;
            t.dst[10] = 0xff;
            t.dst[11] = 0xff;
            if !p.ld_into(l3_off + 12, v4_slot(&mut t.src)) || !p.ld_into(l3_off + 16, v4_slot(&mut t.dst)) {
                return false;
            }
            t.l4_off = l3_off + ihl;
        }
        6 => {
            t.v6 = true;
            let Some(proto) = p.ld::<u8>(l3_off + 6) else {
                return false;
            };
            if !p.ld_into(l3_off + 8, &mut t.src) || !p.ld_into(l3_off + 24, &mut t.dst) {
                return false;
            }
            let mut next = proto;
            let mut off = l3_off + 40;
            // Bounded extension-header walk (hop-by-hop, routing, fragment,
            // destination options, AH); anything deeper is left unparsed.
            let mut i = 0;
            while i < MAX_V6_EXT {
                let len = match next {
                    IPV6_HOPOPTS | IPV6_ROUTING | IPV6_DSTOPTS => match p.ld::<[u8; 2]>(off) {
                        Some(h) => {
                            next = h[0];
                            (h[1] as usize + 1) * 8
                        }
                        None => return false,
                    },
                    IPV6_FRAGMENT => match p.ld::<[u8; 4]>(off) {
                        Some(h) => {
                            next = h[0];
                            fragmented = (u16::from_be_bytes([h[2], h[3]]) & 0xfff8) != 0;
                            8
                        }
                        None => return false,
                    },
                    IPV6_AH => match p.ld::<[u8; 2]>(off) {
                        Some(h) => {
                            next = h[0];
                            (h[1] as usize + 2) * 4
                        }
                        None => return false,
                    },
                    _ => break,
                };
                off += len;
                i += 1;
            }
            t.proto = next;
            t.l4_off = off;
        }
        _ => return false,
    }
    if fragmented {
        return true;
    }
    match t.proto {
        IPPROTO_TCP => {
            let Some(ports) = p.ld::<[u8; 4]>(t.l4_off) else {
                return false;
            };
            t.sport = u16::from_be_bytes([ports[0], ports[1]]);
            t.dport = u16::from_be_bytes([ports[2], ports[3]]);
            let Some(off_flags) = p.ld::<[u8; 2]>(t.l4_off + 12) else {
                return false;
            };
            t.tcp_flags = off_flags[1];
            t.payload_off = t.l4_off + ((off_flags[0] >> 4) as usize) * 4;
        }
        IPPROTO_UDP => {
            let Some(ports) = p.ld::<[u8; 4]>(t.l4_off) else {
                return false;
            };
            t.sport = u16::from_be_bytes([ports[0], ports[1]]);
            t.dport = u16::from_be_bytes([ports[2], ports[3]]);
            t.payload_off = t.l4_off + 8;
        }
        _ => {}
    }
    true
}

impl Tuple {
    pub const fn zero() -> Self {
        Tuple {
            l3_off: 0,
            l4_off: 0,
            payload_off: 0,
            v6: false,
            proto: 0,
            tcp_flags: 0,
            sport: 0,
            dport: 0,
            src: [0; ADDR_LEN],
            dst: [0; ADDR_LEN],
        }
    }
}

/// Out-of-line Ethernet parse for TC programs (keeps parse temporaries out
/// of the caller's stack frame). Returns 1 on success.
#[inline(never)]
pub fn parse_tc(ctx: &TcContext, out: &mut Tuple) -> u32 {
    *out = Tuple::zero();
    match ethertype(ctx) {
        Some(ETH_P_IP | ETH_P_IPV6) => parse_l3_into(ctx, ETH_HLEN, out) as u32,
        _ => 0,
    }
}

/// Out-of-line Ethernet parse for XDP (helper loads, no packet pointers).
#[inline(never)]
pub fn parse_xdp(ctx: &XdpContext, out: &mut Tuple) -> u32 {
    *out = Tuple::zero();
    match ethertype(ctx) {
        Some(ETH_P_IP | ETH_P_IPV6) => parse_l3_into(ctx, ETH_HLEN, out) as u32,
        _ => 0,
    }
}

/// Parse an encapsulated IPv4 packet at `off` (IPIP inner header).
#[inline(never)]
pub fn parse_inner_v4(ctx: &TcContext, off: usize, out: &mut Tuple) -> u32 {
    *out = Tuple::zero();
    (parse_l3_into(ctx, off, out) && !out.v6) as u32
}

/// Out-of-line L3 parse for cgroup_skb programs (no Ethernet header).
#[inline(never)]
pub fn parse_skb(ctx: &SkBuffContext, out: &mut Tuple) -> u32 {
    *out = Tuple::zero();
    parse_l3_into(ctx, 0, out) as u32
}

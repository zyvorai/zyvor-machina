// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM edge L7. The kernel copies every TCP segment past a flow's window in
//! VM_L7_FLOW to VM_L7_EVENTS and, with enforcement live, holds it. Per
//! flow, bpfd orders the segments, parses the client's stream
//! (`netpol::l7stream`), writes the allowed window back and reinjects the
//! held frames through the inject veth, splitting a frame where a verdict
//! boundary falls inside it. Denied clients get an HTTP/1 403 or a TCP RST
//! (and the server a RST), denied DNS queries a REFUSED; allowed UDP DNS
//! queries are reinjected once. Without the inject veth the window still
//! opens and the client's retransmit passes; allowed DNS queries are then
//! forwarded from the host.

use std::collections::{BTreeMap, HashMap as StdMap};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use aya::maps::{HashMap as BpfHash, MapData};

use super::*;
use crate::netpol::l7::{self, Matcher};
use crate::netpol::l7stream::{self, Kind, Stream, Verdict};

const FORBIDDEN: &[u8] =
    b"HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\nContent-Length: 15\r\nConnection: close\r\n\r\nAccess denied\r\n";
const DNS_FORWARD_TIMEOUT: Duration = Duration::from_secs(2);
const TCP_FIN: u8 = 0x01;
const TCP_SYN: u8 = 0x02;
const TCP_RST: u8 = 0x04;
const TCP_PSH: u8 = 0x08;
const TCP_ACK: u8 = 0x10;
const FLOW_CAP: usize = 65536;
const FLOW_IDLE: Duration = Duration::from_secs(300);
const SWEEP_EVERY: Duration = Duration::from_secs(10);
/// Captured bytes held per flow before it is denied.
const HELD_CAP: usize = l7stream::KAFKA_MAX + (1 << 20);
/// Kernel window span limit (its skip field is 31 bits wide).
const WINDOW_MAX: u64 = 1 << 30;
/// Unseen body bytes let through per window update.
const SKIP_MAX: u64 = WINDOW_MAX / 2;
const PACKET_VNET_HDR: libc::c_int = 15;
const VNET_HDR_LEN: usize = 10;
const VNET_NEEDS_CSUM: u8 = 1;
const VNET_GSO_TCPV4: u8 = 1;
const VNET_GSO_TCPV6: u8 = 4;

type CacheKey = (u32, u32, bool, u8, u16);

#[derive(Clone)]
struct Compiled {
    matcher: Arc<Matcher>,
    kinds: Vec<&'static str>,
    source: Option<String>,
}

/// VmL7Event without `data`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Hdr {
    ts_ns: u64,
    ifindex: u32,
    subject: u32,
    peer: u32,
    seq: u32,
    ack: u32,
    len: u32,
    cap: u32,
    skip: u32,
    frame_len: u32,
    payload_off: u16,
    gso_size: u16,
    src: [u8; ADDR_LEN],
    dst: [u8; ADDR_LEN],
    sport: u16,
    dport: u16,
    proto: u8,
    from_vm: u8,
    held: u8,
    v6: u8,
    l3_off: u16,
    l4_off: u16,
    ingress: u8,
    _pad: [u8; 3],
}

/// One captured segment; its payload sits at `off` in the client's stream.
struct Seg {
    off: u64,
    frame: Vec<u8>,
    l3: usize,
    l4: usize,
    pay: usize,
    gso: u16,
    v6: bool,
    ingress: bool,
    held: bool,
}

impl Seg {
    fn len(&self) -> usize {
        self.frame.len() - self.pay
    }

    fn end(&self) -> u64 {
        self.off + self.len() as u64
    }

    fn payload(&self) -> &[u8] {
        &self.frame[self.pay..]
    }

    /// Payload bytes `[a, b)` as a segment of their own.
    fn slice(&self, a: usize, b: usize) -> Seg {
        let mut frame = Vec::with_capacity(self.pay + b - a);
        frame.extend_from_slice(&self.frame[..self.pay]);
        frame.extend_from_slice(&self.frame[self.pay + a..self.pay + b]);
        let t = self.l4;
        let seq = u32::from_be_bytes([frame[t + 4], frame[t + 5], frame[t + 6], frame[t + 7]]);
        frame[t + 4..t + 8].copy_from_slice(&seq.wrapping_add(a as u32).to_be_bytes());
        if a > 0 {
            frame[t + 13] &= !TCP_SYN;
        }
        if b < self.len() {
            frame[t + 13] = (frame[t + 13] & !TCP_FIN) | TCP_PSH;
        }
        Seg {
            off: self.off + a as u64,
            frame,
            l3: self.l3,
            l4: self.l4,
            pay: self.pay,
            gso: self.gso,
            v6: self.v6,
            ingress: self.ingress,
            held: self.held,
        }
    }

    /// virtio_net_hdr + frame with the IP length and checksum fixed and the
    /// TCP checksum (and segmentation) left to the offload.
    fn wire(&self) -> Vec<u8> {
        let (l3, l4) = (self.l3, self.l4);
        let mut f = self.frame.clone();
        let l4len = f.len() - l4;
        let mut pseudo = Vec::with_capacity(40);
        if self.v6 {
            let n = (f.len() - l3 - 40) as u16;
            f[l3 + 4..l3 + 6].copy_from_slice(&n.to_be_bytes());
            pseudo.extend_from_slice(&f[l3 + 8..l3 + 40]);
            pseudo.extend_from_slice(&(l4len as u32).to_be_bytes());
            pseudo.extend_from_slice(&[0, 0, 0, policy::IPPROTO_TCP]);
        } else {
            let ihl = ((f[l3] & 0x0f) as usize) * 4;
            let total = (f.len() - l3) as u16;
            f[l3 + 2..l3 + 4].copy_from_slice(&total.to_be_bytes());
            f[l3 + 10..l3 + 12].fill(0);
            let hc = csum_fold(csum_add(0, &f[l3..l3 + ihl]));
            f[l3 + 10..l3 + 12].copy_from_slice(&hc.to_be_bytes());
            pseudo.extend_from_slice(&f[l3 + 12..l3 + 20]);
            pseudo.extend_from_slice(&[0, policy::IPPROTO_TCP]);
            pseudo.extend_from_slice(&(l4len as u16).to_be_bytes());
        }
        let partial = !csum_fold(csum_add(0, &pseudo));
        f[l4 + 16..l4 + 18].copy_from_slice(&partial.to_be_bytes());
        let gso = self.gso > 0 && self.len() > self.gso as usize;
        let mut out = Vec::with_capacity(VNET_HDR_LEN + f.len());
        out.push(VNET_NEEDS_CSUM);
        out.push(match (gso, self.v6) {
            (false, _) => 0,
            (true, false) => VNET_GSO_TCPV4,
            (true, true) => VNET_GSO_TCPV6,
        });
        out.extend_from_slice(&(self.pay as u16).to_ne_bytes());
        out.extend_from_slice(&(if gso { self.gso } else { 0 }).to_ne_bytes());
        out.extend_from_slice(&(l4 as u16).to_ne_bytes());
        out.extend_from_slice(&16u16.to_ne_bytes());
        out.extend_from_slice(&f);
        out
    }
}

struct Flow {
    c: Compiled,
    stream: Stream,
    /// Sequence number of stream offset 0.
    seq0: u32,
    segs: BTreeMap<u64, Seg>,
    held_bytes: usize,
    /// Stream offset the kernel window and reinjection have reached.
    released: u64,
    hdr: Hdr,
    /// Ethernet addresses (dst, src) and hook of client → server frames.
    l2: [u8; 12],
    ingress: bool,
    last: Instant,
}

impl Flow {
    /// Stream offset of `seq`, relative to the released point so long
    /// streams do not wrap.
    fn offset(&self, seq: u32) -> i64 {
        let at = self.seq0.wrapping_add(self.released as u32);
        self.released as i64 + seq.wrapping_sub(at) as i32 as i64
    }

    fn seq(&self, off: u64) -> u32 {
        self.seq0.wrapping_add(off as u32)
    }

    fn window(&self, state: u8) -> VmL7Flow {
        let r = self.stream.allowed();
        let pass_until = self.seq(r);
        VmL7Flow {
            allow_seq: pass_until.wrapping_sub(r.min(WINDOW_MAX - 1) as u32),
            pass_until,
            pending_seq: 0,
            state,
            _pad: [0; 3],
        }
    }
}

enum Outcome {
    Pending,
    Open,
    Denied,
}

pub(super) struct L7Worker {
    kmap: BpfHash<MapData, FlowKey, VmL7Flow>,
    gen: u64,
    cache: StdMap<CacheKey, Option<Compiled>>,
    flows: StdMap<FlowKey, Flow>,
    swept: Instant,
    /// Plain AF_PACKET socket: answers straight onto the tap or its master.
    raw: Option<Arc<OwnedFd>>,
    /// AF_PACKET socket with PACKET_VNET_HDR on the inject veth.
    inj: Option<OwnedFd>,
}

fn compile(
    rules: &[VmEdgeL7Rule],
    (subject, peer, egress, proto, port): CacheKey,
) -> Option<Compiled> {
    let hits: Vec<&VmEdgeL7Rule> = rules
        .iter()
        .filter(|r| {
            r.subject_identity == subject
                && r.egress == egress
                && (r.peer_identity == 0 || r.peer_identity == peer)
        })
        .filter(|r| r.proto == 0 || r.proto == proto)
        .filter(|r| r.port == 0 || (r.port..=r.port_end.max(r.port)).contains(&port))
        .collect();
    if hits.is_empty() {
        return None;
    }
    let mut kinds: Vec<&'static str> = hits.iter().map(|r| r.rules.kind()).collect();
    kinds.sort();
    kinds.dedup();
    Some(Compiled {
        matcher: Arc::new(Matcher::new(hits.iter().map(|r| &r.rules))),
        kinds,
        source: hits.iter().find_map(|r| r.source.clone()),
    })
}

fn addr_ip(a: &[u8; ADDR_LEN], v6: bool) -> IpAddr {
    if v6 {
        IpAddr::V6(Ipv6Addr::from(*a))
    } else {
        IpAddr::V4(Ipv4Addr::new(a[12], a[13], a[14], a[15]))
    }
}

fn summarize(v: &Verdict) -> String {
    let mut s = v.req.summary();
    if let Some(n) = &v.note {
        s.push_str(&format!(" ({n})"));
    }
    if !v.allowed {
        s.push_str(" [denied]");
    }
    s
}

fn packet_socket(vnet: bool) -> Option<OwnedFd> {
    let fd = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW, 0) };
    if fd < 0 {
        return None;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    if vnet {
        let one: libc::c_int = 1;
        let r = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_PACKET,
                PACKET_VNET_HDR,
                (&one as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as u32,
            )
        };
        if r < 0 {
            return None;
        }
    }
    Some(fd)
}

fn inject_mark(ifindex: u32, ingress: bool) -> u32 {
    VM_L7_INJECT_MAGIC
        | if ingress { VM_L7_INJECT_INGRESS } else { 0 }
        | (ifindex & VM_L7_INJECT_IFINDEX)
}

/// Send `wire` (virtio_net_hdr + frame) on the inject veth; the mark tells
/// `mn_vm_l7_inject` where to redirect it.
fn send_inject(sock: &OwnedFd, inject: u32, mark: u32, wire: &[u8]) {
    if wire.len() < VNET_HDR_LEN + 14 {
        return;
    }
    let r = unsafe {
        libc::setsockopt(
            sock.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_MARK,
            (&mark as *const u32).cast(),
            4,
        )
    };
    if r < 0 {
        tracing::debug!("vm l7: SO_MARK: {}", std::io::Error::last_os_error());
        return;
    }
    let f = &wire[VNET_HDR_LEN..];
    let mut sa: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    sa.sll_family = libc::AF_PACKET as u16;
    sa.sll_protocol = u16::from_be_bytes([f[12], f[13]]).to_be();
    sa.sll_ifindex = inject as i32;
    sa.sll_halen = 6;
    sa.sll_addr[..6].copy_from_slice(&f[..6]);
    let r = unsafe {
        libc::sendto(
            sock.as_raw_fd(),
            wire.as_ptr().cast(),
            wire.len(),
            0,
            (&sa as *const libc::sockaddr_ll).cast(),
            std::mem::size_of::<libc::sockaddr_ll>() as u32,
        )
    };
    if r < 0 {
        tracing::debug!("vm l7: reinject: {}", std::io::Error::last_os_error());
    }
}

/// Frame built by [`frame`] behind an empty virtio_net_hdr.
fn plain_wire(f: &[u8]) -> Vec<u8> {
    let mut w = vec![0u8; VNET_HDR_LEN];
    w.extend_from_slice(f);
    w
}

// ---- frames ------------------------------------------------------------------

fn csum_add(mut sum: u32, b: &[u8]) -> u32 {
    let mut i = 0;
    while i + 1 < b.len() {
        sum += u16::from_be_bytes([b[i], b[i + 1]]) as u32;
        i += 2;
    }
    if i < b.len() {
        sum += (b[i] as u32) << 8;
    }
    sum
}

fn csum_fold(mut sum: u32) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Ethernet + IP frame around a TCP or UDP `l4` segment with its checksum zeroed.
fn frame(
    eth_dst: &[u8],
    eth_src: &[u8],
    src: &[u8; ADDR_LEN],
    dst: &[u8; ADDR_LEN],
    v6: bool,
    proto: u8,
    mut l4: Vec<u8>,
) -> Vec<u8> {
    let csum_at = if proto == policy::IPPROTO_UDP { 6 } else { 16 };
    let len = l4.len();
    let mut pseudo = Vec::with_capacity(40);
    if v6 {
        pseudo.extend_from_slice(src);
        pseudo.extend_from_slice(dst);
        pseudo.extend_from_slice(&(len as u32).to_be_bytes());
        pseudo.extend_from_slice(&[0, 0, 0, proto]);
    } else {
        pseudo.extend_from_slice(&src[12..]);
        pseudo.extend_from_slice(&dst[12..]);
        pseudo.extend_from_slice(&[0, proto]);
        pseudo.extend_from_slice(&(len as u16).to_be_bytes());
    }
    let mut c = csum_fold(csum_add(csum_add(0, &pseudo), &l4));
    if c == 0 && proto == policy::IPPROTO_UDP {
        c = 0xffff;
    }
    l4[csum_at..csum_at + 2].copy_from_slice(&c.to_be_bytes());
    let mut f = Vec::with_capacity(14 + 40 + len);
    f.extend_from_slice(eth_dst);
    f.extend_from_slice(eth_src);
    if v6 {
        f.extend_from_slice(&[0x86, 0xdd, 0x60, 0, 0, 0]);
        f.extend_from_slice(&(len as u16).to_be_bytes());
        f.extend_from_slice(&[proto, 64]);
        f.extend_from_slice(src);
        f.extend_from_slice(dst);
    } else {
        let mut ip = vec![0x45, 0];
        ip.extend_from_slice(&((20 + len) as u16).to_be_bytes());
        ip.extend_from_slice(&[0, 0, 0x40, 0, 64, proto, 0, 0]);
        ip.extend_from_slice(&src[12..]);
        ip.extend_from_slice(&dst[12..]);
        let hc = csum_fold(csum_add(0, &ip));
        ip[10..12].copy_from_slice(&hc.to_be_bytes());
        f.extend_from_slice(&[0x08, 0x00]);
        f.extend_from_slice(&ip);
    }
    f.extend_from_slice(&l4);
    f
}

fn tcp_seg(sport: u16, dport: u16, seq: u32, ack: u32, flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut t = Vec::with_capacity(20 + payload.len());
    t.extend_from_slice(&sport.to_be_bytes());
    t.extend_from_slice(&dport.to_be_bytes());
    t.extend_from_slice(&seq.to_be_bytes());
    t.extend_from_slice(&ack.to_be_bytes());
    t.extend_from_slice(&[0x50, flags]);
    t.extend_from_slice(&(if flags & TCP_RST != 0 { 0u16 } else { 65535 }).to_be_bytes());
    t.extend_from_slice(&[0, 0, 0, 0]);
    t.extend_from_slice(payload);
    t
}

fn udp_dgram(sport: u16, dport: u16, payload: &[u8]) -> Vec<u8> {
    let mut u = Vec::with_capacity(8 + payload.len());
    u.extend_from_slice(&sport.to_be_bytes());
    u.extend_from_slice(&dport.to_be_bytes());
    u.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
    u.extend_from_slice(&[0, 0]);
    u.extend_from_slice(payload);
    u
}

/// REFUSED answer to a DNS query (header + question only).
fn dns_refused(q: &[u8]) -> Option<Vec<u8>> {
    if q.len() < 12 {
        return None;
    }
    let mut i = 12;
    while i < q.len() && q[i] != 0 {
        if q[i] & 0xc0 != 0 {
            return None;
        }
        i += 1 + q[i] as usize;
    }
    let end = i + 5;
    if end > q.len() {
        return None;
    }
    let mut r = q[..end].to_vec();
    r[2] = 0x80 | (q[2] & 0x79);
    r[3] = 0x80 | 5;
    r[4] = 0;
    r[5] = 1;
    r[6..12].fill(0);
    Some(r)
}

fn send_frame(sock: &OwnedFd, ifindex: u32, f: &[u8]) {
    if ifindex == 0 || f.len() < 14 {
        return;
    }
    let mut sa: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    sa.sll_family = libc::AF_PACKET as u16;
    sa.sll_protocol = u16::from_be_bytes([f[12], f[13]]).to_be();
    sa.sll_ifindex = ifindex as i32;
    sa.sll_halen = 6;
    sa.sll_addr[..6].copy_from_slice(&f[..6]);
    let r = unsafe {
        libc::sendto(
            sock.as_raw_fd(),
            f.as_ptr().cast(),
            f.len(),
            0,
            (&sa as *const libc::sockaddr_ll).cast(),
            std::mem::size_of::<libc::sockaddr_ll>() as u32,
        )
    };
    if r < 0 {
        tracing::debug!(
            "vm l7: inject on ifindex {ifindex}: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Bridge (or other master) of a tap: where frames towards the far side go.
fn master_of(ifindex: u32) -> u32 {
    let mut buf = [0 as libc::c_char; libc::IF_NAMESIZE];
    if unsafe { libc::if_indextoname(ifindex, buf.as_mut_ptr()) }.is_null() {
        return 0;
    }
    let name = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    std::fs::read_link(format!("/sys/class/net/{name}/master"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .and_then(|m| if_nametoindex(&m))
        .unwrap_or(0)
}

impl L7Worker {
    pub(super) fn new(kmap: BpfHash<MapData, FlowKey, VmL7Flow>) -> Self {
        let raw = packet_socket(false).map(Arc::new);
        if raw.is_none() {
            tracing::warn!(
                "vm l7: AF_PACKET socket: {}; denied flows get no reply",
                std::io::Error::last_os_error()
            );
        }
        let inj = packet_socket(true);
        if inj.is_none() {
            tracing::warn!(
                "vm l7: PACKET_VNET_HDR socket: {}; held segments wait for retransmits",
                std::io::Error::last_os_error()
            );
        }
        L7Worker {
            kmap,
            gen: u64::MAX,
            cache: StdMap::new(),
            flows: StdMap::new(),
            swept: Instant::now(),
            raw,
            inj,
        }
    }

    pub(super) fn on_event(
        &mut self,
        sh: &SharedState,
        bus: &broadcast::Sender<StreamEvent>,
        b: &[u8],
    ) {
        if b.len() < VM_L7_HDR {
            return;
        }
        let h = unsafe { std::ptr::read_unaligned(b.as_ptr() as *const Hdr) };
        let cap = (h.cap as usize).min(VM_L7_CAP).min(b.len() - VM_L7_HDR);
        let (l3, l4, pay) = (h.l3_off as usize, h.l4_off as usize, h.payload_off as usize);
        if !(14 <= l3 && l3 < l4 && l4 < pay && pay < cap) {
            return;
        }
        let plen = (h.len as usize).min(cap - pay);
        let (rules, gen, inject) = {
            let s = lock(sh);
            (s.vm_l7_rules.clone(), s.vm_l7_gen, s.vm_l7_inject)
        };
        if gen != self.gen {
            self.cache.clear();
            self.gen = gen;
        }
        let ck = (h.subject, h.peer, h.from_vm != 0, h.proto, h.dport);
        let compiled = self
            .cache
            .entry(ck)
            .or_insert_with(|| compile(&rules, ck))
            .clone();
        let key = FlowKey {
            ifindex: h.ifindex,
            proto: h.proto,
            local_port: h.sport,
            remote_port: h.dport,
            local: h.src,
            remote: h.dst,
            ..FlowKey::default()
        };
        let seg = Seg {
            off: 0,
            frame: b[VM_L7_HDR..VM_L7_HDR + pay + plen].to_vec(),
            l3,
            l4,
            pay,
            gso: h.gso_size,
            v6: h.v6 != 0,
            ingress: h.ingress != 0,
            held: h.held != 0,
        };
        if h.proto == policy::IPPROTO_UDP {
            self.on_udp(sh, bus, &h, key, seg, compiled, inject);
            return;
        }
        if h.proto != policy::IPPROTO_TCP {
            return;
        }
        let Some(c) = compiled else {
            let open = VmL7Flow {
                state: L7S_OPEN,
                ..VmL7Flow::default()
            };
            let _ = self.kmap.insert(key, open, 0);
            self.flows.remove(&key);
            if seg.held {
                self.reinject(inject, h.ifindex, &seg);
            }
            return;
        };
        self.sweep();
        self.on_tcp(sh, bus, &h, key, seg, c, inject);
    }

    fn sweep(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.swept) < SWEEP_EVERY && self.flows.len() < FLOW_CAP {
            return;
        }
        self.swept = now;
        self.flows
            .retain(|_, f| now.duration_since(f.last) < FLOW_IDLE);
        if self.flows.len() >= FLOW_CAP {
            if let Some(k) = self
                .flows
                .iter()
                .min_by_key(|(_, f)| f.last)
                .map(|(k, _)| *k)
            {
                self.flows.remove(&k);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_tcp(
        &mut self,
        sh: &SharedState,
        bus: &broadcast::Sender<StreamEvent>,
        h: &Hdr,
        key: FlowKey,
        mut seg: Seg,
        c: Compiled,
        inject: u32,
    ) {
        if seg.len() == 0 {
            return;
        }
        let first = seg.payload()[0];
        let flow = self.flows.entry(key).or_insert_with(|| {
            let mut l2 = [0u8; 12];
            l2.copy_from_slice(&seg.frame[..12]);
            Flow {
                stream: Stream::new(l7stream::pick_kind(&c.kinds, first)),
                c,
                seq0: h.seq,
                segs: BTreeMap::new(),
                held_bytes: 0,
                released: 0,
                hdr: *h,
                l2,
                ingress: seg.ingress,
                last: Instant::now(),
            }
        });
        flow.last = Instant::now();
        flow.hdr = *h;
        let mut off = flow.offset(h.seq);
        if off < 0 && flow.stream.pos() == 0 && flow.released == 0 {
            let shift = off.unsigned_abs();
            flow.segs = std::mem::take(&mut flow.segs)
                .into_values()
                .map(|mut s| {
                    s.off += shift;
                    (s.off, s)
                })
                .collect();
            flow.seq0 = h.seq;
            off = 0;
        }
        if off < 0 {
            let cut = off.unsigned_abs() as usize;
            if cut >= seg.len() {
                return;
            }
            seg = seg.slice(cut, seg.len());
            off = 0;
        }
        seg.off = off as u64;
        if seg.end() <= flow.released {
            if seg.held {
                let ifindex = h.ifindex;
                self.reinject(inject, ifindex, &seg);
            }
            return;
        }
        if flow.segs.get(&seg.off).is_none_or(|o| o.len() < seg.len()) {
            flow.held_bytes += seg.len();
            if let Some(old) = flow.segs.insert(seg.off, seg) {
                flow.held_bytes -= old.len();
            }
        }
        self.advance(sh, bus, key, inject);
    }

    /// Feed the in-order bytes of a flow, write the window and release the
    /// segments it covers.
    fn advance(
        &mut self,
        sh: &SharedState,
        bus: &broadcast::Sender<StreamEvent>,
        key: FlowKey,
        inject: u32,
    ) {
        let Some(flow) = self.flows.get_mut(&key) else {
            return;
        };
        let m = flow.c.matcher.clone();
        let mut check = |r: &l7::Request| m.check(r);
        let mut verdicts = Vec::new();
        let mut error = None;
        let outcome = loop {
            if flow.stream.is_open() {
                break Outcome::Open;
            }
            if flow.stream.is_denied() {
                break Outcome::Denied;
            }
            let pos = flow.stream.pos();
            let cover = flow
                .segs
                .range(..=pos)
                .rev()
                .map(|(_, s)| s)
                .find(|s| s.end() > pos);
            match cover {
                Some(s) => {
                    let p = flow
                        .stream
                        .feed(&s.payload()[(pos - s.off) as usize..], &mut check);
                    verdicts.extend(p.verdicts);
                    if p.error.is_some() {
                        error = p.error;
                    }
                }
                None => {
                    let body = flow.stream.body_left().min(SKIP_MAX);
                    if body == 0 {
                        break Outcome::Pending;
                    }
                    flow.stream.skip(body);
                }
            }
        };
        let outcome = match outcome {
            Outcome::Pending if flow.held_bytes > HELD_CAP => {
                error = Some(format!(
                    "{} request over {HELD_CAP} held bytes",
                    flow.stream.kind().name()
                ));
                Outcome::Denied
            }
            o => o,
        };
        let any_held = flow.segs.values().any(|s| s.held);
        let state = match outcome {
            Outcome::Pending => L7S_NONE,
            Outcome::Open => L7S_OPEN,
            Outcome::Denied => L7S_DENIED,
        };
        let r = flow.stream.allowed();
        if state != L7S_NONE || r > flow.released {
            if let Err(e) = self.kmap.insert(key, flow.window(state), 0) {
                tracing::warn!("vm l7: window write: {e}");
            }
        }
        let upto = match outcome {
            Outcome::Open => u64::MAX,
            _ => r,
        };
        if matches!(outcome, Outcome::Pending) {
            Self::release(&self.inj, inject, flow, upto);
            flow.released = flow.released.max(r);
            let (hdr, src) = (flow.hdr, flow.c.source.clone());
            for v in &verdicts {
                self.record(
                    sh,
                    bus,
                    &hdr,
                    v.allowed,
                    hdr.held != 0,
                    v.req.kind(),
                    summarize(v),
                    src.clone(),
                );
            }
            return;
        }
        let mut flow = self.flows.remove(&key).expect("flow");
        Self::release(&self.inj, inject, &mut flow, upto);
        if matches!(outcome, Outcome::Denied) && any_held {
            self.answer_tcp(&flow, inject);
        }
        let src = flow.c.source.clone();
        let held = any_held || flow.hdr.held != 0;
        for v in &verdicts {
            self.record(
                sh,
                bus,
                &flow.hdr,
                v.allowed,
                held,
                v.req.kind(),
                summarize(v),
                src.clone(),
            );
        }
        if let Some(e) = error {
            self.record(
                sh,
                bus,
                &flow.hdr,
                false,
                held,
                flow.stream.kind().name(),
                e,
                src,
            );
        }
    }

    /// Reinject the held bytes below `upto`; a segment straddling it is
    /// split and its tail kept.
    fn release(inj: &Option<OwnedFd>, inject: u32, flow: &mut Flow, upto: u64) {
        let keys: Vec<u64> = flow.segs.range(..upto).map(|(k, _)| *k).collect();
        let ifindex = flow.hdr.ifindex;
        for k in keys {
            let s = flow.segs.remove(&k).expect("seg");
            flow.held_bytes -= s.len();
            let (send, keep) = if s.end() <= upto {
                (s, None)
            } else {
                let cut = (upto - s.off) as usize;
                (s.slice(0, cut), Some(s.slice(cut, s.len())))
            };
            if send.held {
                if let (Some(sock), true) = (inj, inject != 0) {
                    send_inject(
                        sock,
                        inject,
                        inject_mark(ifindex, send.ingress),
                        &send.wire(),
                    );
                }
            }
            if let Some(t) = keep {
                flow.held_bytes += t.len();
                flow.segs.insert(t.off, t);
            }
        }
    }

    fn reinject(&self, inject: u32, ifindex: u32, s: &Seg) {
        if let (Some(sock), true) = (&self.inj, inject != 0) {
            send_inject(sock, inject, inject_mark(ifindex, s.ingress), &s.wire());
        }
    }

    /// Send `f` towards the client (`to_client`) or the server of a flow
    /// whose client → server frames take `ingress` on `ifindex`.
    fn send_answer(
        &self,
        inject: u32,
        ifindex: u32,
        ingress: bool,
        from_vm: bool,
        to_client: bool,
        f: &[u8],
    ) {
        if let (Some(sock), true) = (&self.inj, inject != 0) {
            send_inject(
                sock,
                inject,
                inject_mark(ifindex, ingress != to_client),
                &plain_wire(f),
            );
            return;
        }
        let Some(sock) = &self.raw else { return };
        let towards_vm = from_vm == to_client;
        let out = if towards_vm {
            ifindex
        } else {
            master_of(ifindex)
        };
        send_frame(sock, out, f);
    }

    fn answer_tcp(&self, f: &Flow, inject: u32) {
        let h = &f.hdr;
        let v6 = h.v6 != 0;
        let from_vm = h.from_vm != 0;
        let (dmac, smac) = (&f.l2[0..6], &f.l2[6..12]);
        let client_end = f
            .segs
            .values()
            .map(Seg::end)
            .max()
            .unwrap_or(0)
            .max(f.stream.pos());
        let client_ack = f.seq(client_end);
        let to_client = if f.stream.kind() == Kind::Http && !f.stream.is_h2() {
            tcp_seg(
                h.dport,
                h.sport,
                h.ack,
                client_ack,
                TCP_PSH | TCP_ACK | TCP_FIN,
                FORBIDDEN,
            )
        } else {
            tcp_seg(h.dport, h.sport, h.ack, client_ack, TCP_RST | TCP_ACK, &[])
        };
        let fc = frame(
            smac,
            dmac,
            &h.dst,
            &h.src,
            v6,
            policy::IPPROTO_TCP,
            to_client,
        );
        self.send_answer(inject, h.ifindex, f.ingress, from_vm, true, &fc);
        let server_seq = f.seq(f.stream.allowed());
        let to_server = tcp_seg(h.sport, h.dport, server_seq, 0, TCP_RST, &[]);
        let fs = frame(
            dmac,
            smac,
            &h.src,
            &h.dst,
            v6,
            policy::IPPROTO_TCP,
            to_server,
        );
        self.send_answer(inject, h.ifindex, f.ingress, from_vm, false, &fs);
    }

    #[allow(clippy::too_many_arguments)]
    fn on_udp(
        &mut self,
        sh: &SharedState,
        bus: &broadcast::Sender<StreamEvent>,
        h: &Hdr,
        key: FlowKey,
        seg: Seg,
        compiled: Option<Compiled>,
        inject: u32,
    ) {
        let data = seg.payload();
        let can_inject = inject != 0 && self.inj.is_some() && data.len() >= 4;
        let one_shot = VmL7Flow {
            allow_seq: u32::from_be_bytes([
                *data.first().unwrap_or(&0),
                *data.get(1).unwrap_or(&0),
                *data.get(2).unwrap_or(&0),
                *data.get(3).unwrap_or(&0),
            ]),
            pass_until: data.len() as u32,
            pending_seq: 0,
            state: L7S_OPEN,
            _pad: [0; 3],
        };
        let Some(c) = compiled else {
            if seg.held && can_inject && self.kmap.insert(key, one_shot, 0).is_ok() {
                self.reinject(inject, h.ifindex, &seg);
            }
            return;
        };
        let (allowed, summary) = match dns::parse(data) {
            Some(msg) if !msg.is_response => {
                let req = l7::Request::Dns {
                    name: crate::netpol::fqdn::normalize(&msg.qname),
                };
                let (allowed, note) = c.matcher.check(&req);
                let v = Verdict { req, allowed, note };
                (allowed, summarize(&v))
            }
            _ => (false, "not a DNS query".to_string()),
        };
        let held = seg.held;
        if held {
            let v6 = h.v6 != 0;
            let (dmac, smac) = (&seg.frame[0..6], &seg.frame[6..12]);
            if allowed && can_inject {
                if self.kmap.insert(key, one_shot, 0).is_ok() {
                    self.reinject(inject, h.ifindex, &seg);
                }
            } else if allowed {
                if let Some(sock) = self.raw.clone() {
                    self.forward_dns(h, data.to_vec(), [dmac, smac].concat(), sock);
                }
            } else if let Some(r) = dns_refused(data) {
                let f = frame(
                    smac,
                    dmac,
                    &h.dst,
                    &h.src,
                    v6,
                    policy::IPPROTO_UDP,
                    udp_dgram(h.dport, h.sport, &r),
                );
                self.send_answer(inject, h.ifindex, seg.ingress, h.from_vm != 0, true, &f);
            }
        }
        self.record(sh, bus, h, allowed, held, "dns", summary, c.source.clone());
    }

    /// Fallback without the inject veth: ask the server from the host and
    /// inject its answer towards the client.
    fn forward_dns(&self, h: &Hdr, query: Vec<u8>, l2: Vec<u8>, sock: Arc<OwnedFd>) {
        let v6 = h.v6 != 0;
        let from_vm = h.from_vm != 0;
        let (src, dst, sport, dport, ifindex) = (h.src, h.dst, h.sport, h.dport, h.ifindex);
        std::thread::spawn(move || {
            let server = addr_ip(&dst, v6);
            let bind = if v6 { "[::]:0" } else { "0.0.0.0:0" };
            let Ok(u) = UdpSocket::bind(bind) else { return };
            let _ = u.set_read_timeout(Some(DNS_FORWARD_TIMEOUT));
            if u.send_to(&query, (server, dport)).is_err() {
                return;
            }
            let mut buf = [0u8; 4096];
            let Ok((n, from)) = u.recv_from(&mut buf) else {
                return;
            };
            if from.ip() != server || n < 12 || buf[..2] != query[..2] {
                return;
            }
            let f = frame(
                &l2[6..12],
                &l2[0..6],
                &dst,
                &src,
                v6,
                policy::IPPROTO_UDP,
                udp_dgram(dport, sport, &buf[..n]),
            );
            let out = if from_vm { ifindex } else { master_of(ifindex) };
            send_frame(&sock, out, &f);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        sh: &SharedState,
        bus: &broadcast::Sender<StreamEvent>,
        h: &Hdr,
        allowed: bool,
        held: bool,
        kind: &str,
        summary: String,
        source: Option<String>,
    ) {
        let (rec, alerts) = {
            let mut s = lock(sh);
            let (iface, tap_vm) = s.iface(h.ifindex);
            let idx = &s.vm_flow_index;
            let egress = h.from_vm != 0;
            let subject = idx.name(h.subject).cloned();
            let peer = idx.name(h.peer).cloned();
            let vm = tap_vm
                .or_else(|| subject.as_ref().map(|s| s.0.clone()))
                .unwrap_or_default();
            let (src_side, dst_side) = if egress {
                (subject, peer)
            } else {
                (peer, subject)
            };
            let (src_id, dst_id) = if egress {
                (h.subject, h.peer)
            } else {
                (h.peer, h.subject)
            };
            let rec = VmFlowRecord {
                ts: mono_to_rfc3339(h.ts_ns),
                iface: iface.unwrap_or_default(),
                vm,
                direction: if egress { "egress" } else { "ingress" }.into(),
                src: fmt_addr(&h.src),
                src_port: h.sport,
                dst: fmt_addr(&h.dst),
                dst_port: h.dport,
                src_vm: src_side.as_ref().map(|s| s.0.clone()),
                dst_vm: dst_side.as_ref().map(|s| s.0.clone()),
                src_labels: src_side.map(|s| s.1).unwrap_or_default(),
                dst_labels: dst_side.map(|s| s.1).unwrap_or_default(),
                src_identity: src_id,
                dst_identity: dst_id,
                proto: proto_name(h.proto).into(),
                bytes: h.len,
                verdict: if allowed {
                    "FORWARDED"
                } else if held {
                    "DROPPED"
                } else {
                    "AUDIT"
                }
                .into(),
                drop_reason: (!allowed).then(|| "l7-deny".into()),
                policy: source,
                l7_type: Some(kind.into()),
                l7: Some(summary),
                ..Default::default()
            };
            let alerts = s.record_vm_flow(&rec);
            (rec, alerts)
        };
        publish_vm_flow(bus, &rec, alerts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_mirrors_event() {
        use std::mem::offset_of;
        assert_eq!(std::mem::size_of::<Hdr>(), VM_L7_HDR);
        assert_eq!(
            offset_of!(Hdr, payload_off),
            offset_of!(VmL7Event, payload_off)
        );
        assert_eq!(offset_of!(Hdr, src), offset_of!(VmL7Event, src));
        assert_eq!(offset_of!(Hdr, l4_off), offset_of!(VmL7Event, l4_off));
        assert_eq!(offset_of!(Hdr, ingress), offset_of!(VmL7Event, ingress));
        assert_eq!(VM_L7_HDR, offset_of!(VmL7Event, data));
    }

    fn v4(a: [u8; 4]) -> [u8; ADDR_LEN] {
        let mut x = [0u8; ADDR_LEN];
        x[10] = 0xff;
        x[11] = 0xff;
        x[12..].copy_from_slice(&a);
        x
    }

    #[test]
    fn split_and_wire() {
        let (src, dst) = (v4([10, 0, 0, 1]), v4([10, 0, 0, 2]));
        let f = frame(
            &[1; 6],
            &[2; 6],
            &src,
            &dst,
            false,
            policy::IPPROTO_TCP,
            tcp_seg(
                5555,
                80,
                1000,
                7,
                TCP_PSH | TCP_ACK | TCP_FIN,
                b"GET / HTTP/1.1\r\n\r\nGET /x",
            ),
        );
        let s = Seg {
            off: 0,
            frame: f,
            l3: 14,
            l4: 34,
            pay: 54,
            gso: 0,
            v6: false,
            ingress: true,
            held: true,
        };
        let (a, b) = (s.slice(0, 18), s.slice(18, s.len()));
        assert_eq!(
            (a.payload(), b.payload()),
            (&b"GET / HTTP/1.1\r\n\r\n"[..], &b"GET /x"[..])
        );
        assert_eq!(
            a.frame[34 + 13] & (TCP_FIN | TCP_PSH),
            TCP_PSH,
            "head: no FIN"
        );
        assert_eq!(b.frame[34 + 13] & TCP_FIN, TCP_FIN, "tail keeps FIN");
        assert_eq!(
            u32::from_be_bytes(b.frame[38..42].try_into().unwrap()),
            1018
        );
        assert_eq!(b.off, 18);
        let w = b.wire();
        assert_eq!((w[0], w[1]), (VNET_NEEDS_CSUM, 0));
        let f = &w[VNET_HDR_LEN..];
        assert_eq!(u16::from_be_bytes([f[16], f[17]]) as usize, f.len() - 14);
        assert_eq!(
            csum_fold(csum_add(0, &f[14..34])),
            0,
            "IPv4 header checksum"
        );
        let mut pseudo = vec![10, 0, 0, 1, 10, 0, 0, 2, 0, 6];
        pseudo.extend_from_slice(&((f.len() - 34) as u16).to_be_bytes());
        let partial = u16::from_be_bytes([f[50], f[51]]);
        assert_eq!(partial, !csum_fold(csum_add(0, &pseudo)));
        let mut big = s.slice(0, s.len());
        big.gso = 8;
        assert_eq!(big.wire()[1], VNET_GSO_TCPV4);
    }

    #[test]
    fn checksums_and_refused() {
        let (src, dst) = (v4([10, 0, 0, 1]), v4([10, 0, 0, 2]));
        let f = frame(
            &[1; 6],
            &[2; 6],
            &src,
            &dst,
            false,
            policy::IPPROTO_TCP,
            tcp_seg(80, 5555, 1, 2, TCP_RST, &[]),
        );
        assert_eq!(f.len(), 54);
        assert_eq!(
            csum_fold(csum_add(0, &f[14..34])),
            0,
            "IPv4 header checksum"
        );
        let mut pseudo = vec![10, 0, 0, 1, 10, 0, 0, 2, 0, 6, 0, 20];
        pseudo.extend_from_slice(&f[34..]);
        assert_eq!(csum_fold(csum_add(0, &pseudo)), 0, "TCP checksum");

        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(b"\x03www\x07example\x03com\x00\x00\x01\x00\x01");
        let r = dns_refused(&q).unwrap();
        let m = dns::parse(&r).unwrap();
        assert!(
            m.is_response
                && m.rcode == 5
                && m.id == 0x1234
                && m.qname.trim_end_matches('.') == "www.example.com"
        );
    }

    #[test]
    fn inject_marks() {
        let m = inject_mark(42, true);
        assert_eq!(m & VM_L7_INJECT_MAGIC_MASK, VM_L7_INJECT_MAGIC);
        assert_eq!(m & VM_L7_INJECT_IFINDEX, 42);
        assert_ne!(m & VM_L7_INJECT_INGRESS, 0);
        assert_eq!(inject_mark(42, false) & VM_L7_INJECT_INGRESS, 0);
    }
}

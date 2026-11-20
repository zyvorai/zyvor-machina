// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Opt-in TLS visibility, both rate limited and off unless configured:
//!
//! * `mn_tlsfp` (cgroup_skb egress) samples TLS ClientHellos for JA3/JA4
//!   fingerprinting in userspace.
//! * `mn_ssl_*` uprobes on libssl `SSL_write(_ex)` / `SSL_read(_ex)` copy the
//!   head of the plaintext; userspace keeps only HTTP metadata.

use aya_ebpf::{
    helpers::{
        bpf_get_current_comm, bpf_get_current_pid_tgid, bpf_probe_read_user,
        generated::{bpf_probe_read_user as probe_read_user_raw, bpf_skb_cgroup_id, bpf_skb_load_bytes},
    },
    macros::{cgroup_skb, map, uprobe, uretprobe},
    maps::{Array, HashMap, LruHashMap, RingBuf},
    programs::{ProbeContext, RetProbeContext, SkBuffContext},
};
use machina_bpf_common::*;

use crate::{
    net::now_ns,
    parse::{parse_skb, Tuple, IPPROTO_TCP},
};

const NSEC: u64 = 1_000_000_000;

#[map]
pub static TLSFP_CFG: Array<SampleCfg> = Array::with_max_entries(1, 0);
#[map]
pub static TLSFP_BUCKET: Array<SampleBucket> = Array::with_max_entries(1, 0);
#[map]
pub static TLSFP_EVENTS: RingBuf = RingBuf::with_byte_size(1 << 20, 0);

#[map]
pub static SSL_CFG: Array<SampleCfg> = Array::with_max_entries(1, 0);
#[map]
pub static SSL_BUCKET: Array<SampleBucket> = Array::with_max_entries(1, 0);
/// Process names (`comm`, NUL padded) allowed to be captured.
#[map]
pub static SSL_COMMS: HashMap<[u8; 16], u8> = HashMap::with_max_entries(64, 0);
#[map]
pub static SSL_READ_ARGS: LruHashMap<u64, SslReadArgs> = LruHashMap::with_max_entries(16384, 0);
#[map]
pub static SSL_EVENTS: RingBuf = RingBuf::with_byte_size(512 * 1024, 0);

/// One sample from a host-wide bucket of `rate`/s (burst: one second).
#[inline(always)]
pub fn take(bucket: &Array<SampleBucket>, rate: u32) -> bool {
    let Some(b) = bucket.get_ptr_mut(0) else { return false };
    let b = unsafe { &mut *b };
    let now = now_ns();
    let cap = (rate as u64).max(1) * 1000;
    let elapsed = now.saturating_sub(b.last_ns).min(NSEC);
    let tokens = (b.tokens + elapsed * rate as u64 / 1_000_000).min(cap);
    b.last_ns = now;
    if tokens < 1000 {
        b.tokens = tokens;
        return false;
    }
    b.tokens = tokens - 1000;
    true
}

// ---- ClientHello sampler ------------------------------------------------------

#[cgroup_skb]
pub fn mn_tlsfp(ctx: SkBuffContext) -> i32 {
    let Some(cfg) = TLSFP_CFG.get(0) else { return 1 };
    if cfg.enabled == 0 {
        return 1;
    }
    let rate = cfg.rate;
    let mut t = Tuple::zero();
    if parse_skb(&ctx, &mut t) == 0 || t.proto != IPPROTO_TCP || t.payload_off == 0 {
        return 1;
    }
    // TLS handshake record carrying a ClientHello.
    match ctx.load::<[u8; 6]>(t.payload_off) {
        Ok(h) if h[0] == 0x16 && h[1] == 3 && h[5] == 1 => {}
        _ => return 1,
    }
    if take(&TLSFP_BUCKET, rate) {
        tlsfp_emit(&ctx, &t);
    }
    1
}

#[inline(never)]
fn tlsfp_emit(ctx: &SkBuffContext, t: &Tuple) {
    let l3_end = if t.v6 {
        match ctx.load::<[u8; 2]>(4) {
            Ok(b) => 40 + u16::from_be_bytes(b) as usize,
            Err(_) => return,
        }
    } else {
        match ctx.load::<[u8; 2]>(2) {
            Ok(b) => u16::from_be_bytes(b) as usize,
            Err(_) => return,
        }
    };
    let len = ctx.len() as usize;
    // GSO skbs carry the whole message; the IP length field may then be 0.
    let end = if l3_end > t.payload_off && l3_end < len { l3_end } else { len };
    if end <= t.payload_off {
        return;
    }
    let total = end - t.payload_off;
    let n = if total > TLSFP_LEN { TLSFP_LEN } else { total };
    let n = ((n - 1) & (TLSFP_LEN - 1)) + 1;
    let Some(mut e) = TLSFP_EVENTS.reserve::<TlsFpEvent>(0) else { return };
    let ev = e.as_mut_ptr();
    unsafe {
        (*ev).ts_ns = now_ns();
        (*ev).cgroup = bpf_skb_cgroup_id(ctx.skb.skb);
        (*ev).len = total as u32;
        (*ev).sport = t.sport;
        (*ev).dport = t.dport;
        (*ev).v6 = t.v6 as u8;
        (*ev)._pad = [0; 3];
        (*ev).src = t.src;
        (*ev).dst = t.dst;
        if bpf_skb_load_bytes(ctx.skb.skb.cast(), t.payload_off as u32, (*ev).data.as_mut_ptr().cast(), n as u32) != 0 {
            e.discard(0);
            return;
        }
        (*ev).cap_len = n as u32;
    }
    e.submit(0);
}

// ---- OpenSSL plaintext heads ------------------------------------------------------

#[inline(always)]
fn ssl_wanted() -> Option<(u32, [u8; 16])> {
    let cfg = SSL_CFG.get(0)?;
    if cfg.enabled == 0 {
        return None;
    }
    let comm = bpf_get_current_comm().ok()?;
    if cfg.all == 0 && unsafe { SSL_COMMS.get(&comm) }.is_none() {
        return None;
    }
    Some((cfg.rate, comm))
}

#[inline(never)]
fn ssl_emit(dir: u32, buf: u64, len: u64, rate: u32, comm: &[u8; 16]) {
    if buf == 0 || len == 0 || !take(&SSL_BUCKET, rate) {
        return;
    }
    let n = if len > SSL_DATA_LEN as u64 { SSL_DATA_LEN } else { len as usize };
    let n = ((n - 1) & (SSL_DATA_LEN - 1)) + 1;
    let Some(mut e) = SSL_EVENTS.reserve::<SslEvent>(0) else { return };
    let ev = e.as_mut_ptr();
    unsafe {
        (*ev).ts_ns = now_ns();
        (*ev).pid_tgid = bpf_get_current_pid_tgid();
        (*ev).len = len.min(u32::MAX as u64) as u32;
        (*ev).dir = dir;
        (*ev)._pad = 0;
        (*ev).comm = *comm;
        if probe_read_user_raw((*ev).data.as_mut_ptr().cast(), n as u32, buf as *const _) != 0 {
            e.discard(0);
            return;
        }
        (*ev).cap_len = n as u32;
    }
    e.submit(0);
}

/// `SSL_write(ssl, buf, num)` and `SSL_write_ex(ssl, buf, num, *written)`.
#[uprobe]
pub fn mn_ssl_write(ctx: ProbeContext) -> u32 {
    let Some((rate, comm)) = ssl_wanted() else { return 0 };
    let buf: u64 = ctx.arg(1).unwrap_or(0);
    let num: u64 = ctx.arg::<u64>(2).unwrap_or(0) & 0x7fff_ffff;
    ssl_emit(SSL_DIR_WRITE, buf, num, rate, &comm);
    0
}

#[inline(always)]
fn save_read(buf: u64, readbytes: u64) {
    if ssl_wanted().is_none() {
        return;
    }
    let _ = SSL_READ_ARGS.insert(&bpf_get_current_pid_tgid(), &SslReadArgs { buf, readbytes }, 0);
}

/// `SSL_read(ssl, buf, num)`.
#[uprobe]
pub fn mn_ssl_read_enter(ctx: ProbeContext) -> u32 {
    save_read(ctx.arg(1).unwrap_or(0), 0);
    0
}

/// `SSL_read_ex(ssl, buf, num, *readbytes)`.
#[uprobe]
pub fn mn_ssl_read_ex_enter(ctx: ProbeContext) -> u32 {
    save_read(ctx.arg(1).unwrap_or(0), ctx.arg(3).unwrap_or(0));
    0
}

#[uretprobe]
pub fn mn_ssl_read_ret(ctx: RetProbeContext) -> u32 {
    let id = bpf_get_current_pid_tgid();
    let Some(args) = (unsafe { SSL_READ_ARGS.get(&id) }).copied() else { return 0 };
    let _ = SSL_READ_ARGS.remove(&id);
    let ret: i64 = ctx.ret::<i64>() as i32 as i64;
    let len = if args.readbytes != 0 {
        if ret != 1 {
            return 0;
        }
        unsafe { bpf_probe_read_user(args.readbytes as *const u64) }.unwrap_or(0)
    } else if ret > 0 {
        ret as u64
    } else {
        return 0;
    };
    let Some((rate, comm)) = ssl_wanted() else { return 0 };
    ssl_emit(SSL_DIR_READ, args.buf, len, rate, &comm);
    0
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! In-memory pcapng writer (SHB + one Ethernet IDB + EPBs, microsecond timestamps).

const LINKTYPE_ETHERNET: u16 = 1;

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

fn push_u16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

pub struct PcapngWriter {
    buf: Vec<u8>,
    packets: u64,
    bytes: u64,
}

impl PcapngWriter {
    pub fn new(if_name: &str, snaplen: u32) -> Self {
        let mut buf = Vec::with_capacity(64 * 1024);
        // Section Header Block.
        push_u32(&mut buf, 0x0a0d_0d0a);
        push_u32(&mut buf, 28);
        push_u32(&mut buf, 0x1a2b_3c4d);
        push_u16(&mut buf, 1);
        push_u16(&mut buf, 0);
        buf.extend_from_slice(&(-1i64).to_le_bytes());
        push_u32(&mut buf, 28);

        // Interface Description Block with if_name (option 2) + opt_endofopt.
        let name = &if_name.as_bytes()[..if_name.len().min(255)];
        let opts_len = 4 + pad4(name.len()) + 4;
        let total = (20 + opts_len) as u32;
        push_u32(&mut buf, 0x0000_0001);
        push_u32(&mut buf, total);
        push_u16(&mut buf, LINKTYPE_ETHERNET);
        push_u16(&mut buf, 0);
        push_u32(&mut buf, snaplen);
        push_u16(&mut buf, 2);
        push_u16(&mut buf, name.len() as u16);
        buf.extend_from_slice(name);
        buf.resize(buf.len() + pad4(name.len()) - name.len(), 0);
        push_u16(&mut buf, 0);
        push_u16(&mut buf, 0);
        push_u32(&mut buf, total);
        Self {
            buf,
            packets: 0,
            bytes: 0,
        }
    }

    /// Append an Enhanced Packet Block. `ts_us` is microseconds since the epoch.
    pub fn push(&mut self, ts_us: u64, orig_len: u32, data: &[u8]) {
        let cap = data.len();
        let total = (32 + pad4(cap)) as u32;
        push_u32(&mut self.buf, 0x0000_0006);
        push_u32(&mut self.buf, total);
        push_u32(&mut self.buf, 0);
        push_u32(&mut self.buf, (ts_us >> 32) as u32);
        push_u32(&mut self.buf, ts_us as u32);
        push_u32(&mut self.buf, cap as u32);
        push_u32(&mut self.buf, orig_len);
        self.buf.extend_from_slice(data);
        self.buf.resize(self.buf.len() + pad4(cap) - cap, 0);
        push_u32(&mut self.buf, total);
        self.packets += 1;
        self.bytes += orig_len as u64;
    }

    pub fn packets(&self) -> u64 {
        self.packets
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(b: &[u8], off: usize) -> u32 {
        u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
    }

    #[test]
    fn blocks_are_well_formed() {
        let mut w = PcapngWriter::new("vnet0", 1024);
        w.push(1_700_000_000_000_000, 60, &[0xab; 42]);
        w.push(1_700_000_000_000_001, 1500, &[0xcd; 64]);
        let b = w.as_bytes();
        // Walk blocks: each starts with type + total length and ends with the same length.
        let mut off = 0;
        let mut types = vec![];
        while off < b.len() {
            let ty = u32_at(b, off);
            let len = u32_at(b, off + 4) as usize;
            assert_eq!(len % 4, 0);
            assert_eq!(u32_at(b, off + len - 4) as usize, len);
            types.push(ty);
            off += len;
        }
        assert_eq!(off, b.len());
        assert_eq!(types, vec![0x0a0d_0d0a, 1, 6, 6]);
        assert_eq!(w.packets(), 2);
        assert_eq!(w.bytes(), 1560);
    }
}

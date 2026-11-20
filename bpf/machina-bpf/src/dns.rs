// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Minimal DNS message decoder for payloads sampled by the TC programs.

use std::net::{Ipv4Addr, Ipv6Addr};

use crate::api::DnsAnswer;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DnsMessage {
    pub id: u16,
    pub is_response: bool,
    pub rcode: u8,
    pub qname: String,
    pub qtype: u16,
    pub answers: Vec<DnsAnswer>,
}

pub fn rtype_name(t: u16) -> String {
    match t {
        1 => "A".into(),
        2 => "NS".into(),
        5 => "CNAME".into(),
        6 => "SOA".into(),
        12 => "PTR".into(),
        15 => "MX".into(),
        16 => "TXT".into(),
        28 => "AAAA".into(),
        33 => "SRV".into(),
        65 => "HTTPS".into(),
        255 => "ANY".into(),
        n => format!("TYPE{n}"),
    }
}

pub fn rcode_name(r: u8) -> &'static str {
    match r {
        0 => "NOERROR",
        1 => "FORMERR",
        2 => "SERVFAIL",
        3 => "NXDOMAIN",
        4 => "NOTIMP",
        5 => "REFUSED",
        _ => "OTHER",
    }
}

fn read_u16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(off)?, *b.get(off + 1)?]))
}

fn read_u32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(off)?,
        *b.get(off + 1)?,
        *b.get(off + 2)?,
        *b.get(off + 3)?,
    ]))
}

/// Decode a (possibly compressed) name at `off`; returns the name and the
/// offset just past it in the original stream.
fn read_name(b: &[u8], mut off: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut end: Option<usize> = None;
    let mut jumps = 0;
    loop {
        let len = *b.get(off)? as usize;
        if len == 0 {
            off += 1;
            break;
        }
        if len & 0xc0 == 0xc0 {
            let ptr = (read_u16(b, off)? & 0x3fff) as usize;
            if end.is_none() {
                end = Some(off + 2);
            }
            jumps += 1;
            if jumps > 16 {
                return None;
            }
            off = ptr;
            continue;
        }
        let label = b.get(off + 1..off + 1 + len)?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        off += 1 + len;
        if labels.len() > 127 {
            return None;
        }
    }
    Some((labels.join("."), end.unwrap_or(off)))
}

pub fn parse(b: &[u8]) -> Option<DnsMessage> {
    if b.len() < 12 {
        return None;
    }
    let id = read_u16(b, 0)?;
    let flags = read_u16(b, 2)?;
    let qd = read_u16(b, 4)? as usize;
    let an = read_u16(b, 6)? as usize;
    let mut msg = DnsMessage {
        id,
        is_response: flags & 0x8000 != 0,
        rcode: (flags & 0x000f) as u8,
        ..Default::default()
    };
    let mut off = 12;
    for i in 0..qd.min(4) {
        let (name, next) = read_name(b, off)?;
        let qtype = read_u16(b, next)?;
        off = next + 4;
        if i == 0 {
            msg.qname = name.to_ascii_lowercase();
            msg.qtype = qtype;
        }
    }
    for _ in 0..an.min(32) {
        let Some((name, next)) = read_name(b, off) else {
            break;
        };
        let (Some(rtype), Some(ttl), Some(rdlen)) = (
            read_u16(b, next),
            read_u32(b, next + 4),
            read_u16(b, next + 8),
        ) else {
            break;
        };
        let rd_off = next + 10;
        let Some(rdata) = b.get(rd_off..rd_off + rdlen as usize) else {
            break;
        };
        let data = match (rtype, rdata.len()) {
            (1, 4) => Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3]).to_string(),
            (28, 16) => {
                let mut a = [0u8; 16];
                a.copy_from_slice(rdata);
                Ipv6Addr::from(a).to_string()
            }
            (5 | 2 | 12, _) => read_name(b, rd_off).map(|(n, _)| n).unwrap_or_default(),
            _ => String::new(),
        };
        msg.answers.push(DnsAnswer {
            name: name.to_ascii_lowercase(),
            rtype: rtype_name(rtype),
            ttl,
            data,
        });
        off = rd_off + rdlen as usize;
    }
    Some(msg)
}

#[cfg(test)]
pub(crate) fn build_response(qname: &str, ips: &[[u8; 4]]) -> Vec<u8> {
    let mut b = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, ips.len() as u8, 0, 0, 0, 0];
    for l in qname.split('.') {
        b.push(l.len() as u8);
        b.extend_from_slice(l.as_bytes());
    }
    b.extend_from_slice(&[0, 0, 1, 0, 1]);
    for ip in ips {
        b.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0x0e, 0x10, 0, 4]);
        b.extend_from_slice(ip);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compressed_response() {
        let b = build_response("www.Example.com", &[[93, 184, 216, 34], [1, 2, 3, 4]]);
        let m = parse(&b).unwrap();
        assert!(m.is_response);
        assert_eq!(m.id, 0x1234);
        assert_eq!(m.qname, "www.example.com");
        assert_eq!(rtype_name(m.qtype), "A");
        assert_eq!(m.answers.len(), 2);
        assert_eq!(m.answers[0].data, "93.184.216.34");
        assert_eq!(m.answers[0].ttl, 3600);
        assert_eq!(rcode_name(m.rcode), "NOERROR");
    }

    #[test]
    fn rejects_garbage_and_loops() {
        assert!(parse(&[1, 2, 3]).is_none());
        // Self-referencing compression pointer.
        let b = [0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0xc0, 12, 0, 1, 0, 1];
        assert!(parse(&b).is_none());
    }
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Verb-only decoding of sampled plaintext L7 payloads. Only the operation
//! (command, statement keyword, API name, gRPC method path) is extracted —
//! never keys, values, bind parameters or literals.

use machina_bpf_common::{L7S_HTTP2, L7S_KAFKA, L7S_MYSQL, L7S_POSTGRES, L7S_REDIS};

pub fn proto_name(id: u8) -> &'static str {
    match id {
        L7S_REDIS => "redis",
        L7S_POSTGRES => "postgres",
        L7S_MYSQL => "mysql",
        L7S_KAFKA => "kafka",
        L7S_HTTP2 => "http2",
        _ => "unknown",
    }
}

pub fn proto_id(name: &str) -> Option<u8> {
    Some(match name.to_ascii_lowercase().as_str() {
        "redis" => L7S_REDIS,
        "postgres" | "postgresql" | "pg" => L7S_POSTGRES,
        "mysql" | "mariadb" => L7S_MYSQL,
        "kafka" => L7S_KAFKA,
        "http2" | "grpc" | "h2" => L7S_HTTP2,
        _ => return None,
    })
}

/// Default service ports sampled when none are configured.
pub const DEFAULT_PORTS: &[(u16, &str)] = &[
    (6379, "redis"),
    (5432, "postgres"),
    (3306, "mysql"),
    (9092, "kafka"),
    (50051, "http2"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub op: String,
    /// gRPC method path (HTTP/2 only).
    pub detail: Option<String>,
}

fn op(s: impl Into<String>) -> Option<Decoded> {
    Some(Decoded {
        op: s.into(),
        detail: None,
    })
}

/// Leading SQL keyword, uppercased ("SELECT", "INSERT", ...).
fn sql_verb(b: &[u8]) -> Option<String> {
    let s = b
        .iter()
        .position(|c| !c.is_ascii_whitespace() && *c != b'(')?;
    let w: String = b[s..]
        .iter()
        .take_while(|c| c.is_ascii_alphabetic())
        .take(16)
        .map(|c| c.to_ascii_uppercase() as char)
        .collect();
    (!w.is_empty()).then_some(w)
}

fn word(b: &[u8]) -> Option<String> {
    let w: String = b
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == b'.')
        .take(24)
        .map(|c| c.to_ascii_uppercase() as char)
        .collect();
    (!w.is_empty()).then_some(w)
}

pub fn decode(proto: u8, to_server: bool, d: &[u8]) -> Option<Decoded> {
    match proto {
        L7S_REDIS => redis(to_server, d),
        L7S_POSTGRES => postgres(to_server, d),
        L7S_MYSQL => mysql(to_server, d),
        L7S_KAFKA => kafka(to_server, d),
        L7S_HTTP2 => http2(d),
        _ => None,
    }
}

fn redis(to_server: bool, d: &[u8]) -> Option<Decoded> {
    if !to_server {
        return match d.first()? {
            b'-' => op("ERROR"),
            b'+' | b':' | b'$' | b'*' | b'%' | b'_' | b'#' | b',' => op("REPLY"),
            _ => None,
        };
    }
    if d.first()? == &b'*' {
        // *N\r\n$len\r\nCMD\r\n
        let mut it = d.split(|c| *c == b'\n');
        it.next()?;
        if !it.next()?.starts_with(b"$") {
            return None;
        }
        return word(it.next()?).map(|op| Decoded { op, detail: None });
    }
    let w = word(d)?;
    w.chars()
        .all(|c| c.is_ascii_alphabetic())
        .then_some(Decoded {
            op: w,
            detail: None,
        })
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn postgres(to_server: bool, d: &[u8]) -> Option<Decoded> {
    let t = *d.first()?;
    if to_server {
        if !t.is_ascii_uppercase() {
            return match be32(d, 4)? {
                80877103 => op("SSL_REQUEST"),
                80877104 => op("GSSENC_REQUEST"),
                196608 => op("STARTUP"),
                _ => None,
            };
        }
        let body = d.get(5..)?;
        return match t {
            b'Q' => op(sql_verb(body)?),
            b'P' => {
                let q = &body[body.iter().position(|c| *c == 0)? + 1..];
                op(format!("PARSE {}", sql_verb(q)?))
            }
            b'B' => op("BIND"),
            b'E' => op("EXECUTE"),
            b'D' => op("DESCRIBE"),
            b'S' => op("SYNC"),
            b'X' => op("TERMINATE"),
            b'p' => op("AUTH"),
            _ => None,
        };
    }
    match t {
        b'E' => op("ERROR"),
        b'C' => op(word(d.get(5..)?)?),
        b'T' | b'D' => op("ROWS"),
        b'1' => op("PARSE_COMPLETE"),
        b'R' => op("AUTH"),
        b'Z' => op("READY"),
        _ => None,
    }
}

fn mysql(to_server: bool, d: &[u8]) -> Option<Decoded> {
    let len = u32::from_le_bytes([*d.first()?, *d.get(1)?, *d.get(2)?, 0]);
    if len == 0 {
        return None;
    }
    let cmd = *d.get(4)?;
    if !to_server {
        return match cmd {
            0x00 => op("OK"),
            0xff => op("ERROR"),
            0xfe => op("EOF"),
            0x0a if d.get(3) == Some(&0) => op("HANDSHAKE"),
            _ => op("ROWS"),
        };
    }
    match cmd {
        0x01 => op("QUIT"),
        0x02 => op("INIT_DB"),
        0x03 => op(sql_verb(d.get(5..)?)?),
        0x0e => op("PING"),
        0x16 => op(format!("PREPARE {}", sql_verb(d.get(5..)?)?)),
        0x17 => op("STMT_EXECUTE"),
        0x19 => op("STMT_CLOSE"),
        0x1f => op("RESET_CONNECTION"),
        _ if d.get(3) == Some(&1) => op("LOGIN"),
        _ => None,
    }
}

const KAFKA_APIS: &[&str] = &[
    "Produce",
    "Fetch",
    "ListOffsets",
    "Metadata",
    "LeaderAndIsr",
    "StopReplica",
    "UpdateMetadata",
    "ControlledShutdown",
    "OffsetCommit",
    "OffsetFetch",
    "FindCoordinator",
    "JoinGroup",
    "Heartbeat",
    "LeaveGroup",
    "SyncGroup",
    "DescribeGroups",
    "ListGroups",
    "SaslHandshake",
    "ApiVersions",
    "CreateTopics",
    "DeleteTopics",
    "DeleteRecords",
    "InitProducerId",
    "OffsetForLeaderEpoch",
    "AddPartitionsToTxn",
    "AddOffsetsToTxn",
    "EndTxn",
    "WriteTxnMarkers",
    "TxnOffsetCommit",
    "DescribeAcls",
    "CreateAcls",
    "DeleteAcls",
    "DescribeConfigs",
    "AlterConfigs",
    "AlterReplicaLogDirs",
    "DescribeLogDirs",
    "SaslAuthenticate",
    "CreatePartitions",
];

fn kafka(to_server: bool, d: &[u8]) -> Option<Decoded> {
    if !to_server || d.len() < 8 {
        return None;
    }
    let key = i16::from_be_bytes([d[4], d[5]]);
    let ver = i16::from_be_bytes([d[6], d[7]]);
    if !(0..=20).contains(&ver) {
        return None;
    }
    match KAFKA_APIS.get(usize::try_from(key).ok()?) {
        Some(n) => op(*n),
        None if key < 100 => op(format!("Api{key}")),
        None => None,
    }
}

const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

fn http2(d: &[u8]) -> Option<Decoded> {
    let mut d = d;
    let mut preface = false;
    if d.starts_with(&H2_PREFACE[..14]) {
        preface = true;
        d = d.get(H2_PREFACE.len()..).unwrap_or_default();
    }
    // Walk frames looking for HEADERS (type 1).
    let mut off = 0;
    for _ in 0..8 {
        let Some(h) = d.get(off..off + 9) else { break };
        let len = u32::from_be_bytes([0, h[0], h[1], h[2]]) as usize;
        let (ty, flags) = (h[3], h[4]);
        if ty > 9 {
            break;
        }
        if ty == 1 {
            let end = (off + 9 + len).min(d.len());
            let mut block = &d[off + 9..end];
            if flags & 0x08 != 0 {
                let pad = *block.first()? as usize;
                block = block.get(1..block.len().saturating_sub(pad))?;
            }
            if flags & 0x20 != 0 {
                block = block.get(5..)?;
            }
            return Some(Decoded {
                op: "HEADERS".into(),
                detail: hpack_path(block),
            });
        }
        off += 9 + len;
    }
    preface.then(|| Decoded {
        op: "PREFACE".into(),
        detail: None,
    })
}

/// `:path` from a literal header field with indexed name 4 (`:path`) and a
/// raw (non-Huffman) value — what gRPC clients send on a fresh connection
/// when they don't Huffman-encode.
fn hpack_path(b: &[u8]) -> Option<String> {
    let mut i = 0;
    while i + 2 < b.len() {
        let c = b[i];
        if matches!(c, 0x44 | 0x04 | 0x14) && b[i + 1] & 0x80 == 0 {
            let n = (b[i + 1] & 0x7f) as usize;
            let v = b.get(i + 2..i + 2 + n)?;
            if v.first() == Some(&b'/') && v.iter().all(|c| c.is_ascii_graphic()) {
                let s = String::from_utf8_lossy(v);
                return Some(s.split('?').next().unwrap_or_default().to_string());
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(proto: u8, to_server: bool, b: &[u8]) -> Option<String> {
        decode(proto, to_server, b).map(|x| x.op)
    }

    #[test]
    fn redis_commands() {
        assert_eq!(
            d(
                L7S_REDIS,
                true,
                b"*3\r\n$3\r\nset\r\n$3\r\nkey\r\n$5\r\nvalue\r\n"
            )
            .as_deref(),
            Some("SET")
        );
        assert_eq!(d(L7S_REDIS, true, b"PING\r\n").as_deref(), Some("PING"));
        assert_eq!(
            d(L7S_REDIS, false, b"-ERR wrong\r\n").as_deref(),
            Some("ERROR")
        );
        assert_eq!(d(L7S_REDIS, false, b"+OK\r\n").as_deref(), Some("REPLY"));
    }

    #[test]
    fn postgres_messages() {
        let mut q = vec![b'Q', 0, 0, 0, 30];
        q.extend_from_slice(b"  select * from t where a='x'\0");
        assert_eq!(d(L7S_POSTGRES, true, &q).as_deref(), Some("SELECT"));
        let mut p = vec![b'P', 0, 0, 0, 20, b's', b'1', 0];
        p.extend_from_slice(b"INSERT INTO t VALUES($1)\0");
        assert_eq!(d(L7S_POSTGRES, true, &p).as_deref(), Some("PARSE INSERT"));
        assert_eq!(
            d(L7S_POSTGRES, true, &[0, 0, 0, 8, 4, 0xd2, 0x16, 0x2f]).as_deref(),
            Some("SSL_REQUEST")
        );
        let mut c = vec![b'C', 0, 0, 0, 13];
        c.extend_from_slice(b"UPDATE 3\0");
        assert_eq!(d(L7S_POSTGRES, false, &c).as_deref(), Some("UPDATE"));
    }

    #[test]
    fn mysql_commands() {
        let mut q = vec![15, 0, 0, 0, 0x03];
        q.extend_from_slice(b"delete from t");
        assert_eq!(d(L7S_MYSQL, true, &q).as_deref(), Some("DELETE"));
        assert_eq!(
            d(L7S_MYSQL, true, &[1, 0, 0, 0, 0x0e]).as_deref(),
            Some("PING")
        );
        assert_eq!(
            d(L7S_MYSQL, false, &[7, 0, 0, 1, 0xff, 0x15, 0x04]).as_deref(),
            Some("ERROR")
        );
    }

    #[test]
    fn kafka_api() {
        assert_eq!(
            d(L7S_KAFKA, true, &[0, 0, 0, 20, 0, 3, 0, 9]).as_deref(),
            Some("Metadata")
        );
        assert_eq!(
            d(L7S_KAFKA, true, &[0, 0, 0, 20, 0, 18, 0, 3]).as_deref(),
            Some("ApiVersions")
        );
        assert_eq!(d(L7S_KAFKA, false, &[0, 0, 0, 20, 0, 18, 0, 3]), None);
    }

    #[test]
    fn grpc_path() {
        let path = b"/pkg.Svc/Method";
        let mut block = vec![0x83, 0x86, 0x44, path.len() as u8];
        block.extend_from_slice(path);
        let mut f = H2_PREFACE.to_vec();
        f.extend_from_slice(&[0, 0, 0, 4, 0, 0, 0, 0, 0]); // empty SETTINGS
        f.extend_from_slice(&[0, 0, block.len() as u8, 1, 0x04, 0, 0, 0, 1]);
        f.extend_from_slice(&block);
        let x = decode(L7S_HTTP2, true, &f).unwrap();
        assert_eq!(x.op, "HEADERS");
        assert_eq!(x.detail.as_deref(), Some("/pkg.Svc/Method"));
        assert_eq!(d(L7S_HTTP2, true, H2_PREFACE).as_deref(), Some("PREFACE"));
    }

    #[test]
    fn names_roundtrip() {
        for (_, n) in DEFAULT_PORTS {
            assert_eq!(proto_name(proto_id(n).unwrap()), *n);
        }
    }
}

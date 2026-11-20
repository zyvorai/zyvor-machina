// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Policy `match` strings → datapath rule entries. Pure Rust, no kernel access.

use std::net::IpAddr;

use machina_bpf_common::{fnv1a64, v4_mapped, ADDR_LEN, V4_MAPPED_PREFIX_BITS};

pub const IPPROTO_TCP: u8 = 6;
pub const IPPROTO_UDP: u8 = 17;

/// An address prefix in the datapath's 16-byte (IPv4-mapped) form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Prefix {
    pub addr: [u8; ADDR_LEN],
    /// Bits of `addr` that are significant (0..=128).
    pub bits: u32,
}

impl Prefix {
    pub fn to_display(&self) -> String {
        if self.bits >= V4_MAPPED_PREFIX_BITS && self.addr[..12] == v4_mapped([0; 4])[..12] {
            let a = &self.addr[12..];
            let b = self.bits - V4_MAPPED_PREFIX_BITS;
            if b == 32 {
                format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
            } else {
                format!("{}.{}.{}.{}/{b}", a[0], a[1], a[2], a[3])
            }
        } else {
            let ip = std::net::Ipv6Addr::from(self.addr);
            if self.bits == 128 {
                ip.to_string()
            } else {
                format!("{ip}/{}", self.bits)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Rule {
    DenyCidr(Prefix),
    /// `proto` 0 = any, `port` 0 = any.
    Allow {
        proto: u8,
        port: u16,
        prefix: Prefix,
    },
    DenyPort {
        proto: u8,
        port: u16,
    },
    ExecDeny {
        path: String,
        hash: u64,
    },
    FileDeny {
        prefix: String,
    },
    CapDeny {
        cap: u32,
    },
    /// Resolved answers for names under this suffix are added to the deny trie.
    DnsDeny {
        suffix: String,
    },
    /// New workload-initiated connections per second (token bucket per interface).
    ConnRate {
        per_sec: u32,
        burst: u32,
    },
}

pub const MAX_CONN_RATE: u32 = 1_000_000;

/// `"100/s"`, `"600/m"`, `"50/s burst 200"`, `"50/s,200"`.
fn parse_rate(s: &str) -> Result<Rule, String> {
    let toks: Vec<&str> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .collect();
    let first: &str = toks
        .first()
        .copied()
        .ok_or("rate_limit requires a rate such as `100/s`")?;
    let (n_s, unit) = first.split_once('/').unwrap_or((first, "s"));
    let n: u32 = n_s
        .parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("invalid rate `{first}`"))?;
    let per_sec = match unit.to_ascii_lowercase().as_str() {
        "s" | "sec" | "second" => n,
        "m" | "min" | "minute" => n.div_ceil(60),
        other => return Err(format!("unsupported rate unit `{other}` (s or m)")),
    };
    if per_sec > MAX_CONN_RATE {
        return Err(format!("rate above {MAX_CONN_RATE}/s"));
    }
    let burst_tok = match toks.get(1).copied() {
        Some("burst") => toks.get(2).copied(),
        Some(t) => Some(t.trim_start_matches("burst=")),
        None => None,
    };
    let burst = match burst_tok {
        Some(b) => b
            .parse::<u32>()
            .ok()
            .filter(|b| *b > 0 && *b <= MAX_CONN_RATE)
            .ok_or_else(|| format!("invalid burst `{b}`"))?,
        None => per_sec,
    };
    Ok(Rule::ConnRate { per_sec, burst })
}

fn mask(mut addr: [u8; ADDR_LEN], bits: u32) -> [u8; ADDR_LEN] {
    for (i, b) in addr.iter_mut().enumerate() {
        let start = (i as u32) * 8;
        if start >= bits {
            *b = 0;
        } else if start + 8 > bits {
            let keep = bits - start;
            *b &= 0xffu8 << (8 - keep);
        }
    }
    addr
}

pub fn ip_to_addr(ip: IpAddr) -> [u8; ADDR_LEN] {
    match ip {
        IpAddr::V4(v4) => v4_mapped(v4.octets()),
        IpAddr::V6(v6) => v6.octets(),
    }
}

/// `"10.0.0.0/8"`, `"203.0.113.5"`, `"2001:db8::/32"` → [`Prefix`].
pub fn parse_prefix(s: &str) -> Result<Prefix, String> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    if s.is_empty() {
        return Err("empty address".into());
    }
    let (ip_s, len_s) = match s.split_once('/') {
        Some((a, l)) => (a, Some(l)),
        None => (s, None),
    };
    let ip: IpAddr = ip_s
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .map_err(|_| format!("invalid IP address `{ip_s}`"))?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    let len = match len_s {
        Some(l) => l
            .parse::<u32>()
            .ok()
            .filter(|n| *n <= max)
            .ok_or_else(|| format!("invalid prefix length `/{l}`"))?,
        None => max,
    };
    let bits = if ip.is_ipv4() {
        V4_MAPPED_PREFIX_BITS + len
    } else {
        len
    };
    Ok(Prefix {
        addr: mask(ip_to_addr(ip), bits),
        bits,
    })
}

pub fn parse_proto(s: &str) -> Result<u8, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "tcp" => Ok(IPPROTO_TCP),
        "udp" => Ok(IPPROTO_UDP),
        "any" | "*" | "all" => Ok(0),
        other => Err(format!("unsupported protocol `{other}` (tcp, udp or any)")),
    }
}

pub fn proto_name(p: u8) -> &'static str {
    match p {
        IPPROTO_TCP => "tcp",
        IPPROTO_UDP => "udp",
        1 => "icmp",
        58 => "icmpv6",
        0 => "any",
        _ => "other",
    }
}

/// `"ip[/len]:port/proto"` (IPv6 in brackets). The address is mandatory: an
/// allow rule must never silently widen into match-any.
fn parse_allow(s: &str) -> Result<Rule, String> {
    let s = s.trim();
    // A trailing "/tcp", "/udp", "/any" or bare "/" is the protocol; any other
    // '/' belongs to a CIDR ("10.0.0.0/8:443/tcp").
    let (host_port, proto) = match s.rsplit_once('/') {
        Some((hp, tail)) if tail.chars().all(|c| c.is_ascii_alphabetic() || c == '*') => (hp, tail),
        _ => (s, ""),
    };
    let proto = parse_proto(proto)?;
    let (ip_s, port_s) = if let Some(rest) = host_port.strip_prefix('[') {
        let (ip, tail) = rest
            .split_once(']')
            .ok_or_else(|| format!("unterminated IPv6 address in `{s}`"))?;
        (ip.to_string(), tail.trim_start_matches(':').to_string())
    } else {
        let (ip, port) = host_port
            .rsplit_once(':')
            .ok_or_else(|| format!("expected `ip:port/proto`, got `{s}`"))?;
        (ip.to_string(), port.to_string())
    };
    if ip_s.trim().is_empty() {
        return Err(
            "allow rules require an explicit destination IP, e.g. \"203.0.113.5:443/tcp\"".into(),
        );
    }
    let port: u16 = port_s
        .parse()
        .map_err(|_| format!("invalid port `{port_s}`"))?;
    let prefix = parse_prefix(&ip_s)?;
    Ok(Rule::Allow {
        proto,
        port,
        prefix,
    })
}

fn parse_deny_port(s: &str) -> Result<Rule, String> {
    let (port_s, proto_s) = s.trim().split_once('/').unwrap_or((s.trim(), "any"));
    let port: u16 = port_s
        .parse()
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| format!("invalid port `{port_s}`"))?;
    Ok(Rule::DenyPort {
        proto: parse_proto(proto_s)?,
        port,
    })
}

pub const CAPABILITIES: &[&str] = &[
    "CAP_CHOWN",
    "CAP_DAC_OVERRIDE",
    "CAP_DAC_READ_SEARCH",
    "CAP_FOWNER",
    "CAP_FSETID",
    "CAP_KILL",
    "CAP_SETGID",
    "CAP_SETUID",
    "CAP_SETPCAP",
    "CAP_LINUX_IMMUTABLE",
    "CAP_NET_BIND_SERVICE",
    "CAP_NET_BROADCAST",
    "CAP_NET_ADMIN",
    "CAP_NET_RAW",
    "CAP_IPC_LOCK",
    "CAP_IPC_OWNER",
    "CAP_SYS_MODULE",
    "CAP_SYS_RAWIO",
    "CAP_SYS_CHROOT",
    "CAP_SYS_PTRACE",
    "CAP_SYS_PACCT",
    "CAP_SYS_ADMIN",
    "CAP_SYS_BOOT",
    "CAP_SYS_NICE",
    "CAP_SYS_RESOURCE",
    "CAP_SYS_TIME",
    "CAP_SYS_TTY_CONFIG",
    "CAP_MKNOD",
    "CAP_LEASE",
    "CAP_AUDIT_WRITE",
    "CAP_AUDIT_CONTROL",
    "CAP_SETFCAP",
    "CAP_MAC_OVERRIDE",
    "CAP_MAC_ADMIN",
    "CAP_SYSLOG",
    "CAP_WAKE_ALARM",
    "CAP_BLOCK_SUSPEND",
    "CAP_AUDIT_READ",
    "CAP_PERFMON",
    "CAP_BPF",
    "CAP_CHECKPOINT_RESTORE",
];

pub fn parse_cap(s: &str) -> Result<u32, String> {
    let t = s.trim();
    if let Ok(n) = t.parse::<u32>() {
        if (n as usize) < CAPABILITIES.len() {
            return Ok(n);
        }
    }
    let up = t.to_ascii_uppercase();
    let name = if up.starts_with("CAP_") {
        up
    } else {
        format!("CAP_{up}")
    };
    CAPABILITIES
        .iter()
        .position(|c| *c == name)
        .map(|i| i as u32)
        .ok_or_else(|| format!("unknown capability `{t}`"))
}

pub fn cap_name(cap: u32) -> String {
    CAPABILITIES
        .get(cap as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("CAP_{cap}"))
}

/// Compile a policy (`kind`, `match`) into datapath rules.
pub fn compile(kind: &str, match_value: &str) -> Result<Vec<Rule>, String> {
    let m = match_value.trim();
    if m.is_empty() {
        return Err(format!("{kind} requires a match value"));
    }
    match kind {
        "deny_ip" => m
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|s| parse_prefix(s).map(Rule::DenyCidr))
            .collect(),
        "tc_allow" | "allow_port" => m
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(parse_allow)
            .collect(),
        "deny_port" => Ok(vec![parse_deny_port(m)?]),
        "deny_process" => {
            if !m.starts_with('/') {
                return Err("deny_process requires an absolute executable path".into());
            }
            if m.len() >= machina_bpf_common::PATH_LEN {
                return Err("executable path too long".into());
            }
            Ok(vec![Rule::ExecDeny {
                path: m.to_string(),
                hash: fnv1a64(m.as_bytes()),
            }])
        }
        "deny_file" => {
            let p = m.trim_end_matches('*');
            if !p.starts_with('/') {
                return Err("deny_file requires an absolute path or path prefix".into());
            }
            if p.len() > machina_bpf_common::FILE_WATCH_PREFIX_LEN {
                return Err(format!(
                    "deny_file prefix longer than {} bytes",
                    machina_bpf_common::FILE_WATCH_PREFIX_LEN
                ));
            }
            Ok(vec![Rule::FileDeny {
                prefix: p.to_string(),
            }])
        }
        "deny_cap" => Ok(vec![Rule::CapDeny { cap: parse_cap(m)? }]),
        "deny_dns" => {
            let suffix = m
                .trim_start_matches("*.")
                .trim_start_matches('.')
                .trim_end_matches('.');
            if suffix.is_empty() || suffix.contains(' ') {
                return Err("deny_dns requires a domain such as `*.example.com`".into());
            }
            Ok(vec![Rule::DnsDeny {
                suffix: suffix.to_ascii_lowercase(),
            }])
        }
        "rate_limit" => Ok(vec![parse_rate(m)?]),
        other => Err(format!(
            "policy kind `{other}` is not supported by the native datapath (supported: {})",
            crate::api::POLICY_KINDS.join(", ")
        )),
    }
}

/// True when `name` equals `suffix` or is a subdomain of it.
pub fn dns_suffix_match(name: &str, suffix: &str) -> bool {
    let n = name.trim_end_matches('.').to_ascii_lowercase();
    n == suffix || n.ends_with(&format!(".{suffix}"))
}

/// Shell-style glob with `*` only (interface patterns).
pub fn glob_match(pattern: &str, s: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == s,
        Some((pre, rest)) => {
            if !s.starts_with(pre) {
                return false;
            }
            let tail = &s[pre.len()..];
            if rest.is_empty() {
                return true;
            }
            (0..=tail.len()).any(|i| tail.is_char_boundary(i) && glob_match(rest, &tail[i..]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes() {
        let p = parse_prefix("10.1.2.3/8").unwrap();
        assert_eq!(p.bits, 104);
        assert_eq!(&p.addr[12..], &[10, 0, 0, 0]);
        assert_eq!(p.to_display(), "10.0.0.0/8");
        assert_eq!(
            parse_prefix("203.0.113.5").unwrap().to_display(),
            "203.0.113.5"
        );
        let v6 = parse_prefix("2001:db8::1/32").unwrap();
        assert_eq!(v6.bits, 32);
        assert_eq!(v6.to_display(), "2001:db8::/32");
        assert!(parse_prefix("10.0.0.0/33").is_err());
        assert!(parse_prefix("nope").is_err());
    }

    #[test]
    fn allow_rules_need_ip() {
        assert!(compile("tc_allow", "443/tcp").is_err());
        assert!(compile("tc_allow", ":443/tcp").is_err());
        assert_eq!(
            compile("tc_allow", "8.8.8.8:53/udp").unwrap(),
            vec![Rule::Allow {
                proto: IPPROTO_UDP,
                port: 53,
                prefix: parse_prefix("8.8.8.8").unwrap()
            }]
        );
        match &compile("allow_port", "10.0.0.0/8:443/tcp").unwrap()[0] {
            Rule::Allow {
                proto,
                port,
                prefix,
            } => {
                assert_eq!((*proto, *port), (IPPROTO_TCP, 443));
                assert_eq!(prefix.to_display(), "10.0.0.0/8");
            }
            r => panic!("{r:?}"),
        }
        match &compile("tc_allow", "[2001:db8::1]:443/tcp").unwrap()[0] {
            Rule::Allow { port, prefix, .. } => {
                assert_eq!(*port, 443);
                assert_eq!(prefix.bits, 128);
            }
            r => panic!("{r:?}"),
        }
        match &compile("tc_allow", "203.0.113.5:443/").unwrap()[0] {
            Rule::Allow { proto, .. } => assert_eq!(*proto, IPPROTO_TCP),
            r => panic!("{r:?}"),
        }
    }

    #[test]
    fn other_kinds() {
        assert_eq!(
            compile("deny_port", "4444/tcp").unwrap(),
            vec![Rule::DenyPort {
                proto: IPPROTO_TCP,
                port: 4444
            }]
        );
        assert_eq!(
            compile("deny_port", "53").unwrap(),
            vec![Rule::DenyPort { proto: 0, port: 53 }]
        );
        assert!(compile("deny_process", "nc").is_err());
        assert!(matches!(
            &compile("deny_process", "/usr/bin/nc").unwrap()[0],
            Rule::ExecDeny { .. }
        ));
        assert_eq!(
            compile("deny_file", "/etc/shadow*").unwrap(),
            vec![Rule::FileDeny {
                prefix: "/etc/shadow".into()
            }]
        );
        assert_eq!(
            compile("deny_cap", "CAP_NET_RAW").unwrap(),
            vec![Rule::CapDeny { cap: 13 }]
        );
        assert_eq!(
            compile("deny_cap", "sys_admin").unwrap(),
            vec![Rule::CapDeny { cap: 21 }]
        );
        assert_eq!(
            compile("deny_dns", "*.XYZ").unwrap(),
            vec![Rule::DnsDeny {
                suffix: "xyz".into()
            }]
        );
        assert!(compile("deny_namespace", "kube-system").is_err());
        assert_eq!(compile("deny_ip", "1.2.3.4, 10.0.0.0/8").unwrap().len(), 2);
    }

    #[test]
    fn rate_limits() {
        assert_eq!(
            compile("rate_limit", "100/s").unwrap(),
            vec![Rule::ConnRate {
                per_sec: 100,
                burst: 100
            }]
        );
        assert_eq!(
            compile("rate_limit", "90/m").unwrap(),
            vec![Rule::ConnRate {
                per_sec: 2,
                burst: 2
            }]
        );
        assert_eq!(
            compile("rate_limit", "50/s burst 200").unwrap(),
            vec![Rule::ConnRate {
                per_sec: 50,
                burst: 200
            }]
        );
        assert_eq!(
            compile("rate_limit", "50/s,burst=10").unwrap(),
            vec![Rule::ConnRate {
                per_sec: 50,
                burst: 10
            }]
        );
        assert_eq!(
            compile("rate_limit", "20").unwrap(),
            vec![Rule::ConnRate {
                per_sec: 20,
                burst: 20
            }]
        );
        assert!(compile("rate_limit", "0/s").is_err());
        assert!(compile("rate_limit", "5/h").is_err());
        assert!(compile("rate_limit", "5000000/s").is_err());
    }

    #[test]
    fn helpers() {
        assert!(dns_suffix_match("a.b.evil.com.", "evil.com"));
        assert!(dns_suffix_match("evil.com", "evil.com"));
        assert!(!dns_suffix_match("notevil.com", "evil.com"));
        assert!(glob_match("vnet*", "vnet12"));
        assert!(glob_match("tap*", "tap"));
        assert!(!glob_match("vnet*", "eth0"));
        assert!(glob_match("*veth*x", "aveth1x"));
        assert_eq!(cap_name(21), "CAP_SYS_ADMIN");
    }
}

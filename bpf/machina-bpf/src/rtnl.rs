// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! rtnetlink message naming for the network change audit.

/// RTM types per object kind (new, del[, set]).
pub const KINDS: &[(&str, &[u16])] = &[
    ("link", &[16, 17, 19]),
    ("addr", &[20, 21]),
    ("route", &[24, 25]),
    ("neigh", &[28, 29]),
    ("rule", &[32, 33]),
    ("qdisc", &[36, 37]),
    ("class", &[40, 41]),
    ("filter", &[44, 45]),
];

const DEFAULT_KINDS: &[&str] = &["link", "addr", "route", "neigh", "rule", "qdisc", "filter"];

pub const NLM_F_CREATE: u16 = 0x400;

/// Kernel type mask for the configured kinds (empty = defaults).
pub fn kinds_mask(kinds: &[String]) -> Result<u64, String> {
    let want: Vec<&str> = if kinds.is_empty() {
        DEFAULT_KINDS.to_vec()
    } else {
        kinds.iter().map(String::as_str).collect()
    };
    let mut types = Vec::new();
    for k in want {
        let (_, t) = KINDS
            .iter()
            .find(|(n, _)| *n == k)
            .ok_or_else(|| format!("unknown rtnetlink kind {k:?}"))?;
        types.extend_from_slice(t);
    }
    Ok(machina_bpf_common::rtnl_bits(&types))
}

/// (kind, action) of an RTM type.
pub fn name(t: u16) -> (&'static str, &'static str) {
    let kind = KINDS
        .iter()
        .find(|(_, ts)| ts.contains(&t) || (t >= ts[0] && t < ts[0] + 4))
        .map(|(n, _)| *n)
        .unwrap_or("other");
    let action = match t.checked_sub(16).map(|x| x % 4) {
        Some(0) => "new",
        Some(1) => "del",
        Some(3) => "set",
        _ => "other",
    };
    (kind, action)
}

/// Route destination as a CIDR (`default` for a zero-length prefix).
pub fn route_dst(family: u8, dst: &[u8; 16], dst_len: u8) -> Option<String> {
    const AF_INET: u8 = 2;
    const AF_INET6: u8 = 10;
    if dst_len == 0 && matches!(family, AF_INET | AF_INET6) {
        return Some("default".into());
    }
    match family {
        AF_INET => Some(format!(
            "{}/{dst_len}",
            std::net::Ipv4Addr::new(dst[0], dst[1], dst[2], dst[3])
        )),
        AF_INET6 => Some(format!("{}/{dst_len}", std::net::Ipv6Addr::from(*dst))),
        _ => None,
    }
}

/// Inode of a `net:[4026531840]` namespace link.
pub fn ns_inode(link: &str) -> Option<u64> {
    link.strip_prefix("net:[")?.strip_suffix(']')?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(name(16), ("link", "new"));
        assert_eq!(name(17), ("link", "del"));
        assert_eq!(name(19), ("link", "set"));
        assert_eq!(name(25), ("route", "del"));
        assert_eq!(name(44), ("filter", "new"));
    }

    #[test]
    fn masks() {
        assert_eq!(kinds_mask(&["link".into()]).unwrap(), 0b1011);
        assert!(kinds_mask(&["bogus".into()]).is_err());
        assert_eq!(
            kinds_mask(&[]).unwrap(),
            machina_bpf_common::RTNL_DEFAULT_MASK
        );
    }

    #[test]
    fn dst() {
        let mut d = [0u8; 16];
        d[..4].copy_from_slice(&[10, 1, 0, 0]);
        assert_eq!(route_dst(2, &d, 16).as_deref(), Some("10.1.0.0/16"));
        assert_eq!(route_dst(2, &d, 0).as_deref(), Some("default"));
        assert_eq!(ns_inode("net:[4026531840]"), Some(4026531840));
    }
}

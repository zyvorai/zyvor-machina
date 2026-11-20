// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Hubble-style flow filters over [`VmFlowRecord`]s, shared by the daemon
//! and controller flow APIs.

use serde::{Deserialize, Serialize};

use crate::api::VmFlowRecord;
use crate::policy::parse_prefix;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowFilter {
    /// Either side, or the tap owner.
    #[serde(default)]
    pub vm: Option<String>,
    #[serde(default)]
    pub from_vm: Option<String>,
    #[serde(default)]
    pub to_vm: Option<String>,
    /// `key=value` on either side's labels.
    #[serde(default)]
    pub label: Option<String>,
    /// Either address.
    #[serde(default)]
    pub ip: Option<String>,
    /// Either address within this prefix.
    #[serde(default)]
    pub cidr: Option<String>,
    /// Either port.
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub protocol: Option<String>,
    /// Comma list: FORWARDED, DROPPED, AUDIT.
    #[serde(default)]
    pub verdict: Option<String>,
    #[serde(default)]
    pub drop_reason: Option<String>,
    /// Substring of the matched policy/rule.
    #[serde(default)]
    pub policy: Option<String>,
    /// `ingress` or `egress`.
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
}

fn in_prefix(addr: &str, cidr: &str) -> bool {
    let (Ok(a), Ok(p)) = (parse_prefix(addr), parse_prefix(cidr)) else {
        return false;
    };
    if a.bits < p.bits {
        return false;
    }
    let mut m = a.addr;
    for (i, b) in m.iter_mut().enumerate() {
        let start = i as u32 * 8;
        if start >= p.bits {
            *b = 0;
        } else if start + 8 > p.bits {
            *b &= 0xffu8 << (8 - (p.bits - start));
        }
    }
    m == p.addr
}

impl FlowFilter {
    pub fn matches(&self, f: &VmFlowRecord) -> bool {
        let eq = |want: &Option<String>, have: Option<&str>| {
            want.as_deref().is_none_or(|w| have == Some(w))
        };
        if let Some(v) = self.vm.as_deref() {
            if f.vm != v && f.src_vm.as_deref() != Some(v) && f.dst_vm.as_deref() != Some(v) {
                return false;
            }
        }
        if !eq(&self.from_vm, f.src_vm.as_deref()) || !eq(&self.to_vm, f.dst_vm.as_deref()) {
            return false;
        }
        if let Some(l) = self.label.as_deref() {
            let (k, v) = l.split_once('=').unwrap_or((l, ""));
            let has = |m: &std::collections::BTreeMap<String, String>| {
                m.get(k).is_some_and(|x| v.is_empty() || x == v)
            };
            if !has(&f.src_labels) && !has(&f.dst_labels) {
                return false;
            }
        }
        if let Some(ip) = self.ip.as_deref() {
            if f.src != ip && f.dst != ip {
                return false;
            }
        }
        if let Some(c) = self.cidr.as_deref() {
            if !in_prefix(&f.src, c) && !in_prefix(&f.dst, c) {
                return false;
            }
        }
        if let Some(p) = self.port {
            if f.src_port != p && f.dst_port != p {
                return false;
            }
        }
        if let Some(p) = self.protocol.as_deref() {
            if !f.proto.eq_ignore_ascii_case(p) {
                return false;
            }
        }
        if let Some(v) = self.verdict.as_deref().filter(|v| !v.is_empty()) {
            if !v
                .split(',')
                .any(|x| x.trim().eq_ignore_ascii_case(&f.verdict))
            {
                return false;
            }
        }
        if let Some(r) = self.drop_reason.as_deref() {
            if f.drop_reason.as_deref() != Some(r) {
                return false;
            }
        }
        if let Some(p) = self.policy.as_deref() {
            if !f.policy.as_deref().is_some_and(|x| x.contains(p)) {
                return false;
            }
        }
        if let Some(d) = self.direction.as_deref() {
            if !f.direction.eq_ignore_ascii_case(d) {
                return false;
            }
        }
        if let Some(h) = self.host.as_deref() {
            if f.host.as_deref() != Some(h) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters() {
        let mut f = VmFlowRecord {
            vm: "web-1".into(),
            src: "10.0.0.5".into(),
            dst: "203.0.113.9".into(),
            dst_port: 443,
            proto: "tcp".into(),
            verdict: "DROPPED".into(),
            direction: "egress".into(),
            src_vm: Some("web-1".into()),
            policy: Some("egress spec.egress[0]".into()),
            ..Default::default()
        };
        f.src_labels.insert("app".into(), "web".into());
        let ok = |x: FlowFilter| x.matches(&f);
        assert!(ok(FlowFilter {
            vm: Some("web-1".into()),
            ..Default::default()
        }));
        assert!(ok(FlowFilter {
            label: Some("app=web".into()),
            ..Default::default()
        }));
        assert!(ok(FlowFilter {
            cidr: Some("203.0.113.0/24".into()),
            ..Default::default()
        }));
        assert!(ok(FlowFilter {
            verdict: Some("audit,dropped".into()),
            ..Default::default()
        }));
        assert!(ok(FlowFilter {
            policy: Some("egress".into()),
            port: Some(443),
            ..Default::default()
        }));
        assert!(!ok(FlowFilter {
            verdict: Some("FORWARDED".into()),
            ..Default::default()
        }));
        assert!(!ok(FlowFilter {
            to_vm: Some("db".into()),
            ..Default::default()
        }));
        assert!(!ok(FlowFilter {
            cidr: Some("10.9.0.0/16".into()),
            ..Default::default()
        }));
    }
}

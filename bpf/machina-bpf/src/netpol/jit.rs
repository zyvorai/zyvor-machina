// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Just-in-time access: an ordinary policy that lets one VM (or the host)
//! reach a port on another VM until `machina.io/expires-at`.
//!
//! Every spec sets `enableDefaultDeny: false`, so a grant only adds an
//! allow and never isolates a VM that was open before. Expired policies are
//! left out of compilation and deleted by the owner's reaper.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{VmNetworkPolicy, KIND, LABEL_VM_NAME};

pub const LABEL_JIT: &str = "machina.io/jit";
pub const ANNOTATION_EXPIRES: &str = "machina.io/expires-at";
pub const ANNOTATION_REASON: &str = "machina.io/reason";
pub const ANNOTATION_GRANTED_BY: &str = "machina.io/granted-by";
pub const JIT_MAX_SECS: u64 = 86_400;
pub const JIT_DEFAULT_SECS: u64 = 3600;

/// `from` is a VM name or `host`; `port` 0 means every port.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JitRequest {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub secs: Option<u64>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JitGrant {
    pub name: String,
    pub from: String,
    pub to: String,
    pub port: u16,
    pub protocol: String,
    pub expires_at: String,
    pub remaining_secs: u64,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub granted_by: String,
}

const ANNOTATION_FROM: &str = "machina.io/jit-from";
const ANNOTATION_TO: &str = "machina.io/jit-to";
const ANNOTATION_PORT: &str = "machina.io/jit-port";

fn slug(s: &str) -> String {
    let mut out: String = s
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    out.truncate(60);
    out.trim_matches('-').to_string()
}

fn base36(mut n: u64) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

impl JitRequest {
    pub fn protocol(&self) -> String {
        match self.protocol.to_ascii_uppercase().as_str() {
            "" => "TCP".into(),
            p => p.into(),
        }
    }

    pub fn secs(&self) -> u64 {
        self.secs.unwrap_or(JIT_DEFAULT_SECS)
    }

    /// `from → to:port/PROTO` (or `from → to` for every port).
    pub fn what(&self) -> String {
        if self.port == 0 {
            format!("{} → {}", self.from, self.to)
        } else {
            format!(
                "{} → {}:{}/{}",
                self.from,
                self.to,
                self.port,
                self.protocol()
            )
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.to.trim().is_empty() || self.from.trim().is_empty() {
            return Err("`from` and `to` are required".into());
        }
        if self.to == "host" {
            return Err("`to` must be a VM".into());
        }
        if self.from == self.to {
            return Err("`from` and `to` are the same VM".into());
        }
        if !matches!(self.protocol().as_str(), "TCP" | "UDP" | "SCTP" | "ANY") {
            return Err(format!(
                "protocol `{}`: use TCP, UDP, SCTP or ANY",
                self.protocol
            ));
        }
        let secs = self.secs();
        if secs == 0 || secs > JIT_MAX_SECS {
            return Err(format!("duration must be 1 to {JIT_MAX_SECS} seconds"));
        }
        Ok(())
    }

    /// The policy granting this request until `now + secs`.
    pub fn policy(&self, by: &str, now: DateTime<Utc>) -> Result<VmNetworkPolicy, String> {
        self.validate()?;
        let until = now + chrono::Duration::seconds(self.secs() as i64);
        let proto = self.protocol();
        let ports = if self.port == 0 {
            None
        } else {
            Some(json!([{ "ports": [{ "port": self.port.to_string(), "protocol": proto }] }]))
        };
        let peer = |name: &str| json!({ "matchLabels": { LABEL_VM_NAME: name } });
        let mut ingress = if self.from == "host" {
            json!({ "fromEntities": ["host"] })
        } else {
            json!({ "fromEndpoints": [peer(&self.from)] })
        };
        if let Some(p) = &ports {
            ingress["toPorts"] = p.clone();
        }
        let mut specs = vec![json!({
            "description": format!(
                "Temporary access {} until {}",
                self.what(),
                until.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            ),
            "endpointSelector": peer(&self.to),
            "enableDefaultDeny": { "ingress": false },
            "ingress": [ingress],
        })];
        if self.from != "host" {
            let mut egress = json!({ "toEndpoints": [peer(&self.to)] });
            if let Some(p) = &ports {
                egress["toPorts"] = p.clone();
            }
            specs.push(json!({
                "endpointSelector": peer(&self.from),
                "enableDefaultDeny": { "egress": false },
                "egress": [egress],
            }));
        }
        let name = format!(
            "jit-{}-to-{}-{}-{}",
            slug(&self.from),
            slug(&self.to),
            self.port,
            base36(now.timestamp_millis().max(0) as u64)
        );
        let mut annotations = BTreeMap::from([
            (
                ANNOTATION_EXPIRES.to_string(),
                until.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ),
            (ANNOTATION_GRANTED_BY.to_string(), by.to_string()),
            (ANNOTATION_FROM.to_string(), self.from.clone()),
            (ANNOTATION_TO.to_string(), self.to.clone()),
            (
                ANNOTATION_PORT.to_string(),
                format!("{}/{proto}", self.port),
            ),
        ]);
        if !self.reason.trim().is_empty() {
            annotations.insert(ANNOTATION_REASON.into(), self.reason.trim().to_string());
        }
        Ok(VmNetworkPolicy {
            name,
            kind: KIND.into(),
            labels: BTreeMap::from([(LABEL_JIT.to_string(), "true".to_string())]),
            annotations,
            specs,
        })
    }
}

/// `90s`, `15m`, `1h30m`.
pub fn duration_label(secs: u64) -> String {
    match (secs / 3600, secs % 3600 / 60, secs % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, 0) => format!("{m}m"),
        (h, 0, 0) => format!("{h}h"),
        (h, m, _) if h > 0 => format!("{h}h{m}m"),
        (_, m, s) => format!("{m}m{s}s"),
    }
}

/// `machina.io/expires-at` of any policy (JIT or hand-written).
pub fn expires_at(p: &VmNetworkPolicy) -> Option<DateTime<Utc>> {
    p.annotations
        .get(ANNOTATION_EXPIRES)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
}

pub fn expired(p: &VmNetworkPolicy, now: DateTime<Utc>) -> bool {
    expires_at(p).is_some_and(|t| t <= now)
}

/// Active grants (policies labelled `machina.io/jit` that have not expired).
pub fn grants(policies: &[VmNetworkPolicy], now: DateTime<Utc>) -> Vec<JitGrant> {
    policies
        .iter()
        .filter(|p| p.labels.get(LABEL_JIT).map(String::as_str) == Some("true"))
        .filter_map(|p| {
            let until = expires_at(p)?;
            if until <= now {
                return None;
            }
            let a = |k: &str| p.annotations.get(k).cloned().unwrap_or_default();
            let port_proto = a(ANNOTATION_PORT);
            let (port, proto) = port_proto.split_once('/').unwrap_or(("0", "TCP"));
            Some(JitGrant {
                name: p.name.clone(),
                from: a(ANNOTATION_FROM),
                to: a(ANNOTATION_TO),
                port: port.parse().unwrap_or(0),
                protocol: proto.to_string(),
                expires_at: a(ANNOTATION_EXPIRES),
                remaining_secs: (until - now).num_seconds().max(0) as u64,
                reason: a(ANNOTATION_REASON),
                granted_by: a(ANNOTATION_GRANTED_BY),
            })
        })
        .collect()
}

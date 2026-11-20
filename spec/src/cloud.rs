// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Cloud primitives with no host side effects. IPv4 first; reject unsupported
//! address families rather than silently provisioning a different network.
use std::net::Ipv4Addr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloudCidr {
    pub network: u32,
    pub prefix: u8,
}

impl FromStr for CloudCidr {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (ip, prefix) = value.split_once('/').ok_or("CIDR prefix required")?;
        let ip: Ipv4Addr = ip.parse().map_err(|_| "IPv4 CIDR required")?;
        // Canonical digits only: `u8::from_str` would accept "+24" and "024", and the CIDR string is stored and
        // compared as text elsewhere, so one network must have exactly one spelling.
        if prefix.is_empty()
            || !prefix.bytes().all(|b| b.is_ascii_digit())
            || (prefix.len() > 1 && prefix.starts_with('0'))
        {
            return Err("invalid prefix".into());
        }
        let prefix: u8 = prefix.parse().map_err(|_| "invalid prefix")?;
        if prefix > 32 {
            return Err("prefix must be 0..32".into());
        }
        let network = u32::from(ip);
        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        };
        if network & mask != network {
            return Err("CIDR must use its network address".into());
        }
        Ok(Self { network, prefix })
    }
}

impl CloudCidr {
    pub fn last(self) -> u32 {
        self.network | u32::MAX.checked_shr(u32::from(self.prefix)).unwrap_or(0)
    }
    pub fn contains(self, other: Self) -> bool {
        self.network <= other.network && self.last() >= other.last()
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.network <= other.last() && other.network <= self.last()
    }
    pub fn address(self, offset: u32) -> Result<String, String> {
        let ip = self.network.checked_add(offset).ok_or("address overflow")?;
        if ip > self.last() {
            return Err("offset outside subnet".into());
        }
        Ok(Ipv4Addr::from(ip).to_string())
    }
    pub fn validate_private(self, subnet: bool) -> Result<(), String> {
        let private = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"]
            .iter()
            .any(|v| v.parse::<Self>().unwrap().contains(self));
        if !private || !(if subnet { 16..=28 } else { 8..=24 }).contains(&self.prefix) {
            return Err("use RFC1918 CIDR (/8../24 VPC, /16../28 subnet)".into());
        }
        Ok(())
    }
    /// Lower half is managed IPAM, upper half DHCP. Never allocate DHCP's range.
    pub fn ipam_end_offset(self) -> u32 {
        // Saturating: a /32 has no host range (callers validate the prefix first, but this is public).
        (self.last() - self.network).div_ceil(2).saturating_sub(1)
    }
}

/// Deterministic names are owned by a subnet UUID, never a user's display name.
pub fn cloud_network_xml(id: &str, cidr: &str) -> Result<String, String> {
    if id.len() != 36
        || !id.chars().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                // Lowercase only: libvirt lower-cases UUIDs, so an uppercase id would never match its own network again.
                c.is_ascii_digit() || ('a'..='f').contains(&c)
            }
        })
    {
        return Err("invalid subnet UUID (lowercase hex expected)".into());
    }
    let c: CloudCidr = cidr.parse()?;
    c.validate_private(true)?;
    let compact = id.replace('-', "");
    let gateway = c.address(1)?;
    let start = c.address(c.ipam_end_offset() + 1)?;
    let end = c.address(c.last() - c.network - 1)?;
    Ok(format!("<network><name>mc-{id}</name><uuid>{id}</uuid><bridge name='mc{}' stp='on' delay='0'/><ip address='{gateway}' prefix='{}'><dhcp><range start='{start}' end='{end}'/></dhcp></ip></network>", &compact[..12], c.prefix))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScalingPolicy {
    pub min: u32,
    pub max: u32,
    pub desired: u32,
    pub target_cpu: Option<f64>,
    pub cooldown_secs: u32,
    // Optional fields are skipped when unset: the reconciler compares stored
    // policy JSON byte for byte, so older rows must serialize unchanged.
    /// Scale up ahead of the daily/weekly pattern in the group's CPU history.
    #[serde(default, skip_serializing_if = "is_false")]
    pub predictive: bool,
    #[serde(default, skip_serializing_if = "ScaleIn::is_stop")]
    pub scale_in: ScaleIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_balancer: Option<LbBinding>,
    /// Seconds a member stays out of the load balancer before it is stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drain_secs: Option<u32>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// What happens to an instance the group no longer needs.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScaleIn {
    /// Shut down; disks are kept.
    #[default]
    Stop,
    /// Managed-save (scale to zero): no RAM held, the first packet wakes it.
    Sleep,
}

impl ScaleIn {
    fn is_stop(&self) -> bool {
        *self == ScaleIn::Stop
    }
}

/// Members join this load balancer on `port` while they are running.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LbBinding {
    pub id: uuid::Uuid,
    pub port: u16,
}

pub const DEFAULT_DRAIN_SECS: u32 = 30;
impl ScalingPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.min > self.desired || self.desired > self.max || self.max > 100 {
            return Err("require 0 <= min <= desired <= max <= 100".into());
        }
        if self
            .target_cpu
            .is_some_and(|v| !v.is_finite() || !(10.0..=90.0).contains(&v))
        {
            return Err("target CPU must be 10..90 percent".into());
        }
        if !(30..=86400).contains(&self.cooldown_secs) {
            return Err("cooldown must be 30..86400 seconds".into());
        }
        if self.drain_secs.is_some_and(|d| d > 900) {
            return Err("drain must be 0..900 seconds".into());
        }
        if self.load_balancer.is_some_and(|l| l.port == 0) {
            return Err("load balancer port must be 1..65535".into());
        }
        if self.predictive && self.target_cpu.is_none() {
            return Err("predictive scaling needs a target CPU".into());
        }
        Ok(())
    }

    pub fn drain(&self) -> u32 {
        if self.load_balancer.is_none() {
            return 0;
        }
        self.drain_secs.unwrap_or(DEFAULT_DRAIN_SECS)
    }

    /// Instances needed to keep `demand` (summed CPU percent across members)
    /// at the target, within bounds.
    pub fn needed_for(&self, demand: f64) -> Option<u32> {
        let target = self.target_cpu?;
        if !demand.is_finite() || demand < 0.0 {
            return None;
        }
        let n = (demand / target).ceil() as u32;
        Some(n.clamp(self.min.max(1), self.max))
    }

    /// Reactive step, then raised (never lowered) to what the forecast peak
    /// needs. The forecast only scales up, so a wrong one can't remove capacity.
    pub fn next_desired_with_forecast(
        &self,
        cpu: Option<f64>,
        elapsed_secs: i64,
        forecast_peak: Option<f64>,
    ) -> u32 {
        let reactive = self.next_desired(cpu, elapsed_secs);
        if !self.predictive {
            return reactive;
        }
        match forecast_peak.and_then(|d| self.needed_for(d)) {
            Some(n) if n > reactive => n,
            _ => reactive,
        }
    }
    /// Missing/stale metrics must never trigger scale-in. One step per cooldown;
    /// 10-point hysteresis prevents small oscillations around the target.
    pub fn next_desired(&self, cpu: Option<f64>, elapsed_secs: i64) -> u32 {
        if elapsed_secs < i64::from(self.cooldown_secs) {
            return self.desired;
        }
        match (
            self.target_cpu,
            cpu.filter(|v| v.is_finite() && (0.0..=100.0).contains(v)),
        ) {
            (Some(target), Some(cpu)) if cpu > target + 10.0 => {
                self.desired.saturating_add(1).min(self.max)
            }
            (Some(target), Some(cpu)) if cpu < target - 10.0 => {
                self.desired.saturating_sub(1).max(self.min)
            }
            _ => self.desired,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cidr_boundaries_and_overlap() {
        let parent: CloudCidr = "10.4.0.0/16".parse().unwrap();
        assert!(parent.contains("10.4.255.0/24".parse().unwrap()));
        assert!(!parent.overlaps("10.5.0.0/16".parse().unwrap()));
        assert!(parent.overlaps("10.0.0.0/8".parse().unwrap()));
        assert!("10.4.0.1/24".parse::<CloudCidr>().is_err());
        for cidr in ["::/0", "10.0.0.0/33", "x", "10.0.0.0/-1"] {
            assert!(cidr.parse::<CloudCidr>().is_err());
        }
        assert_eq!("0.0.0.0/0".parse::<CloudCidr>().unwrap().last(), u32::MAX);
        assert!("8.8.8.0/24"
            .parse::<CloudCidr>()
            .unwrap()
            .validate_private(true)
            .is_err());
    }
    #[test]
    fn only_canonical_spellings_parse_and_tiny_prefixes_do_not_panic() {
        for bad in [
            "10.0.0.0/+24",
            "10.0.0.0/024",
            "10.0.0.0/ 24",
            "10.0.0.0/",
            "10.0.0.0/2 4",
            "010.0.0.0/24",
        ] {
            assert!(bad.parse::<CloudCidr>().is_err(), "{bad} must be rejected");
        }
        assert!("10.0.0.0/24".parse::<CloudCidr>().is_ok());
        assert!("0.0.0.0/0".parse::<CloudCidr>().is_ok());
        // /31 and /32 are not usable subnets, but computing their range must not underflow.
        for c in ["10.0.0.0/32", "10.0.0.0/31"] {
            let c: CloudCidr = c.parse().unwrap();
            let _ = c.ipam_end_offset();
            assert!(c.validate_private(true).is_err());
        }
    }
    #[test]
    fn subnet_uuids_must_be_lowercase_hex() {
        assert!(cloud_network_xml("12345678-1234-1234-1234-123456789abc", "10.2.3.0/24").is_ok());
        assert!(cloud_network_xml("12345678-1234-1234-1234-123456789ABC", "10.2.3.0/24").is_err());
        assert!(cloud_network_xml("12345678-1234-1234-1234-123456789abg", "10.2.3.0/24").is_err());
    }
    #[test]
    fn isolated_xml_has_disjoint_dhcp_and_ipam_ranges() {
        let xml = cloud_network_xml("12345678-1234-1234-1234-123456789abc", "10.2.3.0/24").unwrap();
        assert!(!xml.contains("<forward"));
        assert!(xml.contains("start='10.2.3.128' end='10.2.3.254'"));
        assert!(xml.contains("mc123456781234"));
        assert_eq!(
            "10.2.3.0/24"
                .parse::<CloudCidr>()
                .unwrap()
                .ipam_end_offset(),
            127
        );
        assert!(cloud_network_xml("';touch /tmp/x", "10.2.3.0/24").is_err());
        assert!(cloud_network_xml("12345678-1234-1234-1234-123456789abc", "10.2.3.0/29").is_err());
    }
    #[test]
    fn scaling_limits_cooldown_and_unknown_metrics() {
        let p = ScalingPolicy {
            min: 1,
            max: 4,
            desired: 2,
            target_cpu: Some(60.0),
            cooldown_secs: 60,
            ..Default::default()
        };
        assert!(p.validate().is_ok());
        assert_eq!(p.next_desired(Some(95.0), 60), 3);
        assert_eq!(p.next_desired(Some(20.0), 60), 1);
        assert_eq!(p.next_desired(Some(95.0), 59), 2);
        for cpu in [None, Some(f64::NAN), Some(101.0), Some(-1.0), Some(60.0)] {
            assert_eq!(p.next_desired(cpu, 600), 2);
        }
        assert!(ScalingPolicy {
            max: 1,
            ..p.clone()
        }
        .validate()
        .is_err());
        assert!(ScalingPolicy {
            target_cpu: Some(f64::INFINITY),
            ..p
        }
        .validate()
        .is_err());
    }

    #[test]
    fn older_policies_serialize_unchanged() {
        let raw = r#"{"min":0,"max":10,"desired":2,"target_cpu":null,"cooldown_secs":300}"#;
        let p: ScalingPolicy = serde_json::from_str(raw).unwrap();
        assert_eq!(serde_json::to_string(&p).unwrap(), raw);
        let full: ScalingPolicy = serde_json::from_str(
            r#"{"min":1,"max":6,"desired":2,"target_cpu":50.0,"cooldown_secs":60,"predictive":true,"scale_in":"sleep","load_balancer":{"id":"00000000-0000-0000-0000-000000000001","port":8080},"drain_secs":5}"#,
        )
        .unwrap();
        assert!(full.validate().is_ok());
        assert_eq!(full.scale_in, ScaleIn::Sleep);
        assert_eq!(full.drain(), 5);
        assert!(serde_json::from_str::<ScalingPolicy>(
            r#"{"min":0,"max":1,"desired":0,"target_cpu":null,"cooldown_secs":60,"bogus":1}"#
        )
        .is_err());
    }

    #[test]
    fn forecast_only_scales_up() {
        let p = ScalingPolicy {
            min: 1,
            max: 6,
            desired: 2,
            target_cpu: Some(50.0),
            cooldown_secs: 60,
            predictive: true,
            ..Default::default()
        };
        // 210% of demand at 50% each needs 5.
        assert_eq!(
            p.next_desired_with_forecast(Some(50.0), 600, Some(210.0)),
            5
        );
        // A low forecast never lowers the count below the reactive step.
        assert_eq!(p.next_desired_with_forecast(Some(50.0), 600, Some(10.0)), 2);
        assert_eq!(p.next_desired_with_forecast(Some(95.0), 600, Some(10.0)), 3);
        // Capped at max; ignored inside the cooldown only for the reactive part.
        assert_eq!(p.next_desired_with_forecast(None, 600, Some(10_000.0)), 6);
        // Once raised for the rush, idle CPU doesn't step it back down before it.
        let raised = ScalingPolicy {
            desired: 5,
            ..p.clone()
        };
        assert_eq!(
            raised.next_desired_with_forecast(Some(5.0), 600, Some(210.0)),
            5
        );
        let off = ScalingPolicy {
            predictive: false,
            ..p.clone()
        };
        assert_eq!(
            off.next_desired_with_forecast(Some(50.0), 600, Some(210.0)),
            2
        );
        assert!(ScalingPolicy {
            target_cpu: None,
            ..p
        }
        .validate()
        .is_err());
    }
}

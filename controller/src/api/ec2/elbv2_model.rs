// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! ELBv2 without a database: ARNs, the query-parameter shapes the SDKs send (`Targets.member.1.Id`), what is
//! accepted or refused, and the XML each object renders to. `elbv2.rs` holds the handlers.

use std::collections::BTreeMap;

use uuid::Uuid;

use super::{xml_escape, Ec2Error};

pub type Params = BTreeMap<String, String>;

const ARN_PREFIX: &str = "arn:aws:elasticloadbalancing:machina:000000000000:";

pub fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

pub fn unsupported(msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad("UnsupportedOperation", msg)
}

pub fn validation(msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad("ValidationError", msg)
}

pub fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    match p.get(k) {
        Some(v) if !v.is_empty() => Ok(v.clone()),
        _ => Err(validation(format!("{k} is required"))),
    }
}

// ---- names and ARNs ----------------------------------------------------------------------------------------------

/// 1-32 characters, letters, digits and hyphens, no leading or trailing hyphen (what ELBv2 allows).
pub fn check_name(name: &str) -> Result<(), Ec2Error> {
    let ok = !name.is_empty()
        && name.len() <= 32
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if ok {
        Ok(())
    } else {
        Err(validation(format!(
            "'{name}' is not a valid name: use 1-32 letters, digits or hyphens, not starting or ending with a hyphen"
        )))
    }
}

/// The 16 hex digits of an id that ARNs carry.
pub fn id16(id: Uuid) -> String {
    id.simple().to_string()[..16].to_string()
}

fn short(lb_type: &str) -> &'static str {
    if lb_type == "application" {
        "app"
    } else {
        "net"
    }
}

pub fn lb_arn(lb_type: &str, name: &str, id: Uuid) -> String {
    format!("{ARN_PREFIX}loadbalancer/{}/{name}/{}", short(lb_type), id16(id))
}

pub fn tg_arn(name: &str, id: Uuid) -> String {
    format!("{ARN_PREFIX}targetgroup/{name}/{}", id16(id))
}

pub fn listener_arn(lb_type: &str, lb_name: &str, lb_id: Uuid, id: Uuid) -> String {
    format!("{ARN_PREFIX}listener/{}/{lb_name}/{}/{}", short(lb_type), id16(lb_id), id16(id))
}

/// Every listener has exactly one rule, the default one; its id is the listener's.
pub fn rule_arn(lb_type: &str, lb_name: &str, lb_id: Uuid, listener: Uuid) -> String {
    format!("{ARN_PREFIX}listener-rule/{}/{lb_name}/{}/{}/{}", short(lb_type), id16(lb_id), id16(listener), id16(listener))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArnKind {
    LoadBalancer,
    TargetGroup,
    Listener,
    Rule,
}

/// `(kind, id16)` of one of our ARNs; for a rule the id16 is its listener's.
pub fn parse_arn(arn: &str) -> Option<(ArnKind, String)> {
    let rest = arn.strip_prefix(ARN_PREFIX)?;
    let segs: Vec<&str> = rest.split('/').collect();
    let hex = |s: &str| s.len() == 16 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    match segs.as_slice() {
        ["loadbalancer", _, _, id] if hex(id) => Some((ArnKind::LoadBalancer, id.to_string())),
        ["targetgroup", _, id] if hex(id) => Some((ArnKind::TargetGroup, id.to_string())),
        ["listener", _, _, _, id] if hex(id) => Some((ArnKind::Listener, id.to_string())),
        ["listener-rule", _, _, _, listener, rule] if hex(listener) && hex(rule) => Some((ArnKind::Rule, listener.to_string())),
        _ => None,
    }
}

/// `2026-10-08 10:00:00` as `2026-10-08T10:00:00.000Z`.
pub fn iso(ts: &str) -> String {
    let t = ts.trim();
    if t.len() >= 19 && t.as_bytes()[10] == b' ' {
        format!("{}T{}.000Z", &t[..10], &t[11..19])
    } else {
        t.to_string()
    }
}

// ---- query parameters --------------------------------------------------------------------------------------------

/// The numbers N present as `{prefix}.member.N` or `{prefix}.member.N.…`, ascending.
pub fn indices(p: &Params, prefix: &str) -> Vec<u32> {
    let head = format!("{prefix}.member.");
    let mut out: Vec<u32> = p
        .keys()
        .filter_map(|k| k.strip_prefix(&head))
        .filter_map(|rest| rest.split('.').next())
        .filter_map(|n| n.parse().ok())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Values of `{prefix}.member.N`.
pub fn list(p: &Params, prefix: &str) -> Vec<String> {
    indices(p, prefix).into_iter().filter_map(|i| p.get(&format!("{prefix}.member.{i}")).cloned()).collect()
}

pub fn tags(p: &Params, prefix: &str) -> Result<Vec<(String, String)>, Ec2Error> {
    let mut out: Vec<(String, String)> = Vec::new();
    for i in indices(p, prefix) {
        let key = p.get(&format!("{prefix}.member.{i}.Key")).cloned().unwrap_or_default();
        if key.is_empty() || key.chars().count() > 128 {
            return Err(validation("a tag key must be 1-128 characters"));
        }
        let value = p.get(&format!("{prefix}.member.{i}.Value")).cloned().unwrap_or_default();
        if value.chars().count() > 256 {
            return Err(validation("a tag value is at most 256 characters"));
        }
        if let Some(existing) = out.iter_mut().find(|(k, _)| *k == key) {
            existing.1 = value;
        } else {
            out.push((key, value));
        }
    }
    if out.len() > 50 {
        return Err(Ec2Error::bad("TooManyTags", "a resource has at most 50 tags"));
    }
    Ok(out)
}

/// `Targets.member.N.{Id,Port}`; `default_port` fills a missing Port.
pub fn targets(p: &Params, prefix: &str, default_port: Option<i64>) -> Result<Vec<(String, i64)>, Ec2Error> {
    let mut out = Vec::new();
    for i in indices(p, prefix) {
        let id = p.get(&format!("{prefix}.member.{i}.Id")).cloned().ok_or_else(|| validation("every target needs an Id"))?;
        let port = match p.get(&format!("{prefix}.member.{i}.Port")) {
            Some(v) => v.parse::<i64>().map_err(|_| validation("a target Port must be a number"))?,
            None => default_port.ok_or_else(|| validation("every target needs a Port"))?,
        };
        if !(1..=65535).contains(&port) {
            return Err(validation("a target Port must be 1-65535"));
        }
        out.push((id, port));
    }
    if out.is_empty() {
        return Err(validation("Targets is required"));
    }
    Ok(out)
}

/// The listener's only supported action: forward to one target group. Everything else is refused by name.
pub fn forward_target_group(p: &Params, prefix: &str) -> Result<String, Ec2Error> {
    let idx = indices(p, prefix);
    if idx.is_empty() {
        return Err(validation(format!("{prefix} is required")));
    }
    if idx.len() > 1 {
        return Err(unsupported("a listener has one action; chains such as authenticate-then-forward are not supported"));
    }
    let a = format!("{prefix}.member.{}", idx[0]);
    let kind = p.get(&format!("{a}.Type")).map(String::as_str).unwrap_or("");
    if kind != "forward" {
        return Err(unsupported(format!(
            "action type '{kind}' is not supported: Machina's balancer is a layer-4 forwarder, so only 'forward' to a target group works"
        )));
    }
    let direct = p.get(&format!("{a}.TargetGroupArn")).cloned();
    let groups = indices(p, &format!("{a}.ForwardConfig.TargetGroups"));
    if groups.len() > 1 {
        return Err(unsupported("weighted forwarding to several target groups is not supported; forward to one"));
    }
    if p.get(&format!("{a}.ForwardConfig.TargetGroupStickinessConfig.Enabled")).map(String::as_str) == Some("true") {
        return Err(unsupported("target group stickiness is not supported"));
    }
    let nested = groups.first().and_then(|g| p.get(&format!("{a}.ForwardConfig.TargetGroups.member.{g}.TargetGroupArn")).cloned());
    match (direct, nested) {
        (Some(d), Some(n)) if d != n => Err(validation("TargetGroupArn and ForwardConfig name different target groups")),
        (Some(t), _) | (None, Some(t)) => Ok(t),
        _ => Err(validation("a forward action needs a TargetGroupArn")),
    }
}

// ---- health checks -----------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub enabled: bool,
    pub protocol: String,
    pub port: String,
    pub path: String,
    pub interval: i64,
    pub timeout: i64,
    pub healthy: i64,
    pub unhealthy: i64,
    pub matcher: String,
}

impl Health {
    /// Defaults for a target group of this protocol (what the SDK documents).
    pub fn defaults(tg_protocol: &str) -> Health {
        let http = tg_protocol == "HTTP";
        Health {
            enabled: true,
            protocol: if http { "HTTP" } else { "TCP" }.into(),
            port: "traffic-port".into(),
            path: "/".into(),
            interval: 30,
            timeout: if http { 5 } else { 10 },
            healthy: if http { 5 } else { 3 },
            unhealthy: if http { 2 } else { 3 },
            matcher: "200".into(),
        }
    }

    /// Applies the `HealthCheck*` / `Matcher.HttpCode` parameters on top of `self`.
    pub fn with_params(mut self, p: &Params) -> Result<Health, Ec2Error> {
        let num = |k: &str| -> Result<Option<i64>, Ec2Error> {
            p.get(k).map(|v| v.parse::<i64>().map_err(|_| validation(format!("{k} must be a number")))).transpose()
        };
        if let Some(v) = p.get("HealthCheckEnabled") {
            self.enabled = match v.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err(validation("HealthCheckEnabled must be true or false")),
            };
        }
        if let Some(v) = p.get("HealthCheckProtocol") {
            let v = v.to_ascii_uppercase();
            if !matches!(v.as_str(), "TCP" | "HTTP") {
                return Err(unsupported(format!("health check protocol {v} is not supported; use TCP or HTTP")));
            }
            self.protocol = v;
        }
        if let Some(v) = p.get("HealthCheckPort") {
            if v != "traffic-port" && !v.parse::<i64>().map(|n| (1..=65535).contains(&n)).unwrap_or(false) {
                return Err(validation("HealthCheckPort must be 'traffic-port' or 1-65535"));
            }
            self.port = v.clone();
        }
        if let Some(v) = p.get("HealthCheckPath") {
            self.path = v.clone();
        }
        if let Some(v) = num("HealthCheckIntervalSeconds")? {
            self.interval = v;
        }
        if let Some(v) = num("HealthCheckTimeoutSeconds")? {
            self.timeout = v;
        }
        if let Some(v) = num("HealthyThresholdCount")? {
            self.healthy = v;
        }
        if let Some(v) = num("UnhealthyThresholdCount")? {
            self.unhealthy = v;
        }
        if let Some(v) = p.get("Matcher.HttpCode") {
            // The balancer's HTTP probe counts 2xx and 3xx as healthy; the SDK default "200" is accepted as that.
            if !matches!(v.as_str(), "200" | "200-299" | "200-399") {
                return Err(unsupported(format!(
                    "Matcher.HttpCode '{v}' is not supported: the probe treats 200-399 as healthy; use 200, 200-299 or 200-399"
                )));
            }
            self.matcher = v.clone();
        }
        if p.get("Matcher.GrpcCode").is_some() {
            return Err(unsupported("gRPC health checks are not supported"));
        }
        self.check()?;
        Ok(self)
    }

    /// The native balancer's own limits (`lb_health::validate`), reported as ELBv2 validation errors.
    pub fn check(&self) -> Result<(), Ec2Error> {
        if !self.enabled {
            return Ok(());
        }
        let native = self.native();
        crate::engine::lb_health::validate(&native).map_err(|e| validation(format!("health check: {e}")))
    }

    /// The native balancer's check: `none` when disabled.
    pub fn native(&self) -> crate::engine::lb_health::Check {
        crate::engine::lb_health::Check {
            protocol: if self.enabled { self.protocol.to_ascii_lowercase() } else { "none".into() },
            port: self.port.parse().ok(),
            path: self.path.clone(),
            interval_secs: self.interval,
            timeout_secs: self.timeout,
            healthy_threshold: self.healthy,
            unhealthy_threshold: self.unhealthy,
        }
    }
}

// ---- attributes --------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrKind {
    LoadBalancer,
    TargetGroup,
    Listener,
}

/// `(key, default)` of the attributes each object has. Setting one that promises behaviour Machina does not have is refused by
/// `check_attribute`; the rest are stored and reported back.
pub fn attribute_defaults(kind: AttrKind, lb_type: &str) -> Vec<(&'static str, &'static str)> {
    match kind {
        AttrKind::LoadBalancer if lb_type == "application" => vec![
            ("access_logs.s3.enabled", "false"),
            ("access_logs.s3.bucket", ""),
            ("access_logs.s3.prefix", ""),
            ("deletion_protection.enabled", "false"),
            ("idle_timeout.timeout_seconds", "60"),
            ("routing.http.desync_mitigation_mode", "defensive"),
            ("routing.http.drop_invalid_header_fields.enabled", "false"),
            ("routing.http2.enabled", "true"),
            ("load_balancing.cross_zone.enabled", "true"),
        ],
        AttrKind::LoadBalancer => vec![
            ("access_logs.s3.enabled", "false"),
            ("access_logs.s3.bucket", ""),
            ("access_logs.s3.prefix", ""),
            ("deletion_protection.enabled", "false"),
            ("load_balancing.cross_zone.enabled", "false"),
        ],
        AttrKind::TargetGroup => vec![
            ("deregistration_delay.timeout_seconds", "300"),
            ("stickiness.enabled", "false"),
            ("stickiness.type", "source_ip"),
            ("stickiness.lb_cookie.duration_seconds", "86400"),
            ("load_balancing.algorithm.type", "round_robin"),
            ("slow_start.duration_seconds", "0"),
            ("proxy_protocol_v2.enabled", "false"),
            ("preserve_client_ip.enabled", "true"),
        ],
        AttrKind::Listener => vec![("tcp.idle_timeout.seconds", "350"), ("routing.http.response.server.enabled", "true")],
    }
}

/// Validates one `Attributes.member.N` pair before it is stored.
pub fn check_attribute(kind: AttrKind, lb_type: &str, key: &str, value: &str) -> Result<(), Ec2Error> {
    let defaults = attribute_defaults(kind, lb_type);
    let Some((_, default)) = defaults.iter().find(|(k, _)| *k == key) else {
        return Err(validation(format!("'{key}' is not an attribute of this resource")));
    };
    let refuse = |what: &str| -> Result<(), Ec2Error> { Err(unsupported(format!("{key}={value} is not supported: {what}"))) };
    match key {
        "access_logs.s3.enabled" if value == "true" => refuse("access logs are not written"),
        "stickiness.enabled" if value == "true" => refuse("the balancer does not pin clients to targets"),
        "load_balancing.algorithm.type" if value != "round_robin" => refuse("the balancer is weighted round robin only"),
        "slow_start.duration_seconds" if value != "0" => refuse("targets are not ramped up"),
        "proxy_protocol_v2.enabled" if value == "true" => refuse("the PROXY protocol is not added"),
        "routing.http.desync_mitigation_mode" if value != *default => refuse("HTTP is forwarded as TCP without inspection"),
        "routing.http.drop_invalid_header_fields.enabled" if value == "true" => refuse("HTTP is forwarded as TCP without inspection"),
        "deletion_protection.enabled" | "load_balancing.cross_zone.enabled" | "routing.http2.enabled" if !matches!(value, "true" | "false") => {
            Err(validation(format!("{key} must be true or false")))
        }
        "idle_timeout.timeout_seconds" | "tcp.idle_timeout.seconds" | "deregistration_delay.timeout_seconds" if value.parse::<u32>().is_err() => {
            Err(validation(format!("{key} must be a number")))
        }
        _ => Ok(()),
    }
}

pub fn attributes_xml(rows: &[(String, String)]) -> String {
    let items: String = rows
        .iter()
        .map(|(k, v)| format!("<member><Key>{}</Key><Value>{}</Value></member>", xml_escape(k), xml_escape(v)))
        .collect();
    format!("<Attributes>{items}</Attributes>")
}

pub fn tags_xml(tags: &[(String, String)]) -> String {
    let items: String = tags
        .iter()
        .map(|(k, v)| format!("<member><Key>{}</Key><Value>{}</Value></member>", xml_escape(k), xml_escape(v)))
        .collect();
    format!("<Tags>{items}</Tags>")
}

// ---- rendering ---------------------------------------------------------------------------------------------------

pub struct LbView<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub lb_type: &'a str,
    pub scheme: &'a str,
    pub created_at: &'a str,
    pub zone: &'a str,
    pub subnets: &'a [String],
    /// `(code, reason)` of the state.
    pub state: (&'a str, &'a str),
}

pub fn lb_xml(v: &LbView) -> String {
    let zones: String = if v.subnets.is_empty() {
        format!("<member><ZoneName>{}</ZoneName></member>", xml_escape(v.zone))
    } else {
        v.subnets
            .iter()
            .map(|s| format!("<member><ZoneName>{}</ZoneName><SubnetId>{}</SubnetId></member>", xml_escape(v.zone), xml_escape(s)))
            .collect()
    };
    let reason = if v.state.1.is_empty() { String::new() } else { format!("<Reason>{}</Reason>", xml_escape(v.state.1)) };
    format!(
        "<member><LoadBalancerArn>{}</LoadBalancerArn><DNSName>{}-{}.elb.machina</DNSName><CanonicalHostedZoneId>Z00000000MACHINA</CanonicalHostedZoneId>\
         <CreatedTime>{}</CreatedTime><LoadBalancerName>{}</LoadBalancerName><Scheme>{}</Scheme><Type>{}</Type>\
         <State><Code>{}</Code>{reason}</State><AvailabilityZones>{zones}</AvailabilityZones><SecurityGroups/><IpAddressType>ipv4</IpAddressType></member>",
        lb_arn(v.lb_type, v.name, v.id),
        xml_escape(v.name),
        id16(v.id),
        iso(v.created_at),
        xml_escape(v.name),
        xml_escape(v.scheme),
        xml_escape(v.lb_type),
        xml_escape(v.state.0),
    )
}

pub struct TgView<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub protocol: &'a str,
    pub port: i64,
    pub vpc_id: &'a str,
    pub target_type: &'a str,
    pub health: &'a Health,
    pub lb_arns: &'a [String],
}

pub fn tg_xml(v: &TgView) -> String {
    let lbs: String = v.lb_arns.iter().map(|a| format!("<member>{}</member>", xml_escape(a))).collect();
    let vpc = if v.vpc_id.is_empty() { String::new() } else { format!("<VpcId>{}</VpcId>", xml_escape(v.vpc_id)) };
    let path = if v.health.protocol == "HTTP" { format!("<HealthCheckPath>{}</HealthCheckPath>", xml_escape(&v.health.path)) } else { String::new() };
    let matcher = if v.health.protocol == "HTTP" { format!("<Matcher><HttpCode>{}</HttpCode></Matcher>", xml_escape(&v.health.matcher)) } else { String::new() };
    let version = if v.protocol == "HTTP" { "<ProtocolVersion>HTTP1</ProtocolVersion>" } else { "" };
    format!(
        "<member><TargetGroupArn>{}</TargetGroupArn><TargetGroupName>{}</TargetGroupName><Protocol>{}</Protocol><Port>{}</Port>{vpc}\
         <HealthCheckProtocol>{}</HealthCheckProtocol><HealthCheckPort>{}</HealthCheckPort><HealthCheckEnabled>{}</HealthCheckEnabled>\
         <HealthCheckIntervalSeconds>{}</HealthCheckIntervalSeconds><HealthCheckTimeoutSeconds>{}</HealthCheckTimeoutSeconds>\
         <HealthyThresholdCount>{}</HealthyThresholdCount><UnhealthyThresholdCount>{}</UnhealthyThresholdCount>{path}{matcher}\
         <LoadBalancerArns>{lbs}</LoadBalancerArns><TargetType>{}</TargetType>{version}<IpAddressType>ipv4</IpAddressType></member>",
        tg_arn(v.name, v.id),
        xml_escape(v.name),
        xml_escape(v.protocol),
        v.port,
        v.health.protocol,
        xml_escape(&v.health.port),
        v.health.enabled,
        v.health.interval,
        v.health.timeout,
        v.health.healthy,
        v.health.unhealthy,
        xml_escape(v.target_type),
    )
}

pub fn actions_xml(tg_arn: &str) -> String {
    format!(
        "<member><Type>forward</Type><TargetGroupArn>{a}</TargetGroupArn><Order>1</Order><ForwardConfig><TargetGroups>\
         <member><TargetGroupArn>{a}</TargetGroupArn><Weight>1</Weight></member></TargetGroups>\
         <TargetGroupStickinessConfig><Enabled>false</Enabled></TargetGroupStickinessConfig></ForwardConfig></member>",
        a = xml_escape(tg_arn)
    )
}

pub fn listener_xml(lb_type: &str, lb_name: &str, lb_id: Uuid, id: Uuid, protocol: &str, port: i64, tg: &str) -> String {
    format!(
        "<member><ListenerArn>{}</ListenerArn><LoadBalancerArn>{}</LoadBalancerArn><Port>{port}</Port><Protocol>{}</Protocol>\
         <DefaultActions>{}</DefaultActions></member>",
        listener_arn(lb_type, lb_name, lb_id, id),
        lb_arn(lb_type, lb_name, lb_id),
        xml_escape(protocol),
        actions_xml(tg)
    )
}

pub fn rule_xml(lb_type: &str, lb_name: &str, lb_id: Uuid, listener: Uuid, tg: &str) -> String {
    format!(
        "<member><RuleArn>{}</RuleArn><Priority>default</Priority><Conditions/><Actions>{}</Actions><IsDefault>true</IsDefault></member>",
        rule_arn(lb_type, lb_name, lb_id, listener),
        actions_xml(tg)
    )
}

/// `(state, reason, description)` of a target in `DescribeTargetHealth`.
pub fn target_health_xml(id: &str, port: i64, health_port: &str, state: &str, reason: &str, description: &str) -> String {
    let reason = if reason.is_empty() { String::new() } else { format!("<Reason>{reason}</Reason>") };
    let description = if description.is_empty() { String::new() } else { format!("<Description>{}</Description>", xml_escape(description)) };
    format!(
        "<member><Target><Id>{}</Id><Port>{port}</Port></Target><HealthCheckPort>{}</HealthCheckPort>\
         <TargetHealth><State>{state}</State>{reason}{description}</TargetHealth></member>",
        xml_escape(id),
        xml_escape(health_port)
    )
}

/// Maps a native member's health onto ELBv2's states: `(state, reason, description)`.
pub fn target_state(native: &str, health_checks_on: bool, detail: &str) -> (&'static str, &'static str, String) {
    if !health_checks_on {
        return ("healthy", "", String::new());
    }
    match native {
        "healthy" => ("healthy", "", String::new()),
        "unhealthy" => ("unhealthy", "Target.FailedHealthChecks", detail.to_string()),
        _ => ("initial", "Elb.InitialHealthChecking", "Initial health checking in progress".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn arns_round_trip_for_every_kind() {
        let lb = Uuid::new_v4();
        let l = Uuid::new_v4();
        let tg = Uuid::new_v4();
        assert_eq!(parse_arn(&lb_arn("network", "web", lb)), Some((ArnKind::LoadBalancer, id16(lb))));
        assert_eq!(parse_arn(&lb_arn("application", "web", lb)), Some((ArnKind::LoadBalancer, id16(lb))));
        assert_eq!(parse_arn(&tg_arn("pool", tg)), Some((ArnKind::TargetGroup, id16(tg))));
        assert_eq!(parse_arn(&listener_arn("network", "web", lb, l)), Some((ArnKind::Listener, id16(l))));
        assert_eq!(parse_arn(&rule_arn("network", "web", lb, l)), Some((ArnKind::Rule, id16(l))));
        assert!(lb_arn("application", "web", lb).contains("loadbalancer/app/web/"));
        assert!(lb_arn("network", "web", lb).starts_with("arn:aws:elasticloadbalancing:machina:000000000000:loadbalancer/net/web/"));
        for bad in ["", "arn:aws:s3:::x", "arn:aws:elasticloadbalancing:machina:000000000000:loadbalancer/net/web/zz", "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/web/0123456789abcdef"] {
            assert_eq!(parse_arn(bad), None, "{bad}");
        }
    }

    #[test]
    fn names_follow_the_elbv2_rules() {
        for ok in ["a", "web-1", "A1-b2", &"x".repeat(32)] {
            assert!(check_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-a", "a-", "a_b", "a b", &"x".repeat(33), "ü"] {
            assert_eq!(check_name(bad).unwrap_err().code, "ValidationError", "{bad}");
        }
    }

    #[test]
    fn member_lists_tags_and_targets_parse_in_order() {
        let p = params(&[
            ("Names.member.2", "b"),
            ("Names.member.1", "a"),
            ("Names.member.10", "c"),
            ("Tags.member.1.Key", "env"),
            ("Tags.member.1.Value", "prod"),
            ("Tags.member.2.Key", "env"),
            ("Tags.member.2.Value", "dev"),
            ("Targets.member.1.Id", "i-0123456789abcdef0"),
            ("Targets.member.1.Port", "8080"),
            ("Targets.member.2.Id", "i-0123456789abcdef1"),
        ]);
        assert_eq!(list(&p, "Names"), ["a", "b", "c"]);
        assert_eq!(tags(&p, "Tags").unwrap(), [("env".to_string(), "dev".to_string())]);
        assert_eq!(targets(&p, "Targets", Some(80)).unwrap(), [("i-0123456789abcdef0".to_string(), 8080), ("i-0123456789abcdef1".to_string(), 80)]);
        assert_eq!(targets(&p, "Targets", None).unwrap_err().code, "ValidationError");
        assert_eq!(targets(&params(&[]), "Targets", Some(80)).unwrap_err().code, "ValidationError");
        assert_eq!(targets(&params(&[("Targets.member.1.Id", "i-1"), ("Targets.member.1.Port", "70000")]), "Targets", None).unwrap_err().code, "ValidationError");
        let long = params(&[("Tags.member.1.Key", &"k".repeat(129))]);
        assert_eq!(tags(&long, "Tags").unwrap_err().code, "ValidationError");
    }

    #[test]
    fn only_a_plain_forward_action_is_accepted() {
        let direct = params(&[("DefaultActions.member.1.Type", "forward"), ("DefaultActions.member.1.TargetGroupArn", "arn-x")]);
        assert_eq!(forward_target_group(&direct, "DefaultActions").unwrap(), "arn-x");
        let nested = params(&[
            ("DefaultActions.member.1.Type", "forward"),
            ("DefaultActions.member.1.ForwardConfig.TargetGroups.member.1.TargetGroupArn", "arn-y"),
            ("DefaultActions.member.1.ForwardConfig.TargetGroups.member.1.Weight", "1"),
        ]);
        assert_eq!(forward_target_group(&nested, "DefaultActions").unwrap(), "arn-y");
        let both = params(&[
            ("DefaultActions.member.1.Type", "forward"),
            ("DefaultActions.member.1.TargetGroupArn", "arn-x"),
            ("DefaultActions.member.1.ForwardConfig.TargetGroups.member.1.TargetGroupArn", "arn-y"),
        ]);
        assert_eq!(forward_target_group(&both, "DefaultActions").unwrap_err().code, "ValidationError");
        let weighted = params(&[
            ("DefaultActions.member.1.Type", "forward"),
            ("DefaultActions.member.1.ForwardConfig.TargetGroups.member.1.TargetGroupArn", "a"),
            ("DefaultActions.member.1.ForwardConfig.TargetGroups.member.2.TargetGroupArn", "b"),
        ]);
        assert_eq!(forward_target_group(&weighted, "DefaultActions").unwrap_err().code, "UnsupportedOperation");
        for kind in ["redirect", "fixed-response", "authenticate-oidc"] {
            let p = params(&[("DefaultActions.member.1.Type", kind)]);
            let e = forward_target_group(&p, "DefaultActions").unwrap_err();
            assert_eq!(e.code, "UnsupportedOperation", "{kind}");
            assert!(e.message.contains(kind));
        }
        let two = params(&[("DefaultActions.member.1.Type", "forward"), ("DefaultActions.member.2.Type", "forward")]);
        assert_eq!(forward_target_group(&two, "DefaultActions").unwrap_err().code, "UnsupportedOperation");
        let sticky = params(&[
            ("DefaultActions.member.1.Type", "forward"),
            ("DefaultActions.member.1.TargetGroupArn", "a"),
            ("DefaultActions.member.1.ForwardConfig.TargetGroupStickinessConfig.Enabled", "true"),
        ]);
        assert_eq!(forward_target_group(&sticky, "DefaultActions").unwrap_err().code, "UnsupportedOperation");
        assert_eq!(forward_target_group(&params(&[]), "DefaultActions").unwrap_err().code, "ValidationError");
    }

    #[test]
    fn health_check_defaults_and_overrides_map_onto_the_native_check() {
        let tcp = Health::defaults("TCP");
        assert_eq!((tcp.protocol.as_str(), tcp.interval, tcp.timeout, tcp.healthy, tcp.unhealthy), ("TCP", 30, 10, 3, 3));
        tcp.check().unwrap();
        let n = tcp.native();
        assert_eq!((n.protocol.as_str(), n.port), ("tcp", None));
        let http = Health::defaults("HTTP");
        http.check().unwrap();
        let over = http
            .with_params(&params(&[("HealthCheckPort", "8081"), ("HealthCheckPath", "/up"), ("HealthCheckIntervalSeconds", "10"), ("HealthCheckTimeoutSeconds", "4"), ("Matcher.HttpCode", "200-399")]))
            .unwrap();
        let n = over.native();
        assert_eq!((n.protocol.as_str(), n.port, n.path.as_str(), n.interval_secs, n.timeout_secs), ("http", Some(8081), "/up", 10, 4));
        let off = Health::defaults("TCP").with_params(&params(&[("HealthCheckEnabled", "false")])).unwrap();
        assert_eq!(off.native().protocol, "none");
    }

    #[test]
    fn health_checks_the_probe_cannot_do_are_refused() {
        let d = Health::defaults("HTTP");
        for (k, v, code) in [
            ("HealthCheckProtocol", "HTTPS", "UnsupportedOperation"),
            ("Matcher.HttpCode", "200,204", "UnsupportedOperation"),
            ("Matcher.GrpcCode", "12", "UnsupportedOperation"),
            ("HealthCheckPort", "0", "ValidationError"),
            ("HealthCheckPort", "web", "ValidationError"),
            ("HealthCheckIntervalSeconds", "2", "ValidationError"),
            ("HealthCheckIntervalSeconds", "x", "ValidationError"),
            ("HealthCheckTimeoutSeconds", "30", "ValidationError"),
            ("HealthyThresholdCount", "11", "ValidationError"),
            ("HealthCheckEnabled", "yes", "ValidationError"),
        ] {
            assert_eq!(d.clone().with_params(&params(&[(k, v)])).unwrap_err().code, code, "{k}={v}");
        }
    }

    #[test]
    fn attributes_that_promise_unavailable_behaviour_are_refused() {
        use AttrKind::*;
        assert!(check_attribute(LoadBalancer, "network", "deletion_protection.enabled", "true").is_ok());
        assert!(check_attribute(LoadBalancer, "application", "idle_timeout.timeout_seconds", "120").is_ok());
        assert!(check_attribute(TargetGroup, "network", "deregistration_delay.timeout_seconds", "30").is_ok());
        for (kind, key, value, code) in [
            (LoadBalancer, "access_logs.s3.enabled", "true", "UnsupportedOperation"),
            (LoadBalancer, "no.such.key", "1", "ValidationError"),
            (LoadBalancer, "deletion_protection.enabled", "maybe", "ValidationError"),
            (TargetGroup, "stickiness.enabled", "true", "UnsupportedOperation"),
            (TargetGroup, "load_balancing.algorithm.type", "least_outstanding_requests", "UnsupportedOperation"),
            (TargetGroup, "slow_start.duration_seconds", "30", "UnsupportedOperation"),
            (TargetGroup, "proxy_protocol_v2.enabled", "true", "UnsupportedOperation"),
            (Listener, "tcp.idle_timeout.seconds", "soon", "ValidationError"),
        ] {
            assert_eq!(check_attribute(kind, "application", key, value).unwrap_err().code, code, "{key}={value}");
        }
        // an application-only attribute does not exist on a network balancer
        assert_eq!(check_attribute(LoadBalancer, "network", "idle_timeout.timeout_seconds", "60").unwrap_err().code, "ValidationError");
    }

    #[test]
    fn xml_carries_what_the_sdks_read() {
        let id = Uuid::new_v4();
        let subnets = vec!["subnet-0123456789abcdef0".to_string()];
        let x = lb_xml(&LbView { id, name: "web", lb_type: "network", scheme: "internet-facing", created_at: "2026-10-08 10:00:00", zone: "host-a", subnets: &subnets, state: ("active", "") });
        for needle in [
            "<LoadBalancerName>web</LoadBalancerName>",
            "<Type>network</Type>",
            "<State><Code>active</Code></State>",
            "<CreatedTime>2026-10-08T10:00:00.000Z</CreatedTime>",
            "<SubnetId>subnet-0123456789abcdef0</SubnetId>",
            ".elb.machina</DNSName>",
        ] {
            assert!(x.contains(needle), "{needle} in {x}");
        }
        let failed = lb_xml(&LbView { id, name: "web", lb_type: "network", scheme: "internal", created_at: "", zone: "h", subnets: &[], state: ("failed", "agent <down>") });
        assert!(failed.contains("<State><Code>failed</Code><Reason>agent &lt;down&gt;</Reason></State>"), "{failed}");

        let h = Health::defaults("HTTP");
        let tg = tg_xml(&TgView { id, name: "pool", protocol: "HTTP", port: 80, vpc_id: "vpc-1", target_type: "instance", health: &h, lb_arns: &[lb_arn("application", "web", id)] });
        for needle in ["<TargetGroupName>pool</TargetGroupName>", "<HealthCheckPath>/</HealthCheckPath>", "<Matcher><HttpCode>200</HttpCode></Matcher>", "<ProtocolVersion>HTTP1</ProtocolVersion>", "<VpcId>vpc-1</VpcId>", "<TargetType>instance</TargetType>"] {
            assert!(tg.contains(needle), "{needle} in {tg}");
        }
        let tcp = Health::defaults("TCP");
        let tg = tg_xml(&TgView { id, name: "pool", protocol: "TCP", port: 80, vpc_id: "", target_type: "instance", health: &tcp, lb_arns: &[] });
        assert!(!tg.contains("HealthCheckPath") && !tg.contains("<Matcher>") && !tg.contains("<VpcId>"), "{tg}");
        assert!(tg.contains("<LoadBalancerArns></LoadBalancerArns>"));

        let r = rule_xml("network", "web", id, id, "arn-x");
        assert!(r.contains("<Priority>default</Priority>") && r.contains("<IsDefault>true</IsDefault>") && r.contains("<Conditions/>"), "{r}");
        let l = listener_xml("network", "web", id, id, "TCP", 80, "arn-x");
        assert!(l.contains("<Port>80</Port>") && l.contains("<Type>forward</Type>") && l.contains("<Weight>1</Weight>"), "{l}");
    }

    #[test]
    fn native_health_maps_onto_elbv2_target_states() {
        assert_eq!(target_state("healthy", true, "").0, "healthy");
        assert_eq!(target_state("unknown", true, "").0, "initial");
        let (s, r, d) = target_state("unhealthy", true, "connection refused");
        assert_eq!((s, r, d.as_str()), ("unhealthy", "Target.FailedHealthChecks", "connection refused"));
        // with the check off every target in rotation is healthy
        assert_eq!(target_state("unknown", false, "").0, "healthy");
        let x = target_health_xml("i-1", 80, "traffic-port", "unhealthy", "Target.FailedHealthChecks", "a < b");
        assert!(x.contains("<State>unhealthy</State><Reason>Target.FailedHealthChecks</Reason><Description>a &lt; b</Description>"), "{x}");
    }

    #[test]
    fn timestamps_render_as_iso_8601() {
        assert_eq!(iso("2026-10-08 10:11:12"), "2026-10-08T10:11:12.000Z");
        assert_eq!(iso("n/a"), "n/a");
    }
}

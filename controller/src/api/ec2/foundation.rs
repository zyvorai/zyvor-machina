// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! What every EC2 client relies on and no single action owns: `DryRun`, `ClientToken` idempotency, one filter
//! engine (wildcards, `tag:KEY`, `tag-key`, `tag-value`, loud errors for unknown names) and `MaxResults`/`NextToken`
//! on every describe that returns a list.
//!
//! The filter and pagination steps work on the XML an action already produced: the first element of the result is a
//! set of `<item>` children, so the same code serves every describe without each one re-implementing it.

use std::collections::BTreeMap;

use axum::http::StatusCode;
use sha2::{Digest, Sha256};

use crate::auth::{require_admin, require_operator, AuthUser};
use crate::state::AppState;

use super::{page, parse_filters, Ec2Error};

type Params = BTreeMap<String, String>;

// ---- wildcards and tag filters ---------------------------------------------------------------------------------

/// EC2 filter wildcard match: `*` is any run of characters, `?` exactly one.
pub fn wild_match(pattern: &str, value: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let v: Vec<char> = value.chars().collect();
    let (mut pi, mut vi) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while vi < v.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == v[vi]) {
            pi += 1;
            vi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = vi;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            vi = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

pub fn any_match(values: &[String], candidate: &str) -> bool {
    values.iter().any(|pat| wild_match(pat, candidate))
}

/// `Some(result)` when `name` is a tag filter (`tag:KEY`, `tag-key`, `tag-value`), `None` for any other filter name.
pub fn tag_filter_matches(tags: &[(String, String)], name: &str, values: &[String]) -> Option<bool> {
    if let Some(key) = name.strip_prefix("tag:") {
        return Some(tags.iter().any(|(k, v)| k == key && any_match(values, v)));
    }
    match name {
        "tag-key" => Some(tags.iter().any(|(k, _)| any_match(values, k))),
        "tag-value" => Some(tags.iter().any(|(_, v)| any_match(values, v))),
        _ => None,
    }
}

// ---- DryRun ----------------------------------------------------------------------------------------------------

fn flag(p: &Params, key: &str) -> Result<bool, Ec2Error> {
    match p.get(key).map(String::as_str) {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err(Ec2Error::bad("InvalidParameterValue", format!("{key} must be true or false"))),
    }
}

/// Actions that need an administrator (the same ones whose handlers call `require_admin`).
const ADMIN_ACTIONS: &[&str] = &["FenceHost", "CreateWebhook", "DeleteWebhook", "ProposeResize", "ProposeConsolidation", "CreateProject"];

fn is_read(action: &str) -> bool {
    action.starts_with("Describe") || action.starts_with("Get")
}

/// `DryRun=true` answers what the real call would: `UnauthorizedOperation` when the caller may not do it,
/// `DryRunOperation` (HTTP 412, as AWS) when it would have been allowed. Nothing is changed. The check is the
/// caller's role; parameters are not validated (the real call does that).
pub fn dry_run_gate(action: &str, p: &Params, actor: &AuthUser) -> Result<(), Ec2Error> {
    if !flag(p, "DryRun")? {
        return Ok(());
    }
    if !is_read(action) {
        if ADMIN_ACTIONS.contains(&action) {
            require_admin(actor)?;
        } else {
            require_operator(actor)?;
        }
    }
    Err(Ec2Error::new(
        StatusCode::PRECONDITION_FAILED,
        "DryRunOperation",
        "Request would have succeeded, but DryRun flag is set.",
    ))
}

// ---- ClientToken idempotency -----------------------------------------------------------------------------------

/// Actions that honour `ClientToken`.
pub const IDEMPOTENT_ACTIONS: &[&str] = &["RunInstances", "CreateLaunchTemplate", "CreateFleet", "RequestSpotInstances"];

pub fn validate_token(token: &str) -> Result<(), Ec2Error> {
    if token.is_empty() || token.len() > 64 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Ec2Error::bad("InvalidParameterValue", "ClientToken must be 1 to 64 printable ASCII characters"));
    }
    Ok(())
}

/// Hash of everything that makes two requests "the same", independent of parameter order.
pub fn request_hash(action: &str, p: &Params) -> String {
    let mut h = Sha256::new();
    h.update(action.as_bytes());
    for (k, v) in p {
        if matches!(k.as_str(), "ClientToken" | "DryRun" | "Action" | "Version") {
            continue;
        }
        h.update([0]);
        h.update(k.as_bytes());
        h.update([1]);
        h.update(v.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, PartialEq, Eq)]
pub enum TokenClaim {
    /// First time: run the action, then `finish` (or `release` on failure).
    Fresh,
    /// Same token and same request: answer with what was answered before.
    Replay(String),
    /// Same token, different request.
    Mismatch,
    /// Same token and request, still running.
    InFlight,
}

pub async fn claim_token(state: &AppState, user: &str, action: &str, token: &str, hash: &str) -> Result<TokenClaim, Ec2Error> {
    // tokens are remembered for a day (AWS keeps them at least a few hours)
    let _ = crate::db::query("DELETE FROM ec2_client_tokens WHERE created_at < datetime('now', '-1 day')")
        .execute(&state.pool)
        .await;
    let inserted = crate::db::query(
        "INSERT INTO ec2_client_tokens (username, action, token, request_hash, response) VALUES (?, ?, ?, ?, '') \
         ON CONFLICT (username, action, token) DO NOTHING",
    )
    .bind(user)
    .bind(action)
    .bind(token)
    .bind(hash)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if inserted == 1 {
        return Ok(TokenClaim::Fresh);
    }
    let row: Option<(String, String)> =
        crate::db::query_as("SELECT request_hash, response FROM ec2_client_tokens WHERE username = ? AND action = ? AND token = ?")
            .bind(user)
            .bind(action)
            .bind(token)
            .fetch_optional(&state.pool)
            .await?;
    Ok(match row {
        None => TokenClaim::Fresh,
        Some((h, _)) if h != hash => TokenClaim::Mismatch,
        Some((_, r)) if r.is_empty() => TokenClaim::InFlight,
        Some((_, r)) => TokenClaim::Replay(r),
    })
}

pub async fn finish_token(state: &AppState, user: &str, action: &str, token: &str, response: &str) -> Result<(), Ec2Error> {
    crate::db::query("UPDATE ec2_client_tokens SET response = ? WHERE username = ? AND action = ? AND token = ?")
        .bind(response)
        .bind(user)
        .bind(action)
        .bind(token)
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// The action failed: forget the token so the client can retry with it.
pub async fn release_token(state: &AppState, user: &str, action: &str, token: &str) {
    let _ = crate::db::query("DELETE FROM ec2_client_tokens WHERE username = ? AND action = ? AND token = ?")
        .bind(user)
        .bind(action)
        .bind(token)
        .execute(&state.pool)
        .await;
}

pub fn mismatch_error() -> Ec2Error {
    Ec2Error::bad("IdempotentParameterMismatch", "The same ClientToken was used with different parameters")
}

pub fn in_flight_error() -> Ec2Error {
    Ec2Error::new(StatusCode::CONFLICT, "ConcurrentIdempotentRequest", "A request with this ClientToken is still being processed")
}

// ---- filter tables ---------------------------------------------------------------------------------------------

/// How one filter name is answered: by the action itself (`native`, it reads the parameter) or by comparing the
/// direct child element `field` of every result item.
pub struct FilterDef {
    pub name: &'static str,
    pub field: Option<&'static str>,
}

const fn native(name: &'static str) -> FilterDef {
    FilterDef { name, field: None }
}
const fn field(name: &'static str, f: &'static str) -> FilterDef {
    FilterDef { name, field: Some(f) }
}

/// Per describe: whether the action filters tags itself, and the filter names it answers. Describes that are not
/// listed here accept any filter they implement (nothing is validated for them).
pub struct FilterSpec {
    pub action: &'static str,
    pub native_tags: bool,
    pub filters: &'static [FilterDef],
}

pub const FILTER_SPECS: &[FilterSpec] = &[
    FilterSpec {
        action: "DescribeInstances",
        native_tags: true,
        filters: &[native("instance-id"), native("instance-state-name"), native("instance-type"), native("private-ip-address")],
    },
    FilterSpec {
        action: "DescribeVolumes",
        native_tags: true,
        filters: &[native("volume-id"), native("status"), native("size"), native("attachment.instance-id")],
    },
    FilterSpec { action: "DescribeSecurityGroups", native_tags: true, filters: &[native("group-id"), native("group-name")] },
    FilterSpec { action: "DescribeImages", native_tags: true, filters: &[native("image-id"), native("name"), native("is-public")] },
    FilterSpec {
        action: "DescribeVpcs",
        native_tags: false,
        filters: &[field("vpc-id", "vpcId"), field("cidr", "cidrBlock"), field("cidr-block", "cidrBlock"), field("state", "state")],
    },
    FilterSpec {
        action: "DescribeSubnets",
        native_tags: false,
        filters: &[field("subnet-id", "subnetId"), field("vpc-id", "vpcId"), field("cidr-block", "cidrBlock"), field("cidr", "cidrBlock"), field("state", "state")],
    },
    FilterSpec {
        action: "DescribeNetworkInterfaces",
        native_tags: false,
        filters: &[
            field("network-interface-id", "networkInterfaceId"),
            field("subnet-id", "subnetId"),
            field("status", "status"),
            field("mac-address", "macAddress"),
            field("private-ip-address", "privateIpAddress"),
            field("addresses.private-ip-address", "privateIpAddress"),
        ],
    },
    FilterSpec {
        action: "DescribeKeyPairs",
        native_tags: false,
        filters: &[field("key-name", "keyName"), field("key-pair-id", "keyPairId"), field("fingerprint", "keyFingerprint")],
    },
    FilterSpec {
        action: "DescribeLaunchTemplates",
        native_tags: false,
        filters: &[field("launch-template-id", "launchTemplateId"), field("launch-template-name", "launchTemplateName")],
    },
    FilterSpec {
        action: "DescribeSnapshots",
        native_tags: false,
        filters: &[field("snapshot-id", "snapshotId"), field("volume-id", "volumeId"), field("status", "status")],
    },
    FilterSpec {
        action: "DescribeInternetGateways",
        native_tags: true,
        filters: &[native("internet-gateway-id"), native("attachment.vpc-id"), native("attachment.state"), native("owner-id")],
    },
    FilterSpec {
        action: "DescribeNatGateways",
        native_tags: true,
        filters: &[native("nat-gateway-id"), native("vpc-id"), native("subnet-id"), native("state")],
    },
    FilterSpec {
        action: "DescribeDhcpOptions",
        native_tags: true,
        filters: &[native("dhcp-options-id"), native("key"), native("value"), native("owner-id")],
    },
    FilterSpec {
        action: "DescribeRouteTables",
        native_tags: true,
        filters: &[
            native("route-table-id"),
            native("vpc-id"),
            native("owner-id"),
            native("association.main"),
            native("association.route-table-id"),
            native("association.route-table-association-id"),
            native("association.subnet-id"),
            native("route.destination-cidr-block"),
            native("route.gateway-id"),
            native("route.nat-gateway-id"),
            native("route.vpc-peering-connection-id"),
            native("route.state"),
        ],
    },
    FilterSpec {
        action: "DescribeNetworkAcls",
        native_tags: true,
        filters: &[
            native("network-acl-id"),
            native("vpc-id"),
            native("default"),
            native("owner-id"),
            native("association.network-acl-id"),
            native("association.network-acl-association-id"),
            native("association.subnet-id"),
            native("entry.cidr"),
            native("entry.rule-action"),
            native("entry.egress"),
            native("entry.protocol"),
            native("entry.rule-number"),
        ],
    },
    FilterSpec {
        action: "DescribeSecurityGroupRules",
        native_tags: true,
        filters: &[native("security-group-rule-id"), native("group-id")],
    },
    FilterSpec {
        action: "DescribeInstanceTypes",
        native_tags: false,
        filters: &[field("instance-type", "instanceType"), field("current-generation", "currentGeneration"), field("bare-metal", "bareMetal"), field("hypervisor", "hypervisor")],
    },
    FilterSpec {
        action: "DescribePlacementGroups",
        native_tags: false,
        filters: &[field("group-name", "groupName"), field("strategy", "strategy"), field("state", "state"), field("group-id", "groupId")],
    },
    FilterSpec { action: "DescribeVolumeStatus", native_tags: false, filters: &[] },
    FilterSpec { action: "DescribeLaunchTemplateVersions", native_tags: false, filters: &[] },
    FilterSpec {
        action: "DescribeTags",
        native_tags: true,
        filters: &[native("key"), native("value"), native("resource-type"), native("resource-id")],
    },
];

fn spec_for(action: &str) -> Option<&'static FilterSpec> {
    FILTER_SPECS.iter().find(|s| s.action == action)
}

/// Unknown filter names (and filters without a value) are an error, never "matches nothing".
pub fn validate_filters(action: &str, p: &Params) -> Result<(), Ec2Error> {
    let Some(spec) = spec_for(action) else { return Ok(()) };
    for (name, values) in parse_filters(p) {
        if values.is_empty() {
            return Err(Ec2Error::bad("InvalidParameterValue", format!("The filter '{name}' needs at least one value")));
        }
        let tag = name.starts_with("tag:") || name == "tag-key" || name == "tag-value";
        if tag || spec.filters.iter().any(|f| f.name == name) {
            continue;
        }
        return Err(Ec2Error::bad("InvalidParameterValue", format!("The filter '{name}' is not valid for {action}")));
    }
    Ok(())
}

// ---- the XML result: set, items, children ----------------------------------------------------------------------

/// `<xSet><item>…</item>…</xSet>` followed by anything (a `nextToken`, other elements).
#[derive(Debug, PartialEq, Eq)]
pub struct SetXml {
    pub open: String,
    pub items: Vec<String>,
    pub close: String,
    pub tail: String,
}

/// Direct child elements of an XML fragment as (name, inner text), skipping text between them.
pub fn children(xml: &str) -> Vec<(String, String)> {
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(end) = xml[i..].find('>') else { break };
        let tag = &xml[i + 1..i + end];
        if tag.starts_with('/') || tag.starts_with('?') {
            i += end + 1;
            continue;
        }
        let selfclosing = tag.ends_with('/');
        let name: String = tag.trim_end_matches('/').split_whitespace().next().unwrap_or("").to_string();
        if selfclosing {
            out.push((name, String::new()));
            i += end + 1;
            continue;
        }
        let body_start = i + end + 1;
        let mut depth = 1usize;
        let mut j = body_start;
        let mut body_end = None;
        while j < b.len() {
            if b[j] == b'<' {
                let Some(e2) = xml[j..].find('>') else { break };
                let t2 = &xml[j + 1..j + e2];
                if let Some(rest) = t2.strip_prefix('/') {
                    if rest == name {
                        depth -= 1;
                        if depth == 0 {
                            body_end = Some(j);
                            j += e2 + 1;
                            break;
                        }
                    }
                } else if !t2.ends_with('/') && t2.split_whitespace().next() == Some(name.as_str()) {
                    depth += 1;
                }
                j += e2 + 1;
            } else {
                j += 1;
            }
        }
        match body_end {
            Some(be) => {
                out.push((name, xml[body_start..be].to_string()));
                i = j;
            }
            None => break,
        }
    }
    out
}

pub fn split_set(inner: &str) -> Option<SetXml> {
    let first = inner.trim_start();
    if !first.starts_with('<') {
        return None;
    }
    let end = first.find('>')?;
    let name = &first[1..end];
    if name.is_empty() || name.ends_with('/') || name.starts_with('/') || !name.ends_with("Set") && name != "launchTemplates" && name != "securityGroupInfo" && name != "availabilityZoneInfo" {
        return None;
    }
    let kids = children(first);
    let (set_name, body) = kids.first()?.clone();
    if set_name != name {
        return None;
    }
    let items: Vec<String> = children(&body).into_iter().filter(|(n, _)| n == "item").map(|(_, x)| x).collect();
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let outer_len = open.len() + body.len() + close.len();
    let tail = first.get(outer_len..).unwrap_or("").to_string();
    Some(SetXml { open, items, close, tail })
}

impl SetXml {
    pub fn render(&self) -> String {
        let items: String = self.items.iter().map(|i| format!("<item>{i}</item>")).collect();
        format!("{}{items}{}{}", self.open, self.close, self.tail)
    }
}

fn item_tags(item: &str) -> Vec<(String, String)> {
    let kids = children(item);
    let Some((_, tag_set)) = kids.iter().find(|(n, _)| n == "tagSet") else { return Vec::new() };
    children(tag_set)
        .into_iter()
        .filter(|(n, _)| n == "item")
        .map(|(_, x)| {
            let kv = children(&x);
            let get = |k: &str| kv.iter().find(|(n, _)| n == k).map(|(_, v)| unescape(v)).unwrap_or_default();
            (get("key"), get("value"))
        })
        .collect()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Apply the filters a describe left to us (tag filters and `field` filters).
pub fn filter_items(action: &str, p: &Params, set: &mut SetXml) {
    let Some(spec) = spec_for(action) else { return };
    let filters = parse_filters(p);
    if filters.is_empty() {
        return;
    }
    set.items.retain(|item| {
        let kids = children(item);
        let tags = if spec.native_tags { Vec::new() } else { item_tags(item) };
        filters.iter().all(|(name, values)| {
            if !spec.native_tags {
                if let Some(r) = tag_filter_matches(&tags, name, values) {
                    return r;
                }
            }
            match spec.filters.iter().find(|f| f.name == name) {
                Some(FilterDef { field: Some(f), .. }) => kids.iter().find(|(n, _)| n == f).is_some_and(|(_, v)| any_match(values, &unescape(v))),
                // native filters were applied by the action; tag filters of native-tag actions too
                _ => true,
            }
        })
    });
}

fn item_id(item: &str, index: usize) -> String {
    let id = children(item)
        .into_iter()
        .find(|(n, v)| n.ends_with("Id") && n != "ownerId" && !v.is_empty())
        .map(|(_, v)| v);
    match id {
        Some(v) if v.len() <= 64 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') => v,
        _ => format!("n-{index:08}"),
    }
}

/// `MaxResults` / `NextToken` over the items of a describe (only when the caller asked for pages).
pub fn paginate(p: &Params, set: &mut SetXml) -> Result<(), Ec2Error> {
    if !p.contains_key("MaxResults") && !p.contains_key("NextToken") {
        return Ok(());
    }
    let bad = |m: String| Ec2Error::bad("InvalidParameterValue", m);
    let limit = page::max_results(p.get("MaxResults").map(String::as_str)).map_err(bad)?;
    let mut ids: Vec<String> = set.items.iter().enumerate().map(|(n, i)| item_id(i, n)).collect();
    // a repeated id would make a token ambiguous: fall back to positions for the whole list
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() != ids.len() {
        ids = (0..ids.len()).map(|n| format!("n-{n:08}")).collect();
    }
    let (slice, token) = page::page(&ids, p.get("NextToken").map(String::as_str), limit).map_err(bad)?;
    let keep: Vec<String> = slice.to_vec();
    let items = std::mem::take(&mut set.items);
    set.items = items.into_iter().zip(ids.iter()).filter(|(_, id)| keep.contains(id)).map(|(i, _)| i).collect();
    set.tail.push_str(&page::token_xml(&token));
    Ok(())
}

/// Filters then pages the result of a describe. Results that are not a set of items pass through unchanged.
pub fn post_process(action: &str, p: &Params, inner: String) -> Result<String, Ec2Error> {
    if !action.starts_with("Describe") || inner.contains("<nextToken>") {
        return Ok(inner);
    }
    let wants = !parse_filters(p).is_empty() || p.contains_key("MaxResults") || p.contains_key("NextToken");
    if !wants {
        return Ok(inner);
    }
    let Some(mut set) = split_set(&inner) else { return Ok(inner) };
    filter_items(action, p, &mut set);
    paginate(p, &mut set)?;
    Ok(set.render())
}

// ---- EC2 resource type names -----------------------------------------------------------------------------------

/// `resource_tags.resource_type` → the EC2 `resource-type` name.
pub fn ec2_resource_type(internal: &str) -> String {
    match internal {
        "vm" => "instance".into(),
        "port" => "network-interface".into(),
        "nat_gateway" => "natgateway".into(),
        other => other.replace('_', "-"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn actor(role: &str) -> AuthUser {
        AuthUser { username: "u".into(), role: role.into(), auth_source: None }
    }

    #[test]
    fn wildcards() {
        assert!(wild_match("web-*", "web-1"));
        assert!(wild_match("*", ""));
        assert!(wild_match("w?b", "web"));
        assert!(!wild_match("w?b", "weeb"));
        assert!(wild_match("*prod*", "my-prod-db"));
        assert!(!wild_match("web", "web-1"));
        assert!(wild_match("a*b*c", "aXXbYYc"));
        assert!(!wild_match("a*b*c", "aXXbYY"));
    }

    #[test]
    fn tag_filters() {
        let tags = vec![("Env".to_string(), "prod".to_string()), ("Name".to_string(), "web-1".to_string())];
        assert_eq!(tag_filter_matches(&tags, "tag:Env", &["pr*".into()]), Some(true));
        assert_eq!(tag_filter_matches(&tags, "tag:Env", &["dev".into()]), Some(false));
        assert_eq!(tag_filter_matches(&tags, "tag-key", &["Na*".into()]), Some(true));
        assert_eq!(tag_filter_matches(&tags, "tag-value", &["web-?".into()]), Some(true));
        assert_eq!(tag_filter_matches(&tags, "instance-id", &["x".into()]), None);
    }

    #[test]
    fn dry_run_says_what_the_real_call_would() {
        let dry = p(&[("DryRun", "true")]);
        let e = dry_run_gate("TerminateInstances", &dry, &actor("operator")).unwrap_err();
        assert_eq!((e.status, e.code), (StatusCode::PRECONDITION_FAILED, "DryRunOperation"));
        let e = dry_run_gate("TerminateInstances", &dry, &actor("viewer")).unwrap_err();
        assert_eq!(e.code, "UnauthorizedOperation");
        let e = dry_run_gate("FenceHost", &dry, &actor("operator")).unwrap_err();
        assert_eq!(e.code, "UnauthorizedOperation");
        let e = dry_run_gate("FenceHost", &dry, &actor("admin")).unwrap_err();
        assert_eq!(e.code, "DryRunOperation");
        // a read needs no role
        assert_eq!(dry_run_gate("DescribeInstances", &dry, &actor("viewer")).unwrap_err().code, "DryRunOperation");
        // no flag, or false: the call goes ahead
        assert!(dry_run_gate("TerminateInstances", &p(&[]), &actor("viewer")).is_ok());
        assert!(dry_run_gate("TerminateInstances", &p(&[("DryRun", "false")]), &actor("viewer")).is_ok());
        assert_eq!(dry_run_gate("TerminateInstances", &p(&[("DryRun", "maybe")]), &actor("admin")).unwrap_err().code, "InvalidParameterValue");
    }

    #[test]
    fn request_hash_ignores_order_token_and_dry_run_but_not_values() {
        let a = request_hash("RunInstances", &p(&[("ImageId", "ami-1"), ("MaxCount", "1"), ("ClientToken", "t1")]));
        let b = request_hash("RunInstances", &p(&[("MaxCount", "1"), ("ImageId", "ami-1"), ("ClientToken", "t2"), ("DryRun", "false")]));
        let c = request_hash("RunInstances", &p(&[("ImageId", "ami-1"), ("MaxCount", "2")]));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, request_hash("CreateLaunchTemplate", &p(&[("ImageId", "ami-1"), ("MaxCount", "1")])));
    }

    #[test]
    fn tokens_are_printable_and_bounded() {
        assert!(validate_token("abc-123").is_ok());
        assert!(validate_token("").is_err());
        assert!(validate_token(&"x".repeat(65)).is_err());
        assert!(validate_token("has space\n").is_err());
    }

    #[tokio::test]
    async fn token_claims_replay_mismatch_and_release() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        assert_eq!(claim_token(&state, "u", "RunInstances", "t", "h1").await.unwrap(), TokenClaim::Fresh);
        // a second request while the first runs
        assert_eq!(claim_token(&state, "u", "RunInstances", "t", "h1").await.unwrap(), TokenClaim::InFlight);
        assert_eq!(claim_token(&state, "u", "RunInstances", "t", "h2").await.unwrap(), TokenClaim::Mismatch);
        finish_token(&state, "u", "RunInstances", "t", "<x/>").await.unwrap();
        assert_eq!(claim_token(&state, "u", "RunInstances", "t", "h1").await.unwrap(), TokenClaim::Replay("<x/>".into()));
        // another user or action has its own tokens
        assert_eq!(claim_token(&state, "v", "RunInstances", "t", "h1").await.unwrap(), TokenClaim::Fresh);
        assert_eq!(claim_token(&state, "u", "CreateLaunchTemplate", "t", "h1").await.unwrap(), TokenClaim::Fresh);
        // a failed run frees the token
        release_token(&state, "u", "RunInstances", "t").await;
        assert_eq!(claim_token(&state, "u", "RunInstances", "t", "h3").await.unwrap(), TokenClaim::Fresh);
    }

    #[test]
    fn unknown_filter_names_are_an_error_but_tags_and_unlisted_actions_pass() {
        let ok = p(&[("Filter.1.Name", "tag:Env"), ("Filter.1.Value.1", "prod"), ("Filter.2.Name", "vpc-id"), ("Filter.2.Value.1", "vpc-1")]);
        assert!(validate_filters("DescribeSubnets", &ok).is_ok());
        let typo = p(&[("Filter.1.Name", "vpcid"), ("Filter.1.Value.1", "x")]);
        assert_eq!(validate_filters("DescribeSubnets", &typo).unwrap_err().code, "InvalidParameterValue");
        let empty = p(&[("Filter.1.Name", "vpc-id")]);
        assert!(validate_filters("DescribeSubnets", &empty).is_err());
        assert!(validate_filters("DescribeAlarms", &typo).is_ok());
    }

    const SUBNETS: &str = "<subnetSet>\
        <item><subnetId>subnet-a</subnetId><vpcId>vpc-1</vpcId><state>available</state><tagSet><item><key>Env</key><value>prod</value></item></tagSet></item>\
        <item><subnetId>subnet-b</subnetId><vpcId>vpc-2</vpcId><state>available</state><tagSet/></item>\
        <item><subnetId>subnet-c</subnetId><vpcId>vpc-1</vpcId><state>pending</state><tagSet><item><key>Env</key><value>dev</value></item></tagSet></item>\
        </subnetSet>";

    fn ids(x: &str) -> Vec<String> {
        split_set(x).unwrap().items.iter().map(|i| children(i).into_iter().find(|(n, _)| n == "subnetId").unwrap().1).collect()
    }

    #[test]
    fn children_and_sets_are_split_with_nesting() {
        let set = split_set(SUBNETS).unwrap();
        assert_eq!(set.items.len(), 3);
        assert!(set.items[0].contains("<tagSet><item><key>Env</key>"));
        assert_eq!(set.tail, "");
        assert_eq!(set.render(), SUBNETS);
        let with_tail = format!("{SUBNETS}<nextToken>x</nextToken>");
        assert_eq!(split_set(&with_tail).unwrap().tail, "<nextToken>x</nextToken>");
        assert!(split_set("<return>true</return>").is_none());
    }

    #[test]
    fn field_and_tag_filters_apply_to_items() {
        let by_vpc = p(&[("Filter.1.Name", "vpc-id"), ("Filter.1.Value.1", "vpc-1")]);
        assert_eq!(ids(&post_process("DescribeSubnets", &by_vpc, SUBNETS.into()).unwrap()), ["subnet-a", "subnet-c"]);
        let wild = p(&[("Filter.1.Name", "subnet-id"), ("Filter.1.Value.1", "subnet-[ab]"), ("Filter.2.Name", "state"), ("Filter.2.Value.1", "avail*")]);
        // brackets are literal in EC2 wildcards, so nothing matches
        assert!(ids(&post_process("DescribeSubnets", &wild, SUBNETS.into()).unwrap()).is_empty());
        let tag = p(&[("Filter.1.Name", "tag:Env"), ("Filter.1.Value.1", "pr*")]);
        assert_eq!(ids(&post_process("DescribeSubnets", &tag, SUBNETS.into()).unwrap()), ["subnet-a"]);
        let key = p(&[("Filter.1.Name", "tag-key"), ("Filter.1.Value.1", "Env")]);
        assert_eq!(ids(&post_process("DescribeSubnets", &key, SUBNETS.into()).unwrap()), ["subnet-a", "subnet-c"]);
    }

    #[test]
    fn pages_walk_the_items_and_end_without_a_token() {
        let two = p(&[("MaxResults", "2")]);
        let first = post_process("DescribeSubnets", &two, SUBNETS.into()).unwrap();
        assert_eq!(ids(&first), ["subnet-a", "subnet-b"]);
        let token = first.split("<nextToken>").nth(1).unwrap().split("</nextToken>").next().unwrap().to_string();
        let next = post_process("DescribeSubnets", &p(&[("MaxResults", "2"), ("NextToken", &token)]), SUBNETS.into()).unwrap();
        assert_eq!(ids(&next), ["subnet-c"]);
        assert!(!next.contains("<nextToken>"));
        // no MaxResults: everything, untouched
        assert_eq!(post_process("DescribeSubnets", &p(&[]), SUBNETS.into()).unwrap(), SUBNETS);
        assert!(post_process("DescribeSubnets", &p(&[("MaxResults", "0")]), SUBNETS.into()).is_err());
        assert!(post_process("DescribeSubnets", &p(&[("NextToken", "@@@")]), SUBNETS.into()).is_err());
    }

    #[test]
    fn instances_keep_their_native_tag_filtering_and_page_by_reservation() {
        let xml = "<reservationSet>\
            <item><reservationId>r-1</reservationId><instancesSet><item><instanceId>i-1</instanceId><tagSet><item><key>A</key><value>1</value></item></tagSet></item></instancesSet></item>\
            <item><reservationId>r-2</reservationId><instancesSet><item><instanceId>i-2</instanceId></item></instancesSet></item>\
            </reservationSet>";
        // the action already filtered by tag; the post step must not drop items for lacking a direct tagSet
        let f = p(&[("Filter.1.Name", "tag:A"), ("Filter.1.Value.1", "1")]);
        assert_eq!(split_set(&post_process("DescribeInstances", &f, xml.into()).unwrap()).unwrap().items.len(), 2);
        let one = post_process("DescribeInstances", &p(&[("MaxResults", "1")]), xml.into()).unwrap();
        assert_eq!(split_set(&one).unwrap().items.len(), 1);
        assert!(one.contains("<nextToken>"));
    }

    #[test]
    fn resource_type_names() {
        assert_eq!(ec2_resource_type("vm"), "instance");
        assert_eq!(ec2_resource_type("security_group"), "security-group");
        assert_eq!(ec2_resource_type("port"), "network-interface");
        assert_eq!(ec2_resource_type("volume"), "volume");
        assert_eq!(ec2_resource_type("nat_gateway"), "natgateway");
        assert_eq!(ec2_resource_type("route_table"), "route-table");
    }
}

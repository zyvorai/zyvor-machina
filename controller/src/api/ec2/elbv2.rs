// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! ELBv2 (service `elasticloadbalancing`) on the native layer-4 balancer.
//!
//! A *listener* with a forward action is one native balancer (`load_balancers`: iptables DNAT on the ELBv2 balancer's host, see
//! `engine/load_balancer.rs`) whose `lb_members` mirror the *target group's* targets, and the target group's health check is the
//! native health check. What the native balancer cannot do is refused by name, never dropped: HTTPS/TLS listeners, rules with
//! conditions, redirects and fixed responses, weighted or sticky forwarding, security groups on the balancer, non-instance
//! targets. Listener ports are per host: two balancers cannot both listen on 80/tcp, because they share the host's address.

use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::db::DbPool;
use crate::resource_ids::Kind;
use crate::state::AppState;

use super::elbv2_model::*;
use super::{xml_escape, Ec2Error};

// ---- rows --------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
struct Lb {
    id: Uuid,
    name: String,
    lb_type: String,
    scheme: String,
    host_id: Uuid,
    subnet_ids: String,
    created_at: String,
}
macro_rules! lb_cols {
    () => {
        "id, name, lb_type, scheme, host_id, subnet_ids, created_at"
    };
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct Tg {
    id: Uuid,
    name: String,
    protocol: String,
    port: i64,
    target_type: String,
    vpc_id: String,
    hc_enabled: bool,
    hc_protocol: String,
    hc_port: String,
    hc_path: String,
    hc_interval_secs: i64,
    hc_timeout_secs: i64,
    hc_healthy_threshold: i64,
    hc_unhealthy_threshold: i64,
    hc_matcher: String,
}
macro_rules! tg_cols {
    () => {
        "id, name, protocol, port, target_type, vpc_id, hc_enabled, hc_protocol, hc_port, hc_path, hc_interval_secs, hc_timeout_secs, \
         hc_healthy_threshold, hc_unhealthy_threshold, hc_matcher"
    };
}

impl Tg {
    fn health(&self) -> Health {
        Health {
            enabled: self.hc_enabled,
            protocol: self.hc_protocol.clone(),
            port: self.hc_port.clone(),
            path: self.hc_path.clone(),
            interval: self.hc_interval_secs,
            timeout: self.hc_timeout_secs,
            healthy: self.hc_healthy_threshold,
            unhealthy: self.hc_unhealthy_threshold,
            matcher: self.hc_matcher.clone(),
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct Listener {
    id: Uuid,
    load_balancer_id: Uuid,
    protocol: String,
    port: i64,
    target_group_id: Uuid,
    native_lb_id: Option<Uuid>,
}
macro_rules! listener_cols {
    () => {
        "id, load_balancer_id, protocol, port, target_group_id, native_lb_id"
    };
}

async fn all_lbs(pool: &DbPool) -> Result<Vec<Lb>, Ec2Error> {
    Ok(crate::db::query_as::<_, Lb>(concat!("SELECT ", lb_cols!(), " FROM elbv2_load_balancers ORDER BY name")).fetch_all(pool).await?)
}

async fn all_tgs(pool: &DbPool) -> Result<Vec<Tg>, Ec2Error> {
    Ok(crate::db::query_as::<_, Tg>(concat!("SELECT ", tg_cols!(), " FROM elbv2_target_groups ORDER BY name")).fetch_all(pool).await?)
}

async fn listeners_of(pool: &DbPool, lb: Uuid) -> Result<Vec<Listener>, Ec2Error> {
    Ok(crate::db::query_as::<_, Listener>(concat!("SELECT ", listener_cols!(), " FROM elbv2_listeners WHERE load_balancer_id = ? ORDER BY port"))
        .bind(lb)
        .fetch_all(pool)
        .await?)
}

async fn all_listeners(pool: &DbPool) -> Result<Vec<Listener>, Ec2Error> {
    Ok(crate::db::query_as::<_, Listener>(concat!("SELECT ", listener_cols!(), " FROM elbv2_listeners ORDER BY port")).fetch_all(pool).await?)
}

async fn lb_by_id(pool: &DbPool, id: Uuid) -> Result<Lb, Ec2Error> {
    crate::db::query_as::<_, Lb>(concat!("SELECT ", lb_cols!(), " FROM elbv2_load_balancers WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| bad("LoadBalancerNotFound", "One or more load balancers not found"))
}

async fn tg_by_id(pool: &DbPool, id: Uuid) -> Result<Tg, Ec2Error> {
    crate::db::query_as::<_, Tg>(concat!("SELECT ", tg_cols!(), " FROM elbv2_target_groups WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| bad("TargetGroupNotFound", "One or more target groups not found"))
}

fn arn_hex(arn: &str, want: ArnKind, code: &'static str) -> Result<String, Ec2Error> {
    match parse_arn(arn) {
        Some((k, hex)) if k == want => Ok(hex),
        _ => Err(bad(code, format!("'{arn}' is not a valid ARN for this resource"))),
    }
}

async fn find_lb(pool: &DbPool, arn: &str) -> Result<Lb, Ec2Error> {
    let hex = arn_hex(arn, ArnKind::LoadBalancer, "ValidationError")?;
    all_lbs(pool).await?.into_iter().find(|l| id16(l.id) == hex).ok_or_else(|| bad("LoadBalancerNotFound", format!("Load balancer '{arn}' not found")))
}

async fn find_tg(pool: &DbPool, arn: &str) -> Result<Tg, Ec2Error> {
    let hex = arn_hex(arn, ArnKind::TargetGroup, "ValidationError")?;
    all_tgs(pool).await?.into_iter().find(|t| id16(t.id) == hex).ok_or_else(|| bad("TargetGroupNotFound", format!("Target group '{arn}' not found")))
}

async fn find_listener(pool: &DbPool, arn: &str) -> Result<Listener, Ec2Error> {
    let hex = arn_hex(arn, ArnKind::Listener, "ValidationError")?;
    all_listeners(pool).await?.into_iter().find(|l| id16(l.id) == hex).ok_or_else(|| bad("ListenerNotFound", format!("Listener '{arn}' not found")))
}

/// The listener a rule ARN belongs to (every listener has only its default rule).
async fn find_rule_listener(pool: &DbPool, arn: &str) -> Result<Listener, Ec2Error> {
    let hex = arn_hex(arn, ArnKind::Rule, "ValidationError")?;
    all_listeners(pool).await?.into_iter().find(|l| id16(l.id) == hex).ok_or_else(|| bad("RuleNotFound", format!("Rule '{arn}' not found")))
}

// ---- paging ------------------------------------------------------------------------------------------------------

/// `Marker` / `PageSize` over items keyed by their 16-hex id; returns the page and the `<NextMarker>` element.
fn page_items<T>(p: &Params, mut items: Vec<(String, T)>) -> Result<(Vec<T>, String), Ec2Error> {
    let size = match p.get("PageSize") {
        None => 400,
        Some(v) => v.parse::<usize>().ok().filter(|n| (1..=400).contains(n)).ok_or_else(|| validation("PageSize must be 1-400"))?,
    };
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let after = match p.get("Marker") {
        Some(m) => Some(super::page::decode(m).map_err(|_| validation("the Marker is not valid"))?),
        None => None,
    };
    let rest: Vec<(String, T)> = items.into_iter().filter(|(k, _)| after.as_deref().is_none_or(|a| k.as_str() > a)).collect();
    let more = rest.len() > size;
    let page: Vec<(String, T)> = rest.into_iter().take(size).collect();
    let next = if more { page.last().map(|(k, _)| format!("<NextMarker>{}</NextMarker>", super::page::encode(k))).unwrap_or_default() } else { String::new() };
    Ok((page.into_iter().map(|(_, v)| v).collect(), next))
}

// ---- attributes and tags -----------------------------------------------------------------------------------------

async fn attributes(pool: &DbPool, kind: AttrKind, lb_type: &str, resource: Uuid) -> Result<Vec<(String, String)>, Ec2Error> {
    let stored: Vec<(String, String)> = crate::db::query_as("SELECT attr_key, attr_value FROM elbv2_attributes WHERE resource_id = ?").bind(resource).fetch_all(pool).await?;
    Ok(attribute_defaults(kind, lb_type)
        .into_iter()
        .map(|(k, d)| (k.to_string(), stored.iter().find(|(sk, _)| sk == k).map(|(_, v)| v.clone()).unwrap_or_else(|| d.to_string())))
        .collect())
}

async fn set_attributes(pool: &DbPool, p: &Params, kind: AttrKind, lb_type: &str, resource: Uuid) -> Result<Vec<(String, String)>, Ec2Error> {
    let mut pairs = Vec::new();
    for i in indices(p, "Attributes") {
        let key = p.get(&format!("Attributes.member.{i}.Key")).cloned().ok_or_else(|| validation("every attribute needs a Key"))?;
        let value = p.get(&format!("Attributes.member.{i}.Value")).cloned().unwrap_or_default();
        check_attribute(kind, lb_type, &key, &value)?;
        pairs.push((key, value));
    }
    if pairs.is_empty() {
        return Err(validation("Attributes is required"));
    }
    for (k, v) in &pairs {
        crate::db::query(
            "INSERT INTO elbv2_attributes (resource_id, attr_key, attr_value) VALUES (?, ?, ?) \
             ON CONFLICT (resource_id, attr_key) DO UPDATE SET attr_value = excluded.attr_value",
        )
        .bind(resource)
        .bind(k)
        .bind(v)
        .execute(pool)
        .await?;
    }
    Ok(pairs)
}

async fn tags_of(pool: &DbPool, resource: Uuid) -> Result<Vec<(String, String)>, Ec2Error> {
    Ok(crate::db::query_as("SELECT tag_key, tag_value FROM elbv2_tags WHERE resource_id = ? ORDER BY tag_key").bind(resource).fetch_all(pool).await?)
}

async fn put_tags(pool: &DbPool, resource: Uuid, tags: &[(String, String)]) -> Result<(), Ec2Error> {
    for (k, v) in tags {
        crate::db::query(
            "INSERT INTO elbv2_tags (resource_id, tag_key, tag_value) VALUES (?, ?, ?) \
             ON CONFLICT (resource_id, tag_key) DO UPDATE SET tag_value = excluded.tag_value",
        )
        .bind(resource)
        .bind(k)
        .bind(v)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn forget(pool: &DbPool, resource: Uuid) -> Result<(), Ec2Error> {
    crate::db::query("DELETE FROM elbv2_tags WHERE resource_id = ?").bind(resource).execute(pool).await?;
    crate::db::query("DELETE FROM elbv2_attributes WHERE resource_id = ?").bind(resource).execute(pool).await?;
    Ok(())
}

// ---- the native balancer -----------------------------------------------------------------------------------------

/// Makes the native balancer of `l` match its target group: creates it on first use, copies the health check and mirrors the targets.
/// Database only; `push` sends the result to the host's agent.
async fn sync_native(pool: &DbPool, l: &Listener) -> Result<Uuid, Ec2Error> {
    let lb = lb_by_id(pool, l.load_balancer_id).await?;
    let tg = tg_by_id(pool, l.target_group_id).await?;
    let protocol = if l.protocol == "UDP" { "udp" } else { "tcp" };
    let native = match l.native_lb_id {
        Some(id) => id,
        None => {
            let owner: Option<String> = crate::db::query_scalar("SELECT name FROM load_balancers WHERE host_id = ? AND protocol = ? AND listener_port = ?")
                .bind(lb.host_id)
                .bind(protocol)
                .bind(l.port)
                .fetch_optional(pool)
                .await?;
            if let Some(owner) = owner {
                return Err(validation(format!("port {}/{protocol} is already served on this balancer's host by '{owner}'; listener ports are per host", l.port)));
            }
            let id = Uuid::new_v4();
            crate::db::query("INSERT INTO load_balancers (id, name, protocol, host_id, listener_port) VALUES (?, ?, ?, ?, ?)")
                .bind(id)
                .bind(format!("elbv2-{}-{}", lb.name, l.port))
                .bind(protocol)
                .bind(lb.host_id)
                .bind(l.port)
                .execute(pool)
                .await?;
            crate::db::query("UPDATE elbv2_listeners SET native_lb_id = ? WHERE id = ?").bind(id).bind(l.id).execute(pool).await?;
            id
        }
    };

    let want = tg.health().native();
    let have: (String, Option<i64>, String, i64, i64, i64, i64) = crate::db::query_as(
        "SELECT hc_protocol, hc_port, hc_path, hc_interval_secs, hc_timeout_secs, hc_healthy_threshold, hc_unhealthy_threshold FROM load_balancers WHERE id = ?",
    )
    .bind(native)
    .fetch_one(pool)
    .await?;
    let wanted = (want.protocol.clone(), want.port, want.path.clone(), want.interval_secs, want.timeout_secs, want.healthy_threshold, want.unhealthy_threshold);
    if have != wanted {
        crate::db::query(
            "UPDATE load_balancers SET hc_protocol = ?, hc_port = ?, hc_path = ?, hc_interval_secs = ?, hc_timeout_secs = ?, \
             hc_healthy_threshold = ?, hc_unhealthy_threshold = ?, hc_last_run = NULL WHERE id = ?",
        )
        .bind(&want.protocol)
        .bind(want.port)
        .bind(&want.path)
        .bind(want.interval_secs)
        .bind(want.timeout_secs)
        .bind(want.healthy_threshold)
        .bind(want.unhealthy_threshold)
        .bind(native)
        .execute(pool)
        .await?;
        crate::db::query("UPDATE lb_members SET health = 'unknown', health_ok = 0, health_fail = 0, health_detail = '' WHERE load_balancer_id = ?")
            .bind(native)
            .execute(pool)
            .await?;
    }

    let desired: Vec<(Uuid, i64)> = crate::db::query_as("SELECT vm_id, port FROM elbv2_targets WHERE target_group_id = ?").bind(tg.id).fetch_all(pool).await?;
    let members: Vec<(Uuid, Uuid, i64)> = crate::db::query_as("SELECT id, vm_id, port FROM lb_members WHERE load_balancer_id = ?").bind(native).fetch_all(pool).await?;
    for (member, vm, port) in &members {
        if !desired.contains(&(*vm, *port)) {
            crate::db::query("DELETE FROM lb_members WHERE id = ?").bind(*member).execute(pool).await?;
        }
    }
    for (vm, port) in &desired {
        if !members.iter().any(|(_, v, pt)| v == vm && pt == port) {
            crate::db::query("INSERT INTO lb_members (id, load_balancer_id, vm_id, port, weight) VALUES (?, ?, ?, ?, 1)")
                .bind(Uuid::new_v4())
                .bind(native)
                .bind(*vm)
                .bind(*port)
                .execute(pool)
                .await?;
        }
    }
    Ok(native)
}

/// Sends the native balancer's rule set to the host's agent. A failure leaves the balancer in `status = error` (shown as the
/// ELBv2 state `failed`); it does not undo the change, as the database is the record of what was asked for.
#[cfg(not(test))]
async fn push(state: &AppState, native: Uuid) {
    if let Err(e) = crate::engine::load_balancer::apply(&state.pool, &state.config, native).await {
        tracing::warn!("elbv2: pushing balancer {native} to its host failed: {e}");
    }
}

/// Tests never reach an agent: on a host that runs one, a test must not install iptables rules.
#[cfg(test)]
async fn push(_state: &AppState, _native: Uuid) {}

async fn realize(state: &AppState, l: &Listener) -> Result<(), Ec2Error> {
    let native = sync_native(&state.pool, l).await?;
    push(state, native).await;
    Ok(())
}

/// Re-syncs every listener that forwards to `tg`.
async fn resync_group(state: &AppState, tg: Uuid) -> Result<(), Ec2Error> {
    for l in all_listeners(&state.pool).await?.into_iter().filter(|l| l.target_group_id == tg) {
        realize(state, &l).await?;
    }
    Ok(())
}

/// Removes a listener's native balancer (rules on the host, then rows) and its own row.
async fn drop_native(state: &AppState, l: &Listener) -> Result<(), Ec2Error> {
    if let Some(native) = l.native_lb_id {
        crate::engine::load_balancer::teardown(&state.pool, &state.config, native).await;
        crate::db::query("DELETE FROM load_balancers WHERE id = ?").bind(native).execute(&state.pool).await?;
    }
    Ok(())
}

async fn lb_state(pool: &DbPool, lb: Uuid) -> Result<(String, String), Ec2Error> {
    let failed: Option<String> = crate::db::query_scalar(
        "SELECT n.status_message FROM load_balancers n JOIN elbv2_listeners l ON l.native_lb_id = n.id \
         WHERE l.load_balancer_id = ? AND n.status = 'error' LIMIT 1",
    )
    .bind(lb)
    .fetch_optional(pool)
    .await?;
    Ok(match failed {
        Some(msg) => ("failed".into(), msg),
        None => ("active".into(), String::new()),
    })
}

async fn lb_arns_of_tg(pool: &DbPool, tg: Uuid) -> Result<Vec<String>, Ec2Error> {
    let rows: Vec<(Uuid, String, String)> = crate::db::query_as(
        "SELECT DISTINCT b.id, b.name, b.lb_type FROM elbv2_listeners l JOIN elbv2_load_balancers b ON b.id = l.load_balancer_id WHERE l.target_group_id = ?",
    )
    .bind(tg)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id, name, t)| lb_arn(&t, &name, id)).collect())
}

async fn tg_view_xml(pool: &DbPool, tg: &Tg) -> Result<String, Ec2Error> {
    let health = tg.health();
    let arns = lb_arns_of_tg(pool, tg.id).await?;
    Ok(tg_xml(&TgView { id: tg.id, name: &tg.name, protocol: &tg.protocol, port: tg.port, vpc_id: &tg.vpc_id, target_type: &tg.target_type, health: &health, lb_arns: &arns }))
}

async fn lb_view_xml(pool: &DbPool, lb: &Lb) -> Result<String, Ec2Error> {
    let zone: Option<String> = crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?").bind(lb.host_id).fetch_optional(pool).await?;
    let subnets: Vec<String> = lb.subnet_ids.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
    let (code, reason) = lb_state(pool, lb.id).await?;
    Ok(lb_xml(&LbView {
        id: lb.id,
        name: &lb.name,
        lb_type: &lb.lb_type,
        scheme: &lb.scheme,
        created_at: &lb.created_at,
        zone: zone.as_deref().unwrap_or(""),
        subnets: &subnets,
        state: (&code, &reason),
    }))
}

async fn listener_view_xml(pool: &DbPool, l: &Listener) -> Result<String, Ec2Error> {
    let lb = lb_by_id(pool, l.load_balancer_id).await?;
    let tg = tg_by_id(pool, l.target_group_id).await?;
    Ok(listener_xml(&lb.lb_type, &lb.name, lb.id, l.id, &l.protocol, l.port, &tg_arn(&tg.name, tg.id)))
}

async fn rule_view_xml(pool: &DbPool, l: &Listener) -> Result<String, Ec2Error> {
    let lb = lb_by_id(pool, l.load_balancer_id).await?;
    let tg = tg_by_id(pool, l.target_group_id).await?;
    Ok(rule_xml(&lb.lb_type, &lb.name, lb.id, l.id, &tg_arn(&tg.name, tg.id)))
}

// ---- load balancers ----------------------------------------------------------------------------------------------

async fn pick_host(pool: &DbPool) -> Result<Uuid, Ec2Error> {
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM hosts WHERE state = 'online' AND maintenance_mode = FALSE AND fenced = FALSE ORDER BY created_at, hostname LIMIT 1")
        .fetch_optional(pool)
        .await?;
    id.ok_or_else(|| bad("InvalidConfigurationRequest", "no online host can run a balancer"))
}

pub async fn create_load_balancer(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = need(p, "Name")?;
    check_name(&name)?;
    let lb_type = p.get("Type").map(String::as_str).unwrap_or("application");
    match lb_type {
        "application" | "network" => {}
        other => return Err(unsupported(format!("load balancer type '{other}' is not supported; use application or network"))),
    }
    let scheme = p.get("Scheme").map(String::as_str).unwrap_or("internet-facing");
    if !matches!(scheme, "internet-facing" | "internal") {
        return Err(validation("Scheme must be internet-facing or internal"));
    }
    if p.get("IpAddressType").is_some_and(|v| v != "ipv4") {
        return Err(unsupported("only IpAddressType ipv4 is supported"));
    }
    if !list(p, "SecurityGroups").is_empty() {
        return Err(unsupported("security groups on a load balancer are not enforced; leave SecurityGroups empty"));
    }
    let mut subnet_ids = list(p, "Subnets");
    for i in indices(p, "SubnetMappings") {
        if let Some(s) = p.get(&format!("SubnetMappings.member.{i}.SubnetId")) {
            subnet_ids.push(s.clone());
        }
        if p.contains_key(&format!("SubnetMappings.member.{i}.AllocationId")) {
            return Err(unsupported("Elastic IPs on a load balancer's subnets are not supported"));
        }
    }
    for s in &subnet_ids {
        super::more::resolve(state, Kind::Subnet, s, "InvalidSubnet").await?;
    }
    let tags = tags(p, "Tags")?;
    let exists: Option<Uuid> = crate::db::query_scalar("SELECT id FROM elbv2_load_balancers WHERE name = ?").bind(&name).fetch_optional(&state.pool).await?;
    if exists.is_some() {
        return Err(bad("DuplicateLoadBalancerName", format!("A load balancer named '{name}' already exists")));
    }
    let host = pick_host(&state.pool).await?;
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO elbv2_load_balancers (id, name, lb_type, scheme, host_id, subnet_ids) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(id)
        .bind(&name)
        .bind(lb_type)
        .bind(scheme)
        .bind(host)
        .bind(subnet_ids.join(","))
        .execute(&state.pool)
        .await?;
    put_tags(&state.pool, id, &tags).await?;
    let lb = lb_by_id(&state.pool, id).await?;
    Ok(format!("<LoadBalancers>{}</LoadBalancers>", lb_view_xml(&state.pool, &lb).await?))
}

pub async fn describe_load_balancers(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arns = list(p, "LoadBalancerArns");
    let names = list(p, "Names");
    let all = all_lbs(&state.pool).await?;
    for arn in &arns {
        let hex = arn_hex(arn, ArnKind::LoadBalancer, "ValidationError")?;
        if !all.iter().any(|l| id16(l.id) == hex) {
            return Err(bad("LoadBalancerNotFound", format!("Load balancer '{arn}' not found")));
        }
    }
    for n in &names {
        if !all.iter().any(|l| &l.name == n) {
            return Err(bad("LoadBalancerNotFound", format!("Load balancer '{n}' not found")));
        }
    }
    let arn_hexes: Vec<String> = arns.iter().filter_map(|a| parse_arn(a).map(|(_, h)| h)).collect();
    let picked: Vec<(String, Lb)> = all
        .into_iter()
        .filter(|l| (arns.is_empty() || arn_hexes.contains(&id16(l.id))) && (names.is_empty() || names.contains(&l.name)))
        .map(|l| (id16(l.id), l))
        .collect();
    let (page, next) = page_items(p, picked)?;
    let mut out = String::new();
    for lb in &page {
        out.push_str(&lb_view_xml(&state.pool, lb).await?);
    }
    Ok(format!("<LoadBalancers>{out}</LoadBalancers>{next}"))
}

pub async fn delete_load_balancer(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arn = need(p, "LoadBalancerArn")?;
    let hex = arn_hex(&arn, ArnKind::LoadBalancer, "ValidationError")?;
    // deleting what is already gone succeeds
    let Some(lb) = all_lbs(&state.pool).await?.into_iter().find(|l| id16(l.id) == hex) else {
        return Ok(String::new());
    };
    let protected = attributes(&state.pool, AttrKind::LoadBalancer, &lb.lb_type, lb.id).await?.iter().any(|(k, v)| k == "deletion_protection.enabled" && v == "true");
    if protected {
        return Err(bad("OperationNotPermitted", "Load balancer cannot be deleted because deletion protection is enabled"));
    }
    for l in listeners_of(&state.pool, lb.id).await? {
        drop_native(state, &l).await?;
        forget(&state.pool, l.id).await?;
    }
    crate::db::query("DELETE FROM elbv2_load_balancers WHERE id = ?").bind(lb.id).execute(&state.pool).await?;
    forget(&state.pool, lb.id).await?;
    Ok(String::new())
}

pub async fn describe_load_balancer_attributes(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let lb = find_lb(&state.pool, &need(p, "LoadBalancerArn")?).await?;
    Ok(attributes_xml(&attributes(&state.pool, AttrKind::LoadBalancer, &lb.lb_type, lb.id).await?))
}

pub async fn modify_load_balancer_attributes(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let lb = find_lb(&state.pool, &need(p, "LoadBalancerArn")?).await?;
    Ok(attributes_xml(&set_attributes(&state.pool, p, AttrKind::LoadBalancer, &lb.lb_type, lb.id).await?))
}

// ---- target groups -----------------------------------------------------------------------------------------------

pub async fn create_target_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = need(p, "Name")?;
    check_name(&name)?;
    let protocol = need(p, "Protocol")?.to_ascii_uppercase();
    if !matches!(protocol.as_str(), "TCP" | "UDP" | "HTTP") {
        return Err(unsupported(format!("target group protocol {protocol} is not supported; use TCP, UDP or HTTP")));
    }
    let port: i64 = need(p, "Port")?.parse().map_err(|_| validation("Port must be a number"))?;
    if !(1..=65535).contains(&port) {
        return Err(validation("Port must be 1-65535"));
    }
    let target_type = p.get("TargetType").map(String::as_str).unwrap_or("instance");
    if target_type != "instance" {
        return Err(unsupported(format!("target type '{target_type}' is not supported; targets are instances")));
    }
    if p.get("ProtocolVersion").is_some_and(|v| v != "HTTP1") {
        return Err(unsupported("only ProtocolVersion HTTP1 is supported"));
    }
    if p.get("IpAddressType").is_some_and(|v| v != "ipv4") {
        return Err(unsupported("only IpAddressType ipv4 is supported"));
    }
    let vpc_id = p.get("VpcId").cloned().unwrap_or_default();
    if !vpc_id.is_empty() {
        super::more::resolve(state, Kind::Vpc, &vpc_id, "InvalidVpcID.NotFound").await?;
    }
    let health = Health::defaults(&protocol).with_params(p)?;
    let tags = tags(p, "Tags")?;
    let exists: Option<Uuid> = crate::db::query_scalar("SELECT id FROM elbv2_target_groups WHERE name = ?").bind(&name).fetch_optional(&state.pool).await?;
    if exists.is_some() {
        return Err(bad("DuplicateTargetGroupName", format!("A target group named '{name}' already exists")));
    }
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO elbv2_target_groups (id, name, protocol, port, target_type, vpc_id, hc_enabled, hc_protocol, hc_port, hc_path, \
         hc_interval_secs, hc_timeout_secs, hc_healthy_threshold, hc_unhealthy_threshold, hc_matcher) \
         VALUES (?, ?, ?, ?, 'instance', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&name)
    .bind(&protocol)
    .bind(port)
    .bind(&vpc_id)
    .bind(health.enabled)
    .bind(&health.protocol)
    .bind(&health.port)
    .bind(&health.path)
    .bind(health.interval)
    .bind(health.timeout)
    .bind(health.healthy)
    .bind(health.unhealthy)
    .bind(&health.matcher)
    .execute(&state.pool)
    .await?;
    put_tags(&state.pool, id, &tags).await?;
    let tg = tg_by_id(&state.pool, id).await?;
    Ok(format!("<TargetGroups>{}</TargetGroups>", tg_view_xml(&state.pool, &tg).await?))
}

pub async fn describe_target_groups(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arns = list(p, "TargetGroupArns");
    let names = list(p, "Names");
    let all = all_tgs(&state.pool).await?;
    for arn in &arns {
        let hex = arn_hex(arn, ArnKind::TargetGroup, "ValidationError")?;
        if !all.iter().any(|t| id16(t.id) == hex) {
            return Err(bad("TargetGroupNotFound", format!("Target group '{arn}' not found")));
        }
    }
    for n in &names {
        if !all.iter().any(|t| &t.name == n) {
            return Err(bad("TargetGroupNotFound", format!("Target group '{n}' not found")));
        }
    }
    let of_lb: Option<Vec<Uuid>> = match p.get("LoadBalancerArn") {
        Some(a) => {
            let lb = find_lb(&state.pool, a).await?;
            Some(listeners_of(&state.pool, lb.id).await?.into_iter().map(|l| l.target_group_id).collect())
        }
        None => None,
    };
    let arn_hexes: Vec<String> = arns.iter().filter_map(|a| parse_arn(a).map(|(_, h)| h)).collect();
    let picked: Vec<(String, Tg)> = all
        .into_iter()
        .filter(|t| (arns.is_empty() || arn_hexes.contains(&id16(t.id))) && (names.is_empty() || names.contains(&t.name)) && of_lb.as_ref().is_none_or(|ids| ids.contains(&t.id)))
        .map(|t| (id16(t.id), t))
        .collect();
    let (page, next) = page_items(p, picked)?;
    let mut out = String::new();
    for tg in &page {
        out.push_str(&tg_view_xml(&state.pool, tg).await?);
    }
    Ok(format!("<TargetGroups>{out}</TargetGroups>{next}"))
}

pub async fn modify_target_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let tg = find_tg(&state.pool, &need(p, "TargetGroupArn")?).await?;
    let health = tg.health().with_params(p)?;
    crate::db::query(
        "UPDATE elbv2_target_groups SET hc_enabled = ?, hc_protocol = ?, hc_port = ?, hc_path = ?, hc_interval_secs = ?, hc_timeout_secs = ?, \
         hc_healthy_threshold = ?, hc_unhealthy_threshold = ?, hc_matcher = ? WHERE id = ?",
    )
    .bind(health.enabled)
    .bind(&health.protocol)
    .bind(&health.port)
    .bind(&health.path)
    .bind(health.interval)
    .bind(health.timeout)
    .bind(health.healthy)
    .bind(health.unhealthy)
    .bind(&health.matcher)
    .bind(tg.id)
    .execute(&state.pool)
    .await?;
    resync_group(state, tg.id).await?;
    let tg = tg_by_id(&state.pool, tg.id).await?;
    Ok(format!("<TargetGroups>{}</TargetGroups>", tg_view_xml(&state.pool, &tg).await?))
}

pub async fn delete_target_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arn = need(p, "TargetGroupArn")?;
    let hex = arn_hex(&arn, ArnKind::TargetGroup, "ValidationError")?;
    let Some(tg) = all_tgs(&state.pool).await?.into_iter().find(|t| id16(t.id) == hex) else {
        return Ok(String::new());
    };
    let used: Option<Uuid> = crate::db::query_scalar("SELECT id FROM elbv2_listeners WHERE target_group_id = ? LIMIT 1").bind(tg.id).fetch_optional(&state.pool).await?;
    if used.is_some() {
        return Err(bad("ResourceInUse", format!("Target group '{arn}' is currently in use by a listener")));
    }
    crate::db::query("DELETE FROM elbv2_target_groups WHERE id = ?").bind(tg.id).execute(&state.pool).await?;
    forget(&state.pool, tg.id).await?;
    Ok(String::new())
}

pub async fn describe_target_group_attributes(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let tg = find_tg(&state.pool, &need(p, "TargetGroupArn")?).await?;
    Ok(attributes_xml(&attributes(&state.pool, AttrKind::TargetGroup, "", tg.id).await?))
}

pub async fn modify_target_group_attributes(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let tg = find_tg(&state.pool, &need(p, "TargetGroupArn")?).await?;
    Ok(attributes_xml(&set_attributes(&state.pool, p, AttrKind::TargetGroup, "", tg.id).await?))
}

// ---- targets -----------------------------------------------------------------------------------------------------

async fn resolve_targets(state: &AppState, tg: &Tg, p: &Params) -> Result<Vec<(Uuid, i64, String)>, Ec2Error> {
    let mut out = Vec::new();
    for (id, port) in targets(p, "Targets", Some(tg.port))? {
        let vm = super::more::resolve(state, Kind::Vm, &id, "InvalidTarget").await.map_err(|e| bad("InvalidTarget", e.message))?;
        out.push((vm, port, id));
    }
    Ok(out)
}

pub async fn register_targets(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let tg = find_tg(&state.pool, &need(p, "TargetGroupArn")?).await?;
    let wanted = resolve_targets(state, &tg, p).await?;
    for (vm, port, _) in &wanted {
        crate::db::query("INSERT OR IGNORE INTO elbv2_targets (id, target_group_id, vm_id, port) VALUES (?, ?, ?, ?)")
            .bind(Uuid::new_v4())
            .bind(tg.id)
            .bind(*vm)
            .bind(*port)
            .execute(&state.pool)
            .await?;
    }
    resync_group(state, tg.id).await?;
    Ok(String::new())
}

pub async fn deregister_targets(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let tg = find_tg(&state.pool, &need(p, "TargetGroupArn")?).await?;
    let wanted = resolve_targets(state, &tg, p).await?;
    for (vm, port, _) in &wanted {
        crate::db::query("DELETE FROM elbv2_targets WHERE target_group_id = ? AND vm_id = ? AND port = ?").bind(tg.id).bind(*vm).bind(*port).execute(&state.pool).await?;
    }
    resync_group(state, tg.id).await?;
    Ok(String::new())
}

pub async fn describe_target_health(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let tg = find_tg(&state.pool, &need(p, "TargetGroupArn")?).await?;
    let filter = if indices(p, "Targets").is_empty() { None } else { Some(resolve_targets(state, &tg, p).await?) };
    let rows: Vec<(Uuid, i64)> = crate::db::query_as("SELECT vm_id, port FROM elbv2_targets WHERE target_group_id = ? ORDER BY created_at, port").bind(tg.id).fetch_all(&state.pool).await?;
    // The target group is "in use" when a listener forwards to it; the first such listener's native balancer holds the health.
    let listener = all_listeners(&state.pool).await?.into_iter().find(|l| l.target_group_id == tg.id && l.native_lb_id.is_some());
    let health = tg.health();
    let mut out = String::new();
    for (vm, port) in rows {
        if let Some(f) = &filter {
            if !f.iter().any(|(v, pt, _)| *v == vm && *pt == port) {
                continue;
            }
        }
        let id = crate::resource_ids::ec2_id(Kind::Vm, vm);
        let hc_port = if health.port == "traffic-port" { port.to_string() } else { health.port.clone() };
        let (state_name, reason, desc) = match &listener {
            None => ("unused", "Target.NotInUse", "Target group is not configured to receive traffic from the load balancer".to_string()),
            Some(l) => {
                let m: Option<(String, String)> = crate::db::query_as("SELECT health, health_detail FROM lb_members WHERE load_balancer_id = ? AND vm_id = ? AND port = ?")
                    .bind(l.native_lb_id)
                    .bind(vm)
                    .bind(port)
                    .fetch_optional(&state.pool)
                    .await?;
                match m {
                    Some((h, detail)) => target_state(&h, health.enabled, &detail),
                    None => ("initial", "Elb.RegistrationInProgress", "Target registration is in progress".to_string()),
                }
            }
        };
        out.push_str(&target_health_xml(&id, port, &hc_port, state_name, reason, &desc));
    }
    Ok(format!("<TargetHealthDescriptions>{out}</TargetHealthDescriptions>"))
}

// ---- listeners ---------------------------------------------------------------------------------------------------

/// Refuses the listener settings the native balancer cannot honour and checks protocol against balancer type and target group.
fn check_listener(lb: &Lb, tg: &Tg, protocol: &str, port: i64, p: &Params) -> Result<(), Ec2Error> {
    if !(1..=65535).contains(&port) {
        return Err(validation("Port must be 1-65535"));
    }
    if !indices(p, "Certificates").is_empty() || p.contains_key("SslPolicy") || !indices(p, "AlpnPolicy").is_empty() {
        return Err(unsupported("certificates, SSL policies and ALPN need a TLS listener, and TLS listeners are not supported"));
    }
    let allowed: &[&str] = if lb.lb_type == "application" { &["HTTP"] } else { &["TCP", "UDP"] };
    if !allowed.contains(&protocol) {
        return Err(unsupported(format!(
            "listener protocol {protocol} is not supported on a {} load balancer; use {}",
            lb.lb_type,
            allowed.join(" or ")
        )));
    }
    if tg.protocol != protocol {
        return Err(bad("IncompatibleProtocols", format!("target group '{}' speaks {} but the listener speaks {protocol}", tg.name, tg.protocol)));
    }
    Ok(())
}

pub async fn create_listener(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let lb = find_lb(&state.pool, &need(p, "LoadBalancerArn")?).await?;
    let protocol = need(p, "Protocol")?.to_ascii_uppercase();
    let port: i64 = need(p, "Port")?.parse().map_err(|_| validation("Port must be a number"))?;
    let tg = find_tg(&state.pool, &forward_target_group(p, "DefaultActions")?).await?;
    check_listener(&lb, &tg, &protocol, port, p)?;
    let tags = tags(p, "Tags")?;
    let dup: Option<Uuid> = crate::db::query_scalar("SELECT id FROM elbv2_listeners WHERE load_balancer_id = ? AND port = ?").bind(lb.id).bind(port).fetch_optional(&state.pool).await?;
    if dup.is_some() {
        return Err(bad("DuplicateListener", format!("A listener already exists on port {port} of this load balancer")));
    }
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO elbv2_listeners (id, load_balancer_id, protocol, port, target_group_id) VALUES (?, ?, ?, ?, ?)")
        .bind(id)
        .bind(lb.id)
        .bind(&protocol)
        .bind(port)
        .bind(tg.id)
        .execute(&state.pool)
        .await?;
    let l = Listener { id, load_balancer_id: lb.id, protocol, port, target_group_id: tg.id, native_lb_id: None };
    if let Err(e) = realize(state, &l).await {
        // a listener that cannot be set up (a port taken on the host) must not stay behind half made
        crate::db::query("DELETE FROM elbv2_listeners WHERE id = ?").bind(id).execute(&state.pool).await?;
        return Err(e);
    }
    put_tags(&state.pool, id, &tags).await?;
    Ok(format!("<Listeners>{}</Listeners>", listener_view_xml(&state.pool, &l).await?))
}

pub async fn describe_listeners(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arns = list(p, "ListenerArns");
    let picked: Vec<Listener> = if let Some(a) = p.get("LoadBalancerArn") {
        let lb = find_lb(&state.pool, a).await?;
        listeners_of(&state.pool, lb.id).await?
    } else if !arns.is_empty() {
        let mut out = Vec::new();
        for a in &arns {
            out.push(find_listener(&state.pool, a).await?);
        }
        out
    } else {
        return Err(validation("Either LoadBalancerArn or ListenerArns must be specified"));
    };
    let (page, next) = page_items(p, picked.into_iter().map(|l| (id16(l.id), l)).collect())?;
    let mut out = String::new();
    for l in &page {
        out.push_str(&listener_view_xml(&state.pool, l).await?);
    }
    Ok(format!("<Listeners>{out}</Listeners>{next}"))
}

pub async fn modify_listener(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let l = find_listener(&state.pool, &need(p, "ListenerArn")?).await?;
    let lb = lb_by_id(&state.pool, l.load_balancer_id).await?;
    let protocol = p.get("Protocol").map(|v| v.to_ascii_uppercase()).unwrap_or_else(|| l.protocol.clone());
    let port: i64 = match p.get("Port") {
        Some(v) => v.parse().map_err(|_| validation("Port must be a number"))?,
        None => l.port,
    };
    let tg_id = if indices(p, "DefaultActions").is_empty() { l.target_group_id } else { find_tg(&state.pool, &forward_target_group(p, "DefaultActions")?).await?.id };
    let tg = tg_by_id(&state.pool, tg_id).await?;
    check_listener(&lb, &tg, &protocol, port, p)?;
    if port != l.port {
        let dup: Option<Uuid> = crate::db::query_scalar("SELECT id FROM elbv2_listeners WHERE load_balancer_id = ? AND port = ? AND id <> ?").bind(lb.id).bind(port).bind(l.id).fetch_optional(&state.pool).await?;
        if dup.is_some() {
            return Err(bad("DuplicateListener", format!("A listener already exists on port {port} of this load balancer")));
        }
    }
    let mut next = Listener { protocol: protocol.clone(), port, target_group_id: tg.id, ..l.clone() };
    if protocol != l.protocol || port != l.port {
        // the host-side rule is keyed by protocol and port: take the old one down and make a new one
        drop_native(state, &l).await?;
        next.native_lb_id = None;
    }
    crate::db::query("UPDATE elbv2_listeners SET protocol = ?, port = ?, target_group_id = ?, native_lb_id = ? WHERE id = ?")
        .bind(&next.protocol)
        .bind(next.port)
        .bind(next.target_group_id)
        .bind(next.native_lb_id)
        .bind(l.id)
        .execute(&state.pool)
        .await?;
    realize(state, &next).await?;
    Ok(format!("<Listeners>{}</Listeners>", listener_view_xml(&state.pool, &next).await?))
}

pub async fn delete_listener(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arn = need(p, "ListenerArn")?;
    let hex = arn_hex(&arn, ArnKind::Listener, "ValidationError")?;
    let Some(l) = all_listeners(&state.pool).await?.into_iter().find(|l| id16(l.id) == hex) else {
        return Ok(String::new());
    };
    drop_native(state, &l).await?;
    crate::db::query("DELETE FROM elbv2_listeners WHERE id = ?").bind(l.id).execute(&state.pool).await?;
    forget(&state.pool, l.id).await?;
    Ok(String::new())
}

pub async fn describe_listener_attributes(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let l = find_listener(&state.pool, &need(p, "ListenerArn")?).await?;
    Ok(attributes_xml(&attributes(&state.pool, AttrKind::Listener, "", l.id).await?))
}

pub async fn modify_listener_attributes(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let l = find_listener(&state.pool, &need(p, "ListenerArn")?).await?;
    Ok(attributes_xml(&set_attributes(&state.pool, p, AttrKind::Listener, "", l.id).await?))
}

// ---- rules: only each listener's default rule exists -------------------------------------------------------------

pub async fn describe_rules(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arns = list(p, "RuleArns");
    let picked: Vec<Listener> = if let Some(a) = p.get("ListenerArn") {
        vec![find_listener(&state.pool, a).await?]
    } else if !arns.is_empty() {
        let mut out = Vec::new();
        for a in &arns {
            out.push(find_rule_listener(&state.pool, a).await?);
        }
        out
    } else {
        return Err(validation("Either ListenerArn or RuleArns must be specified"));
    };
    let (page, next) = page_items(p, picked.into_iter().map(|l| (id16(l.id), l)).collect())?;
    let mut out = String::new();
    for l in &page {
        out.push_str(&rule_view_xml(&state.pool, l).await?);
    }
    Ok(format!("<Rules>{out}</Rules>{next}"))
}

pub async fn create_rule(_state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    Err(unsupported(
        "listener rules are not supported: Machina's balancer is a layer-4 forwarder and cannot route on path, host, header, query string or source; \
         use the listener's default action",
    ))
}

pub async fn modify_rule(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let l = find_rule_listener(&state.pool, &need(p, "RuleArn")?).await?;
    if !indices(p, "Conditions").is_empty() {
        return Err(unsupported("rule conditions are not supported; the default rule has none"));
    }
    let lb = lb_by_id(&state.pool, l.load_balancer_id).await?;
    let tg = find_tg(&state.pool, &forward_target_group(p, "Actions")?).await?;
    check_listener(&lb, &tg, &l.protocol, l.port, &Params::new())?;
    crate::db::query("UPDATE elbv2_listeners SET target_group_id = ? WHERE id = ?").bind(tg.id).bind(l.id).execute(&state.pool).await?;
    let fresh = Listener { target_group_id: tg.id, ..l };
    realize(state, &fresh).await?;
    Ok(format!("<Rules>{}</Rules>", rule_view_xml(&state.pool, &fresh).await?))
}

pub async fn delete_rule(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    find_rule_listener(&state.pool, &need(p, "RuleArn")?).await?;
    Err(bad("OperationNotPermitted", "The default rule cannot be deleted; delete the listener instead"))
}

pub async fn set_rule_priorities(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    if indices(p, "RulePriorities").is_empty() {
        return Err(validation("RulePriorities is required"));
    }
    for i in indices(p, "RulePriorities") {
        let arn = p.get(&format!("RulePriorities.member.{i}.RuleArn")).ok_or_else(|| validation("every entry needs a RuleArn"))?;
        find_rule_listener(&state.pool, arn).await?;
    }
    Err(validation("the default rule has no priority; there are no other rules"))
}

// ---- tags --------------------------------------------------------------------------------------------------------

async fn taggable(pool: &DbPool, arn: &str) -> Result<Uuid, Ec2Error> {
    match parse_arn(arn) {
        Some((ArnKind::LoadBalancer, _)) => Ok(find_lb(pool, arn).await?.id),
        Some((ArnKind::TargetGroup, _)) => Ok(find_tg(pool, arn).await?.id),
        Some((ArnKind::Listener, _)) => Ok(find_listener(pool, arn).await?.id),
        Some((ArnKind::Rule, _)) => Err(unsupported("the default rule cannot be tagged; tag the listener")),
        None => Err(validation(format!("'{arn}' is not a valid ARN"))),
    }
}

pub async fn add_tags(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arns = list(p, "ResourceArns");
    if arns.is_empty() {
        return Err(validation("ResourceArns is required"));
    }
    let new = tags(p, "Tags")?;
    if new.is_empty() {
        return Err(validation("Tags is required"));
    }
    let mut ids = Vec::new();
    for a in &arns {
        ids.push(taggable(&state.pool, a).await?);
    }
    for id in ids {
        put_tags(&state.pool, id, &new).await?;
        if tags_of(&state.pool, id).await?.len() > 50 {
            return Err(bad("TooManyTags", "a resource has at most 50 tags"));
        }
    }
    Ok(String::new())
}

pub async fn remove_tags(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arns = list(p, "ResourceArns");
    let keys = list(p, "TagKeys");
    if arns.is_empty() || keys.is_empty() {
        return Err(validation("ResourceArns and TagKeys are required"));
    }
    for a in &arns {
        let id = taggable(&state.pool, a).await?;
        for k in &keys {
            crate::db::query("DELETE FROM elbv2_tags WHERE resource_id = ? AND tag_key = ?").bind(id).bind(k).execute(&state.pool).await?;
        }
    }
    Ok(String::new())
}

pub async fn describe_tags(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let arns = list(p, "ResourceArns");
    if arns.is_empty() {
        return Err(validation("ResourceArns is required"));
    }
    let mut out = String::new();
    for a in &arns {
        let id = taggable(&state.pool, a).await?;
        out.push_str(&format!("<member><ResourceArn>{}</ResourceArn>{}</member>", xml_escape(a), tags_xml(&tags_of(&state.pool, id).await?)));
    }
    Ok(format!("<TagDescriptions>{out}</TagDescriptions>"))
}

// ---- static answers ----------------------------------------------------------------------------------------------

/// TLS policies are listed so clients that read them find something, but a TLS listener cannot be created.
pub async fn describe_ssl_policies(_state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let known = [("ELBSecurityPolicy-TLS13-1-2-2021-06", "TLSv1.3", "TLS_AES_128_GCM_SHA256"), ("ELBSecurityPolicy-TLS-1-2-2017-01", "TLSv1.2", "ECDHE-RSA-AES128-GCM-SHA256")];
    let wanted = list(p, "Names");
    let mut out = String::new();
    for (name, proto, cipher) in known {
        if wanted.is_empty() || wanted.iter().any(|w| w == name) {
            out.push_str(&format!(
                "<member><Name>{name}</Name><SslProtocols><member>{proto}</member></SslProtocols>\
                 <Ciphers><member><Name>{cipher}</Name><Priority>1</Priority></member></Ciphers></member>"
            ));
        }
    }
    Ok(format!("<SslPolicies>{out}</SslPolicies>"))
}

/// Fixed limits (Machina has no per-account quotas on these objects).
pub async fn describe_account_limits(_state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let limits = [("application-load-balancers", 50), ("network-load-balancers", 50), ("target-groups", 3000), ("listeners-per-application-load-balancer", 50), ("listeners-per-network-load-balancer", 50), ("targets-per-target-group", 1000)];
    let items: String = limits.iter().map(|(n, m)| format!("<member><Name>{n}</Name><Max>{m}</Max></member>")).collect();
    Ok(format!("<Limits>{items}</Limits>"))
}

/// The ELBv2 action table; `None` for an action this service does not have.
pub async fn dispatch(state: &AppState, actor: &AuthUser, p: &Params, action: &str) -> Option<Result<String, Ec2Error>> {
    Some(match action {
        "CreateLoadBalancer" => create_load_balancer(state, actor, p).await,
        "DescribeLoadBalancers" => describe_load_balancers(state, actor, p).await,
        "DeleteLoadBalancer" => delete_load_balancer(state, actor, p).await,
        "DescribeLoadBalancerAttributes" => describe_load_balancer_attributes(state, actor, p).await,
        "ModifyLoadBalancerAttributes" => modify_load_balancer_attributes(state, actor, p).await,
        "CreateTargetGroup" => create_target_group(state, actor, p).await,
        "DescribeTargetGroups" => describe_target_groups(state, actor, p).await,
        "ModifyTargetGroup" => modify_target_group(state, actor, p).await,
        "DeleteTargetGroup" => delete_target_group(state, actor, p).await,
        "DescribeTargetGroupAttributes" => describe_target_group_attributes(state, actor, p).await,
        "ModifyTargetGroupAttributes" => modify_target_group_attributes(state, actor, p).await,
        "RegisterTargets" => register_targets(state, actor, p).await,
        "DeregisterTargets" => deregister_targets(state, actor, p).await,
        "DescribeTargetHealth" => describe_target_health(state, actor, p).await,
        "CreateListener" => create_listener(state, actor, p).await,
        "DescribeListeners" => describe_listeners(state, actor, p).await,
        "ModifyListener" => modify_listener(state, actor, p).await,
        "DeleteListener" => delete_listener(state, actor, p).await,
        "DescribeListenerAttributes" => describe_listener_attributes(state, actor, p).await,
        "ModifyListenerAttributes" => modify_listener_attributes(state, actor, p).await,
        "DescribeRules" => describe_rules(state, actor, p).await,
        "CreateRule" => create_rule(state, actor, p).await,
        "ModifyRule" => modify_rule(state, actor, p).await,
        "DeleteRule" => delete_rule(state, actor, p).await,
        "SetRulePriorities" => set_rule_priorities(state, actor, p).await,
        "AddTags" => add_tags(state, actor, p).await,
        "RemoveTags" => remove_tags(state, actor, p).await,
        "DescribeTags" => describe_tags(state, actor, p).await,
        "DescribeSSLPolicies" => describe_ssl_policies(state, actor, p).await,
        "DescribeAccountLimits" => describe_account_limits(state, actor, p).await,
        _ => return None,
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_ids::{ec2_id, Kind};

    fn q(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn admin() -> AuthUser {
        AuthUser { username: "u".into(), role: "admin".into(), auth_source: None }
    }

    /// A state with one online host and one VM; returns the VM's EC2 id.
    async fn world() -> (AppState, String) {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        crate::engine::test_support::seed_host(&state.pool, Uuid::new_v4()).await;
        let vm = Uuid::new_v4();
        crate::db::query("INSERT INTO vms (id, name) VALUES (?, 'web-1')").bind(vm).execute(&state.pool).await.unwrap();
        (state, ec2_id(Kind::Vm, vm))
    }

    /// The text between `<tag>` and `</tag>`, first occurrence.
    fn between<'a>(xml: &'a str, tag: &str) -> &'a str {
        let open = format!("<{tag}>");
        let start = xml.find(&open).unwrap_or_else(|| panic!("no <{tag}> in {xml}")) + open.len();
        &xml[start..start + xml[start..].find(&format!("</{tag}>")).unwrap()]
    }

    async fn nlb(state: &AppState, name: &str) -> String {
        let x = create_load_balancer(state, &admin(), &q(&[("Name", name), ("Type", "network")])).await.unwrap();
        between(&x, "LoadBalancerArn").to_string()
    }

    async fn tcp_group(state: &AppState, name: &str, port: &str) -> String {
        let x = create_target_group(state, &admin(), &q(&[("Name", name), ("Protocol", "TCP"), ("Port", port)])).await.unwrap();
        between(&x, "TargetGroupArn").to_string()
    }

    async fn listen(state: &AppState, lb: &str, protocol: &str, port: &str, tg: &str) -> Result<String, Ec2Error> {
        create_listener(
            state,
            &admin(),
            &q(&[("LoadBalancerArn", lb), ("Protocol", protocol), ("Port", port), ("DefaultActions.member.1.Type", "forward"), ("DefaultActions.member.1.TargetGroupArn", tg)]),
        )
        .await
    }

    async fn natives(state: &AppState) -> Vec<(String, i64)> {
        crate::db::query_as("SELECT protocol, listener_port FROM load_balancers ORDER BY listener_port").fetch_all(&state.pool).await.unwrap()
    }

    async fn member_count(state: &AppState) -> i64 {
        crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM lb_members").fetch_one(&state.pool).await.unwrap()
    }

    #[tokio::test]
    async fn a_network_balancer_forwards_through_a_listener_to_a_native_balancer() {
        let (state, vm) = world().await;
        let a = admin();
        let lb = nlb(&state, "web").await;
        let tg = tcp_group(&state, "pool", "80").await;

        // nothing is realised until a listener exists
        assert!(natives(&state).await.is_empty());
        let reg = q(&[("TargetGroupArn", &tg), ("Targets.member.1.Id", &vm)]);
        assert_eq!(register_targets(&state, &a, &reg).await.unwrap(), "");
        assert_eq!(member_count(&state).await, 0);
        // a registered target of an unattached group is "unused"
        let h = describe_target_health(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap();
        assert!(h.contains("<State>unused</State><Reason>Target.NotInUse</Reason>") && h.contains(&format!("<Id>{vm}</Id>")) && h.contains("<Port>80</Port>"), "{h}");

        let l = listen(&state, &lb, "TCP", "80", &tg).await.unwrap();
        assert!(l.contains("<Port>80</Port>") && l.contains("<Protocol>TCP</Protocol>") && l.contains(&format!("<TargetGroupArn>{tg}</TargetGroupArn>")), "{l}");
        assert_eq!(natives(&state).await, [("tcp".to_string(), 80)]);
        assert_eq!(member_count(&state).await, 1);
        let h = describe_target_health(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap();
        assert!(h.contains("<State>initial</State><Reason>Elb.InitialHealthChecking</Reason>"), "{h}");
        // the group now knows its balancer
        let g = describe_target_groups(&state, &a, &q(&[("Names.member.1", "pool")])).await.unwrap();
        assert!(g.contains(&format!("<LoadBalancerArns><member>{lb}</member></LoadBalancerArns>")), "{g}");

        // health comes from the native member
        crate::db::query("UPDATE lb_members SET health = 'healthy'").execute(&state.pool).await.unwrap();
        let h = describe_target_health(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap();
        assert!(h.contains("<State>healthy</State>") && !h.contains("<Reason>"), "{h}");
        crate::db::query("UPDATE lb_members SET health = 'unhealthy', health_detail = 'connection refused'").execute(&state.pool).await.unwrap();
        let h = describe_target_health(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap();
        assert!(h.contains("<State>unhealthy</State><Reason>Target.FailedHealthChecks</Reason><Description>connection refused</Description>"), "{h}");

        // targets follow the group, and a second registration is a no-op
        assert_eq!(register_targets(&state, &a, &reg).await.unwrap(), "");
        assert_eq!(member_count(&state).await, 1);
        assert_eq!(deregister_targets(&state, &a, &reg).await.unwrap(), "");
        assert_eq!(member_count(&state).await, 0);

        // the whole thing goes away in order: listener (and its native balancer), group, balancer
        let listener_arn = between(&l, "ListenerArn").to_string();
        assert_eq!(delete_target_group(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap_err().code, "ResourceInUse");
        assert_eq!(delete_listener(&state, &a, &q(&[("ListenerArn", &listener_arn)])).await.unwrap(), "");
        assert!(natives(&state).await.is_empty());
        assert_eq!(delete_target_group(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap(), "");
        assert_eq!(delete_load_balancer(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap(), "");
        // deleting what is gone succeeds
        assert_eq!(delete_load_balancer(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap(), "");
        let none = describe_load_balancers(&state, &a, &q(&[])).await.unwrap();
        assert_eq!(none, "<LoadBalancers></LoadBalancers>");
    }

    #[tokio::test]
    async fn deleting_a_balancer_takes_its_listeners_and_native_balancers_with_it() {
        let (state, _) = world().await;
        let a = admin();
        let lb = nlb(&state, "web").await;
        let tg = tcp_group(&state, "pool", "80").await;
        listen(&state, &lb, "TCP", "80", &tg).await.unwrap();
        listen(&state, &lb, "TCP", "8080", &tg).await.unwrap();
        assert_eq!(natives(&state).await.len(), 2);
        delete_load_balancer(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap();
        assert!(natives(&state).await.is_empty());
        let left: i64 = crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM elbv2_listeners").fetch_one(&state.pool).await.unwrap();
        assert_eq!(left, 0);
        // the group is free again
        assert_eq!(delete_target_group(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap(), "");
    }

    #[tokio::test]
    async fn what_the_layer_4_balancer_cannot_do_is_refused_by_name() {
        let (state, _) = world().await;
        let a = admin();
        let lb = nlb(&state, "web").await;
        let tg = tcp_group(&state, "pool", "80").await;
        let http = create_target_group(&state, &a, &q(&[("Name", "h"), ("Protocol", "HTTP"), ("Port", "80")])).await.unwrap();
        let http = between(&http, "TargetGroupArn").to_string();

        for (what, params, code) in [
            ("security groups", q(&[("Name", "x"), ("SecurityGroups.member.1", "sg-1")]), "UnsupportedOperation"),
            ("gateway type", q(&[("Name", "x"), ("Type", "gateway")]), "UnsupportedOperation"),
            ("dualstack", q(&[("Name", "x"), ("IpAddressType", "dualstack")]), "UnsupportedOperation"),
            ("elastic ip", q(&[("Name", "x"), ("SubnetMappings.member.1.SubnetId", "subnet-1"), ("SubnetMappings.member.1.AllocationId", "eipalloc-1")]), "UnsupportedOperation"),
            ("scheme", q(&[("Name", "x"), ("Scheme", "public")]), "ValidationError"),
            ("name", q(&[("Name", "-x")]), "ValidationError"),
            ("no name", q(&[]), "ValidationError"),
        ] {
            assert_eq!(create_load_balancer(&state, &a, &params).await.unwrap_err().code, code, "{what}");
        }
        for (what, params, code) in [
            ("https target group", q(&[("Name", "x"), ("Protocol", "HTTPS"), ("Port", "443")]), "UnsupportedOperation"),
            ("ip targets", q(&[("Name", "x"), ("Protocol", "TCP"), ("Port", "80"), ("TargetType", "ip")]), "UnsupportedOperation"),
            ("lambda targets", q(&[("Name", "x"), ("Protocol", "TCP"), ("Port", "80"), ("TargetType", "lambda")]), "UnsupportedOperation"),
            ("grpc", q(&[("Name", "x"), ("Protocol", "HTTP"), ("Port", "80"), ("ProtocolVersion", "GRPC")]), "UnsupportedOperation"),
            ("port", q(&[("Name", "x"), ("Protocol", "TCP"), ("Port", "0")]), "ValidationError"),
            ("duplicate", q(&[("Name", "pool"), ("Protocol", "TCP"), ("Port", "80")]), "DuplicateTargetGroupName"),
        ] {
            assert_eq!(create_target_group(&state, &a, &params).await.unwrap_err().code, code, "{what}");
        }
        assert_eq!(create_load_balancer(&state, &a, &q(&[("Name", "web")])).await.unwrap_err().code, "DuplicateLoadBalancerName");

        // listeners
        let cert = q(&[("LoadBalancerArn", &lb), ("Protocol", "TCP"), ("Port", "80"), ("Certificates.member.1.CertificateArn", "arn:x"), ("DefaultActions.member.1.Type", "forward"), ("DefaultActions.member.1.TargetGroupArn", &tg)]);
        assert_eq!(create_listener(&state, &a, &cert).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(listen(&state, &lb, "TLS", "443", &tg).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(listen(&state, &lb, "HTTPS", "443", &tg).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(listen(&state, &lb, "HTTP", "80", &http).await.unwrap_err().code, "UnsupportedOperation", "HTTP is not a network balancer protocol");
        assert_eq!(listen(&state, &lb, "UDP", "80", &tg).await.unwrap_err().code, "IncompatibleProtocols");
        assert_eq!(listen(&state, &lb, "TCP", "0", &tg).await.unwrap_err().code, "ValidationError");
        let redirect = q(&[("LoadBalancerArn", &lb), ("Protocol", "TCP"), ("Port", "80"), ("DefaultActions.member.1.Type", "redirect")]);
        let e = create_listener(&state, &a, &redirect).await.unwrap_err();
        assert!(e.code == "UnsupportedOperation" && e.message.contains("redirect"), "{e:?}");
        assert!(natives(&state).await.is_empty(), "a refused listener leaves nothing behind");

        // rules
        let l = listen(&state, &lb, "TCP", "80", &tg).await.unwrap();
        let listener = between(&l, "ListenerArn").to_string();
        assert_eq!(create_rule(&state, &a, &q(&[("ListenerArn", &listener)])).await.unwrap_err().code, "UnsupportedOperation");
        let rules = describe_rules(&state, &a, &q(&[("ListenerArn", &listener)])).await.unwrap();
        assert!(rules.contains("<Priority>default</Priority>") && rules.contains("<IsDefault>true</IsDefault>"), "{rules}");
        let rule = between(&rules, "RuleArn").to_string();
        assert_eq!(delete_rule(&state, &a, &q(&[("RuleArn", &rule)])).await.unwrap_err().code, "OperationNotPermitted");
        let prio = q(&[("RulePriorities.member.1.RuleArn", &rule), ("RulePriorities.member.1.Priority", "5")]);
        assert_eq!(set_rule_priorities(&state, &a, &prio).await.unwrap_err().code, "ValidationError");
        let cond = q(&[("RuleArn", &rule), ("Conditions.member.1.Field", "path-pattern"), ("Actions.member.1.Type", "forward"), ("Actions.member.1.TargetGroupArn", &tg)]);
        assert_eq!(modify_rule(&state, &a, &cond).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(describe_rules(&state, &a, &q(&[])).await.unwrap_err().code, "ValidationError");
        // a second listener on the same port, and an unknown ARN
        assert_eq!(listen(&state, &lb, "TCP", "80", &tg).await.unwrap_err().code, "DuplicateListener");
        assert_eq!(describe_listeners(&state, &a, &q(&[("ListenerArns.member.1", &lb)])).await.unwrap_err().code, "ValidationError");
        assert_eq!(describe_target_groups(&state, &a, &q(&[("Names.member.1", "nope")])).await.unwrap_err().code, "TargetGroupNotFound");
        assert_eq!(register_targets(&state, &a, &q(&[("TargetGroupArn", &tg), ("Targets.member.1.Id", "i-0123456789abcdef0")])).await.unwrap_err().code, "InvalidTarget");
    }

    #[tokio::test]
    async fn two_balancers_cannot_share_a_listener_port_on_one_host() {
        let (state, _) = world().await;
        let a = admin();
        let one = nlb(&state, "one").await;
        let two = nlb(&state, "two").await;
        let tg = tcp_group(&state, "pool", "80").await;
        listen(&state, &one, "TCP", "80", &tg).await.unwrap();
        let e = listen(&state, &two, "TCP", "80", &tg).await.unwrap_err();
        assert!(e.code == "ValidationError" && e.message.contains("already served on this balancer's host"), "{e:?}");
        // the loser left no listener behind, the winner is untouched
        let ls = describe_listeners(&state, &a, &q(&[("LoadBalancerArn", &two)])).await.unwrap();
        assert_eq!(ls, "<Listeners></Listeners>");
        assert_eq!(natives(&state).await, [("tcp".to_string(), 80)]);
        // another port, or another protocol, is fine
        listen(&state, &two, "TCP", "81", &tg).await.unwrap();
        let udp = create_target_group(&state, &a, &q(&[("Name", "dns"), ("Protocol", "UDP"), ("Port", "53"), ("HealthCheckPort", "53")])).await.unwrap();
        listen(&state, &two, "UDP", "80", between(&udp, "TargetGroupArn")).await.unwrap();
        let mut got = natives(&state).await;
        got.sort();
        assert_eq!(got, [("tcp".to_string(), 80), ("tcp".to_string(), 81), ("udp".to_string(), 80)]);
    }

    #[tokio::test]
    async fn an_application_balancer_takes_http_and_the_target_groups_health_check_reaches_the_native_one() {
        let (state, vm) = world().await;
        let a = admin();
        let lb = create_load_balancer(&state, &a, &q(&[("Name", "site")])).await.unwrap();
        assert!(lb.contains("<Type>application</Type>"), "the SDK default type is application: {lb}");
        let lb = between(&lb, "LoadBalancerArn").to_string();
        assert!(lb.contains("loadbalancer/app/site/"));
        let tg = create_target_group(
            &state,
            &a,
            &q(&[("Name", "web"), ("Protocol", "HTTP"), ("Port", "8080"), ("HealthCheckPath", "/up"), ("HealthCheckIntervalSeconds", "10"), ("HealthCheckTimeoutSeconds", "4"), ("HealthyThresholdCount", "2"), ("Matcher.HttpCode", "200-399")]),
        )
        .await
        .unwrap();
        assert!(tg.contains("<HealthCheckPath>/up</HealthCheckPath>") && tg.contains("<Matcher><HttpCode>200-399</HttpCode></Matcher>"), "{tg}");
        let tg = between(&tg, "TargetGroupArn").to_string();
        // a target without a Port gets the group's port
        register_targets(&state, &a, &q(&[("TargetGroupArn", &tg), ("Targets.member.1.Id", &vm)])).await.unwrap();
        listen(&state, &lb, "HTTP", "80", &tg).await.unwrap();

        let row: (String, i64, Option<i64>, String, i64, i64, i64) =
            crate::db::query_as("SELECT protocol, listener_port, hc_port, hc_path, hc_interval_secs, hc_timeout_secs, hc_healthy_threshold FROM load_balancers").fetch_one(&state.pool).await.unwrap();
        assert_eq!(row, ("tcp".to_string(), 80, None, "/up".to_string(), 10, 4, 2));
        let hc: String = crate::db::query_scalar("SELECT hc_protocol FROM load_balancers").fetch_one(&state.pool).await.unwrap();
        assert_eq!(hc, "http");
        let member_port: i64 = crate::db::query_scalar::<_, i64>("SELECT port FROM lb_members").fetch_one(&state.pool).await.unwrap();
        assert_eq!(member_port, 8080);

        // changing the group's check changes the native one and resets what was learned about the members
        crate::db::query("UPDATE lb_members SET health = 'healthy'").execute(&state.pool).await.unwrap();
        modify_target_group(&state, &a, &q(&[("TargetGroupArn", &tg), ("HealthCheckIntervalSeconds", "20"), ("HealthCheckPort", "9000")])).await.unwrap();
        let row: (Option<i64>, i64) = crate::db::query_as("SELECT hc_port, hc_interval_secs FROM load_balancers").fetch_one(&state.pool).await.unwrap();
        assert_eq!(row, (Some(9000), 20));
        let health: String = crate::db::query_scalar("SELECT health FROM lb_members").fetch_one(&state.pool).await.unwrap();
        assert_eq!(health, "unknown");
        // a modification that changes nothing leaves the learned health alone
        crate::db::query("UPDATE lb_members SET health = 'healthy'").execute(&state.pool).await.unwrap();
        modify_target_group(&state, &a, &q(&[("TargetGroupArn", &tg), ("HealthCheckIntervalSeconds", "20")])).await.unwrap();
        let health: String = crate::db::query_scalar("SELECT health FROM lb_members").fetch_one(&state.pool).await.unwrap();
        assert_eq!(health, "healthy");
        // a check the probe cannot run is refused and nothing changes
        let e = modify_target_group(&state, &a, &q(&[("TargetGroupArn", &tg), ("HealthCheckProtocol", "HTTPS")])).await.unwrap_err();
        assert_eq!(e.code, "UnsupportedOperation");
        // with the check off the native check is off and targets count as healthy
        modify_target_group(&state, &a, &q(&[("TargetGroupArn", &tg), ("HealthCheckEnabled", "false")])).await.unwrap();
        let hc: String = crate::db::query_scalar("SELECT hc_protocol FROM load_balancers").fetch_one(&state.pool).await.unwrap();
        assert_eq!(hc, "none");
        let h = describe_target_health(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap();
        assert!(h.contains("<State>healthy</State>"), "{h}");
    }

    #[tokio::test]
    async fn modifying_a_listener_moves_its_native_balancer() {
        let (state, vm) = world().await;
        let a = admin();
        let lb = nlb(&state, "web").await;
        let one = tcp_group(&state, "one", "80").await;
        let two = tcp_group(&state, "two", "90").await;
        register_targets(&state, &a, &q(&[("TargetGroupArn", &one), ("Targets.member.1.Id", &vm)])).await.unwrap();
        let l = listen(&state, &lb, "TCP", "80", &one).await.unwrap();
        let arn = between(&l, "ListenerArn").to_string();
        assert_eq!(member_count(&state).await, 1);

        // another target group: same native balancer, no members (the new group has none)
        let m = modify_listener(&state, &a, &q(&[("ListenerArn", &arn), ("DefaultActions.member.1.Type", "forward"), ("DefaultActions.member.1.TargetGroupArn", &two)])).await.unwrap();
        assert!(m.contains(&format!("<TargetGroupArn>{two}</TargetGroupArn>")), "{m}");
        assert_eq!(natives(&state).await, [("tcp".to_string(), 80)]);
        assert_eq!(member_count(&state).await, 0);

        // another port: the old native balancer goes, a new one appears
        modify_listener(&state, &a, &q(&[("ListenerArn", &arn), ("Port", "8443")])).await.unwrap();
        assert_eq!(natives(&state).await, [("tcp".to_string(), 8443)]);
        let shown = describe_listeners(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap();
        assert!(shown.contains("<Port>8443</Port>") && !shown.contains("<Port>80</Port>"), "{shown}");
        // the default rule's action is changed through ModifyRule too
        let rule = between(&describe_rules(&state, &a, &q(&[("ListenerArn", &arn)])).await.unwrap(), "RuleArn").to_string();
        modify_rule(&state, &a, &q(&[("RuleArn", &rule), ("Actions.member.1.Type", "forward"), ("Actions.member.1.TargetGroupArn", &one)])).await.unwrap();
        assert_eq!(member_count(&state).await, 1);
        // a protocol the group does not speak is refused and nothing moves
        let e = modify_listener(&state, &a, &q(&[("ListenerArn", &arn), ("Protocol", "UDP")])).await.unwrap_err();
        assert_eq!(e.code, "IncompatibleProtocols");
        assert_eq!(natives(&state).await, [("tcp".to_string(), 8443)]);
    }

    #[tokio::test]
    async fn attributes_default_store_and_refuse() {
        let (state, _) = world().await;
        let a = admin();
        let lb = nlb(&state, "web").await;
        let x = describe_load_balancer_attributes(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap();
        assert!(x.contains("<Key>deletion_protection.enabled</Key><Value>false</Value>") && !x.contains("idle_timeout"), "{x}");
        let set = |k: &str, v: &str| q(&[("LoadBalancerArn", &lb), ("Attributes.member.1.Key", k), ("Attributes.member.1.Value", v)]);
        modify_load_balancer_attributes(&state, &a, &set("load_balancing.cross_zone.enabled", "true")).await.unwrap();
        let x = describe_load_balancer_attributes(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap();
        assert!(x.contains("<Key>load_balancing.cross_zone.enabled</Key><Value>true</Value>"), "{x}");
        assert_eq!(modify_load_balancer_attributes(&state, &a, &set("access_logs.s3.enabled", "true")).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(modify_load_balancer_attributes(&state, &a, &set("made.up", "1")).await.unwrap_err().code, "ValidationError");
        assert_eq!(modify_load_balancer_attributes(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap_err().code, "ValidationError");

        // deletion protection is enforced
        modify_load_balancer_attributes(&state, &a, &set("deletion_protection.enabled", "true")).await.unwrap();
        let e = delete_load_balancer(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap_err();
        assert_eq!(e.code, "OperationNotPermitted");
        modify_load_balancer_attributes(&state, &a, &set("deletion_protection.enabled", "false")).await.unwrap();
        delete_load_balancer(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap();

        let tg = tcp_group(&state, "pool", "80").await;
        let tset = |k: &str, v: &str| q(&[("TargetGroupArn", &tg), ("Attributes.member.1.Key", k), ("Attributes.member.1.Value", v)]);
        modify_target_group_attributes(&state, &a, &tset("deregistration_delay.timeout_seconds", "30")).await.unwrap();
        let x = describe_target_group_attributes(&state, &a, &q(&[("TargetGroupArn", &tg)])).await.unwrap();
        assert!(x.contains("<Key>deregistration_delay.timeout_seconds</Key><Value>30</Value>"), "{x}");
        assert_eq!(modify_target_group_attributes(&state, &a, &tset("stickiness.enabled", "true")).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(modify_target_group_attributes(&state, &a, &tset("proxy_protocol_v2.enabled", "true")).await.unwrap_err().code, "UnsupportedOperation");
    }

    #[tokio::test]
    async fn tags_attach_list_and_go_with_their_resource() {
        let (state, _) = world().await;
        let a = admin();
        let made = create_load_balancer(&state, &a, &q(&[("Name", "web"), ("Type", "network"), ("Tags.member.1.Key", "env"), ("Tags.member.1.Value", "prod")])).await.unwrap();
        let lb = between(&made, "LoadBalancerArn").to_string();
        let tg = tcp_group(&state, "pool", "80").await;
        let l = listen(&state, &lb, "TCP", "80", &tg).await.unwrap();
        let listener = between(&l, "ListenerArn").to_string();

        let add = q(&[("ResourceArns.member.1", &lb), ("ResourceArns.member.2", &tg), ("ResourceArns.member.3", &listener), ("Tags.member.1.Key", "team"), ("Tags.member.1.Value", "infra")]);
        assert_eq!(add_tags(&state, &a, &add).await.unwrap(), "");
        let got = describe_tags(&state, &a, &q(&[("ResourceArns.member.1", &lb), ("ResourceArns.member.2", &tg)])).await.unwrap();
        assert!(
            got.contains(&format!("<ResourceArn>{lb}</ResourceArn><Tags><member><Key>env</Key><Value>prod</Value></member><member><Key>team</Key><Value>infra</Value></member></Tags>")),
            "{got}"
        );
        assert!(got.contains(&format!("<ResourceArn>{tg}</ResourceArn><Tags><member><Key>team</Key><Value>infra</Value></member></Tags>")), "{got}");
        remove_tags(&state, &a, &q(&[("ResourceArns.member.1", &lb), ("TagKeys.member.1", "env")])).await.unwrap();
        let got = describe_tags(&state, &a, &q(&[("ResourceArns.member.1", &lb)])).await.unwrap();
        assert!(!got.contains("env") && got.contains("team"), "{got}");

        assert_eq!(describe_tags(&state, &a, &q(&[("ResourceArns.member.1", "not-an-arn")])).await.unwrap_err().code, "ValidationError");
        let rule_of_listener = between(&describe_rules(&state, &a, &q(&[("ListenerArn", &listener)])).await.unwrap(), "RuleArn").to_string();
        assert_eq!(describe_tags(&state, &a, &q(&[("ResourceArns.member.1", &rule_of_listener)])).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(add_tags(&state, &a, &q(&[("ResourceArns.member.1", &lb)])).await.unwrap_err().code, "ValidationError");

        delete_load_balancer(&state, &a, &q(&[("LoadBalancerArn", &lb)])).await.unwrap();
        let left: i64 = crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM elbv2_tags WHERE resource_id <> ?").bind(Uuid::nil()).fetch_one(&state.pool).await.unwrap();
        assert_eq!(left, 1, "only the target group's tag is left");
    }

    #[tokio::test]
    async fn a_balancer_whose_host_rejected_the_rules_shows_failed() {
        let (state, _) = world().await;
        let a = admin();
        let lb = nlb(&state, "web").await;
        let tg = tcp_group(&state, "pool", "80").await;
        let x = describe_load_balancers(&state, &a, &q(&[])).await.unwrap();
        assert!(x.contains("<State><Code>active</Code></State>"), "{x}");
        listen(&state, &lb, "TCP", "80", &tg).await.unwrap();
        crate::db::query("UPDATE load_balancers SET status = 'error', status_message = 'agent unreachable'").execute(&state.pool).await.unwrap();
        let x = describe_load_balancers(&state, &a, &q(&[("Names.member.1", "web")])).await.unwrap();
        assert!(x.contains("<State><Code>failed</Code><Reason>agent unreachable</Reason></State>"), "{x}");
    }

    #[tokio::test]
    async fn lists_page_with_a_marker() {
        let (state, _) = world().await;
        let a = admin();
        for n in ["a", "b", "c"] {
            nlb(&state, n).await;
        }
        let first = describe_load_balancers(&state, &a, &q(&[("PageSize", "2")])).await.unwrap();
        assert_eq!(first.matches("<LoadBalancerName>").count(), 2, "{first}");
        let marker = between(&first, "NextMarker").to_string();
        let second = describe_load_balancers(&state, &a, &q(&[("PageSize", "2"), ("Marker", &marker)])).await.unwrap();
        assert_eq!(second.matches("<LoadBalancerName>").count(), 1, "{second}");
        assert!(!second.contains("NextMarker"));
        let mut seen: Vec<&str> = first.split("<LoadBalancerName>").skip(1).chain(second.split("<LoadBalancerName>").skip(1)).map(|s| &s[..s.find('<').unwrap()]).collect();
        seen.sort_unstable();
        assert_eq!(seen, ["a", "b", "c"]);
        assert_eq!(describe_load_balancers(&state, &a, &q(&[("PageSize", "0")])).await.unwrap_err().code, "ValidationError");
        assert_eq!(describe_load_balancers(&state, &a, &q(&[("PageSize", "401")])).await.unwrap_err().code, "ValidationError");
        assert_eq!(describe_load_balancers(&state, &a, &q(&[("Marker", "!!")])).await.unwrap_err().code, "ValidationError");
    }

    #[tokio::test]
    async fn every_action_needs_the_operator_role_and_the_static_answers_are_valid() {
        let (state, _) = world().await;
        let viewer = AuthUser { username: "v".into(), role: "viewer".into(), auth_source: None };
        for action in ["DescribeLoadBalancers", "CreateLoadBalancer", "DescribeTargetGroups", "DescribeSSLPolicies", "DescribeAccountLimits", "AddTags"] {
            let e = dispatch(&state, &viewer, &q(&[("Name", "x")]), action).await.unwrap().unwrap_err();
            assert_eq!(e.code, "UnauthorizedOperation", "{action}");
        }
        assert!(dispatch(&state, &admin(), &q(&[]), "ConfigureHealthCheck").await.is_none());
        let a = admin();
        let p = describe_ssl_policies(&state, &a, &q(&[])).await.unwrap();
        assert!(p.contains("ELBSecurityPolicy-TLS13-1-2-2021-06") && p.contains("<SslProtocols><member>"), "{p}");
        let one = describe_ssl_policies(&state, &a, &q(&[("Names.member.1", "ELBSecurityPolicy-TLS-1-2-2017-01")])).await.unwrap();
        assert!(one.contains("ELBSecurityPolicy-TLS-1-2-2017-01") && !one.contains("TLS13"), "{one}");
        let l = describe_account_limits(&state, &a, &q(&[])).await.unwrap();
        assert!(l.contains("<Name>target-groups</Name><Max>3000</Max>"), "{l}");
        assert_eq!(create_load_balancer(&state, &a, &q(&[("Name", "x"), ("Subnets.member.1", "subnet-0123456789abcdef0")])).await.unwrap_err().code, "InvalidSubnet");
    }
}

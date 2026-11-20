// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Placement groups. A group is a name and a strategy; a member is an instance launched with `Placement.GroupName`.
//!
//! * `cluster` puts every member on one host: the host of a member that already runs, otherwise the host the cluster's own
//!   placement picks for the first one.
//! * `spread` puts every member on a different host. A launch that cannot find enough distinct hosts fails with
//!   `InsufficientInstanceCapacity` rather than quietly sharing one.
//! * `partition` is refused: there are no partitions to place into.
//!
//! The hosts are chosen at launch. A later migration (DRS, HA failover, maintenance) can move a member, and nothing then
//! re-checks the group.

use std::collections::{BTreeMap, HashSet};

use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::{indexed, tagspec, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b' ' | b'.'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub strategy: String,
}

pub async fn find(state: &AppState, name: &str) -> Result<Option<Group>, Ec2Error> {
    let row: Option<(Uuid, String, String)> =
        crate::db::query_as("SELECT id, name, strategy FROM ec2_placement_groups WHERE name = ?").bind(name).fetch_optional(&state.pool).await?;
    Ok(row.map(|(id, name, strategy)| Group { id, name, strategy }))
}

fn group_xml(g: &Group, tags: &BTreeMap<String, String>) -> String {
    format!(
        "<groupName>{}</groupName><state>available</state><strategy>{}</strategy><groupId>{}</groupId>{}",
        xml_escape(&g.name),
        xml_escape(&g.strategy),
        ec2_id(Kind::PlacementGroup, g.id),
        tagspec::tag_set_xml(tags)
    )
}

pub async fn create_placement_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = p.get("GroupName").cloned().ok_or_else(|| bad("MissingParameter", "The request must contain the parameter GroupName"))?;
    if !valid_name(&name) {
        return Err(bad("InvalidParameterValue", "GroupName may hold letters, digits, space, '-', '_' and '.' (at most 255)"));
    }
    let strategy = p.get("Strategy").map(String::as_str).unwrap_or("cluster");
    match strategy {
        "cluster" | "spread" => {}
        "partition" => return Err(bad("UnsupportedOperation", "the partition strategy is not supported: there are no partitions to place into (use spread)")),
        other => return Err(bad("InvalidParameterValue", format!("Strategy must be cluster, spread or partition, not '{other}'"))),
    }
    if p.contains_key("PartitionCount") {
        return Err(bad("UnsupportedOperation", "PartitionCount is not supported"));
    }
    if p.get("SpreadLevel").is_some_and(|v| v != "host") {
        return Err(bad("UnsupportedOperation", "SpreadLevel can only be host: there are no racks to spread over"));
    }
    tagspec::only_types(p, &["placement-group"])?;
    if find(state, &name).await?.is_some() {
        return Err(bad("InvalidPlacementGroup.Duplicate", format!("The placement group '{name}' already exists")));
    }
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO ec2_placement_groups (id, name, strategy) VALUES (?, ?, ?)").bind(id).bind(&name).bind(strategy).execute(&state.pool).await?;
    let tags = tagspec::tags_for(p, "placement-group");
    tagspec::apply(state, Kind::PlacementGroup, id, &tags).await?;
    Ok(format!("<placementGroup>{}</placementGroup>", group_xml(&Group { id, name, strategy: strategy.to_string() }, &tags)))
}

pub async fn describe_placement_groups(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let names = indexed(p, "GroupName");
    let ids = indexed(p, "GroupId");
    let rows: Vec<(Uuid, String, String)> = crate::db::query_as("SELECT id, name, strategy FROM ec2_placement_groups ORDER BY name").fetch_all(&state.pool).await?;
    for n in &names {
        if !rows.iter().any(|r| &r.1 == n) {
            return Err(bad("InvalidPlacementGroup.Unknown", format!("The placement group '{n}' does not exist")));
        }
    }
    for i in &ids {
        if !rows.iter().any(|r| &ec2_id(Kind::PlacementGroup, r.0) == i) {
            return Err(bad("InvalidPlacementGroup.Unknown", format!("The placement group '{i}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, name, strategy) in rows {
        if (!names.is_empty() && !names.contains(&name)) || (!ids.is_empty() && !ids.contains(&ec2_id(Kind::PlacementGroup, id))) {
            continue;
        }
        let tags = tagspec::tags_of(state, Kind::PlacementGroup, id).await?;
        items.push_str(&format!("<item>{}</item>", group_xml(&Group { id, name, strategy }, &tags)));
    }
    Ok(format!("<placementGroupSet>{items}</placementGroupSet>"))
}

pub async fn delete_placement_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = p.get("GroupName").cloned().ok_or_else(|| bad("MissingParameter", "The request must contain the parameter GroupName"))?;
    let Some(g) = find(state, &name).await? else {
        return Err(bad("InvalidPlacementGroup.Unknown", format!("The placement group '{name}' does not exist")));
    };
    let members: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM ec2_instance_attrs WHERE placement_group = ?").bind(&name).fetch_one(&state.pool).await?;
    if members > 0 {
        return Err(bad("InvalidPlacementGroup.InUse", format!("The placement group '{name}' has {members} instance(s)")));
    }
    crate::db::query("DELETE FROM resource_tags WHERE resource_type = 'placement_group' AND resource_id = ?").bind(g.id.simple().to_string()).execute(&state.pool).await?;
    crate::db::query("DELETE FROM ec2_placement_groups WHERE id = ?").bind(g.id).execute(&state.pool).await?;
    Ok("<return>true</return>".into())
}

// ---- choosing hosts ------------------------------------------------------------------------------------------------

/// A host that can take a new instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: Uuid,
    pub free_mib: i64,
}

/// Pure placement over a strategy: the host for each of `count` new members.
///
/// * `members` are the hosts that already run a member of the group;
/// * `candidates` are the hosts that may take one more (online, schedulable, with room for `need_mib`).
pub fn choose(strategy: &str, count: usize, members: &[Uuid], candidates: &[Candidate], need_mib: i64) -> Result<Vec<Uuid>, Ec2Error> {
    let capacity = |msg: String| Ec2Error::bad("InsufficientInstanceCapacity", msg);
    let room = |c: &&Candidate| c.free_mib >= need_mib;
    match strategy {
        "cluster" => {
            // all on the host that already holds the group, or on the roomiest host when it is empty
            let host = if let Some(m) = members.first() {
                let h = candidates.iter().find(|c| &c.id == m).ok_or_else(|| capacity("the host of the cluster placement group cannot take another instance".into()))?;
                if members.iter().any(|x| x != m) {
                    return Err(capacity("the cluster placement group's members are on different hosts".into()));
                }
                h.clone()
            } else {
                candidates.iter().filter(room).max_by_key(|c| c.free_mib).cloned().ok_or_else(|| capacity("no host has room for the cluster placement group".into()))?
            };
            if host.free_mib < need_mib.saturating_mul(count as i64) {
                return Err(capacity(format!("the host of the cluster placement group has room for {} of the {count} instances", host.free_mib / need_mib.max(1))));
            }
            Ok(vec![host.id; count])
        }
        "spread" => {
            let used: HashSet<&Uuid> = members.iter().collect();
            let mut free: Vec<&Candidate> = candidates.iter().filter(room).filter(|c| !used.contains(&c.id)).collect();
            free.sort_by_key(|c| std::cmp::Reverse(c.free_mib));
            if free.len() < count {
                return Err(capacity(format!("a spread placement group needs {count} more host(s) without a member, {} available", free.len())));
            }
            Ok(free.into_iter().take(count).map(|c| c.id).collect())
        }
        other => Err(Ec2Error::bad("UnsupportedOperation", format!("placement strategy '{other}' is not supported"))),
    }
}

/// Hosts for a launch into `group`: reads the members' hosts and the eligible hosts, then [`choose`]s.
pub async fn plan_hosts(state: &AppState, group: &Group, count: usize, need_mib: i64) -> Result<Vec<Uuid>, Ec2Error> {
    let members: Vec<Uuid> = crate::db::query_scalar(
        "SELECT v.host_id FROM vms v JOIN ec2_instance_attrs a ON a.vm_id = v.id WHERE a.placement_group = ? AND v.host_id IS NOT NULL",
    )
    .bind(&group.name)
    .fetch_all(&state.pool)
    .await?;
    let rows: Vec<(Uuid, i64, i64)> = crate::db::query_as(
        "SELECT id, memory_used_mib, memory_total_mib FROM hosts WHERE state = 'online' AND maintenance_mode = FALSE AND schedulable = TRUE",
    )
    .fetch_all(&state.pool)
    .await?;
    let candidates: Vec<Candidate> = rows.into_iter().map(|(id, used, total)| Candidate { id, free_mib: total - used }).collect();
    choose(&group.strategy, count, &members, &candidates, need_mib)
}

/// `Placement.GroupName` of a launch → the group, which must exist.
pub async fn group_for_run(state: &AppState, p: &Params) -> Result<Option<Group>, Ec2Error> {
    let Some(name) = p.get("Placement.GroupName").filter(|n| !n.is_empty()) else { return Ok(None) };
    find(state, name).await?.map(Some).ok_or_else(|| bad("InvalidPlacementGroup.Unknown", format!("The placement group '{name}' does not exist")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(n: u128, free: i64) -> Candidate {
        Candidate { id: Uuid::from_u128(n), free_mib: free }
    }

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn cluster_stays_on_the_members_host_or_the_roomiest() {
        let cands = [h(1, 8000), h(2, 20000), h(3, 4000)];
        // empty group: the roomiest host takes all
        assert_eq!(choose("cluster", 3, &[], &cands, 1024).unwrap(), vec![Uuid::from_u128(2); 3]);
        // existing member on host 1: stay there
        assert_eq!(choose("cluster", 2, &[Uuid::from_u128(1)], &cands, 1024).unwrap(), vec![Uuid::from_u128(1); 2]);
        // not enough room on the members' host for all of them
        assert_eq!(choose("cluster", 9, &[Uuid::from_u128(1)], &cands, 1024).unwrap_err().code, "InsufficientInstanceCapacity");
        // the members' host is gone / full
        assert_eq!(choose("cluster", 1, &[Uuid::from_u128(9)], &cands, 1024).unwrap_err().code, "InsufficientInstanceCapacity");
        assert_eq!(choose("cluster", 1, &[], &[h(1, 100)], 1024).unwrap_err().code, "InsufficientInstanceCapacity");
    }

    #[test]
    fn spread_uses_distinct_hosts_and_fails_rather_than_share() {
        let cands = [h(1, 8000), h(2, 20000), h(3, 4000)];
        let hosts = choose("spread", 3, &[], &cands, 1024).unwrap();
        let unique: HashSet<_> = hosts.iter().collect();
        assert_eq!(unique.len(), 3);
        // a host that already has a member is skipped
        let rest = choose("spread", 2, &[Uuid::from_u128(2)], &cands, 1024).unwrap();
        assert!(!rest.contains(&Uuid::from_u128(2)));
        assert_eq!(choose("spread", 3, &[Uuid::from_u128(2)], &cands, 1024).unwrap_err().code, "InsufficientInstanceCapacity");
        // a host without room does not count
        assert_eq!(choose("spread", 3, &[], &cands, 6000).unwrap_err().code, "InsufficientInstanceCapacity");
        assert_eq!(choose("partition", 1, &[], &cands, 1).unwrap_err().code, "UnsupportedOperation");
    }

    #[test]
    fn group_names_are_conservative() {
        assert!(valid_name("db-tier_1"));
        assert!(!valid_name(""));
        assert!(!valid_name("a/b"));
        assert!(!valid_name(&"x".repeat(256)));
    }

    #[tokio::test]
    async fn groups_are_created_listed_tagged_and_deleted_only_when_empty() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let admin = crate::auth::AuthUser { username: "u".into(), role: "admin".into(), auth_source: None };
        let created = create_placement_group(
            &state,
            &admin,
            &p(&[("GroupName", "web"), ("Strategy", "spread"), ("TagSpecification.1.ResourceType", "placement-group"), ("TagSpecification.1.Tag.1.Key", "Env"), ("TagSpecification.1.Tag.1.Value", "prod")]),
        )
        .await
        .unwrap();
        assert!(created.contains("<groupName>web</groupName>") && created.contains("<strategy>spread</strategy>") && created.contains("<groupId>pg-"), "{created}");
        assert_eq!(create_placement_group(&state, &admin, &p(&[("GroupName", "web")])).await.unwrap_err().code, "InvalidPlacementGroup.Duplicate");
        assert_eq!(create_placement_group(&state, &admin, &p(&[("GroupName", "p"), ("Strategy", "partition")])).await.unwrap_err().code, "UnsupportedOperation");
        let listed = describe_placement_groups(&state, &p(&[])).await.unwrap();
        assert!(listed.contains("<key>Env</key><value>prod</value>"), "{listed}");
        assert_eq!(describe_placement_groups(&state, &p(&[("GroupName.1", "nope")])).await.unwrap_err().code, "InvalidPlacementGroup.Unknown");
        // a member blocks deletion
        let g = find(&state, "web").await.unwrap().unwrap();
        let vm = Uuid::new_v4();
        let cluster = Uuid::new_v4();
        crate::db::query("INSERT INTO clusters (id, name) VALUES (?, 'c')").bind(cluster).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO vms (id, cluster_id, name, spec_json) VALUES (?, ?, 'm', '{}')").bind(vm).bind(cluster).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO ec2_instance_attrs (vm_id, placement_group) VALUES (?, ?)").bind(vm).bind("web").execute(&state.pool).await.unwrap();
        assert_eq!(delete_placement_group(&state, &admin, &p(&[("GroupName", "web")])).await.unwrap_err().code, "InvalidPlacementGroup.InUse");
        crate::db::query("DELETE FROM ec2_instance_attrs WHERE vm_id = ?").bind(vm).execute(&state.pool).await.unwrap();
        delete_placement_group(&state, &admin, &p(&[("GroupName", "web")])).await.unwrap();
        assert!(find(&state, "web").await.unwrap().is_none());
        assert_eq!(g.strategy, "spread");
        assert_eq!(group_for_run(&state, &p(&[("Placement.GroupName", "web")])).await.unwrap_err().code, "InvalidPlacementGroup.Unknown");
        assert!(group_for_run(&state, &p(&[])).await.unwrap().is_none());
    }
}

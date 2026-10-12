// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Instance status, and the DescribeInstances fields the first cut left empty.
//! Status is the host's observed state. There is no second reachability probe.

use std::collections::{BTreeMap, HashMap};

use uuid::Uuid;

use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::{indexed, instance_state, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn reachability(observed: &str) -> &'static str {
    match observed {
        "running" | "blocked" => "passed",
        "shutoff" | "stopped" | "paused" | "suspended" | "pmsuspended" | "terminated" => "passed",
        "crashed" => "failed",
        _ => "initializing",
    }
}

/// Launch tasks (`vm.apply`) whose latest run failed, by VM id, with the task's error text. A VM that the host
/// reports a state for is not looked up by the callers: a later start can still succeed.
pub(crate) async fn failed_launches(state: &AppState) -> Result<HashMap<Uuid, String>, Ec2Error> {
    let rows: Vec<(Uuid, Option<String>)> = crate::db::query_as(
        "SELECT t.resource_id, t.message FROM tasks t WHERE t.operation = 'vm.apply' AND t.status = 'failed' AND t.resource_type = 'vm' \
         AND t.resource_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM tasks n WHERE n.operation = 'vm.apply' AND n.resource_id = t.resource_id \
         AND n.status != 'failed' AND n.created_at >= t.created_at) ORDER BY t.created_at",
    )
    .fetch_all(&state.pool)
    .await?;
    // ORDER BY created_at: a later failure replaces an earlier one
    Ok(rows.into_iter().map(|(id, m)| (id, clean_cause(m.as_deref()))).collect())
}

/// The failure of one instance's launch, when it never came up (the host reports no state for it).
pub(crate) async fn launch_failure_of(state: &AppState, id: Uuid) -> Result<Option<String>, Ec2Error> {
    let observed: Option<String> = crate::db::query_scalar("SELECT observed_state FROM vms WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    match observed {
        Some(o) if instance_state(&o).0 == 0 => Ok(failed_launches(state).await?.remove(&id)),
        _ => Ok(None),
    }
}

fn clean_cause(m: Option<&str>) -> String {
    let t = m.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("the launch task failed without an error message");
    t.chars().take(500).collect()
}

pub(crate) fn launch_failed_message(id: &str, cause: &str) -> String {
    format!("The instance '{id}' never started: its launch failed ({cause}). Terminate it and launch a new one")
}

/// `<stateReason>` and `<reason>` elements for an instance whose launch failed.
pub(crate) fn state_reason_xml(cause: &str) -> String {
    let msg = xml_escape(&format!("Server.InternalError: {cause}"));
    format!("<reason>{msg}</reason><stateReason><code>Server.InternalError</code><message>{msg}</message></stateReason>")
}

pub async fn describe_instance_status(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "InstanceId");
    let include_all = matches!(p.get("IncludeAllInstances").map(String::as_str), Some("true") | Some("1"));
    let rows: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, observed_state FROM vms WHERE COALESCE(inventory_source, 'libvirt') != 'kubevirt' ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    let failures = failed_launches(state).await?;
    let mut items = String::new();
    for (id, observed) in rows {
        let failed = instance_state(&observed).0 == 0 && failures.contains_key(&id);
        let observed = if failed { "terminated".to_string() } else { observed };
        let eid = ec2_id(Kind::Vm, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let (code, name) = instance_state(&observed);
        if !include_all && name != "running" {
            continue;
        }
        let status = if observed == "crashed" { "impaired" } else if failed { "not-applicable" } else { "ok" };
        items.push_str(&format!(
            "<item><instanceId>{eid}</instanceId><instanceState><code>{code}</code><name>{name}</name></instanceState>\
<systemStatus><status>{status}</status><details><item><name>reachability</name><status>{}</status></item></details></systemStatus>\
<instanceStatus><status>{status}</status><details><item><name>reachability</name><status>{}</status></item></details></instanceStatus></item>",
            reachability(&observed),
            reachability(&observed)
        ));
    }
    for w in &wanted {
        if !items.contains(w) {
            return Err(bad("InvalidInstanceID.NotFound", format!("The instance ID '{w}' does not exist")));
        }
    }
    Ok(format!("<instanceStatusSet>{items}</instanceStatusSet>"))
}

/// Extra DescribeInstances elements for one machine. Empty string when the row is a tombstone.
pub async fn instance_extra(state: &AppState, id: Uuid) -> Result<String, Ec2Error> {
    let vm: Option<(Option<String>, Option<Uuid>)> =
        crate::db::query_as("SELECT json_extract(spec_json, '$.template_ref'), host_id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    let Some((template_ref, host_id)) = vm else {
        return Ok(String::new());
    };
    let image = match template_ref {
        Some(name) => {
            let tid: Option<Uuid> = crate::db::query_scalar("SELECT id FROM templates WHERE name = ? ORDER BY version LIMIT 1")
                .bind(&name)
                .fetch_optional(&state.pool)
                .await?;
            tid.map(|t| ec2_id(Kind::Image, t)).unwrap_or(name)
        }
        None => String::new(),
    };
    let zone = match host_id {
        Some(host) => crate::db::query_scalar::<_, String>("SELECT hostname FROM hosts WHERE id = ?")
            .bind(host)
            .fetch_optional(&state.pool)
            .await?
            .unwrap_or_default(),
        None => String::new(),
    };
    let volumes: Vec<(Uuid, Option<String>, bool)> = crate::db::query_as(
        "SELECT id, attached_device, delete_on_termination FROM volumes WHERE attached_vm_id = ? ORDER BY attached_device",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let block: String = volumes
        .iter()
        .map(|(vid, dev, del)| {
            let device = dev.as_deref().unwrap_or("vda");
            format!(
                "<item><deviceName>/dev/{device}</deviceName><ebs><volumeId>{}</volumeId><status>attached</status><deleteOnTermination>{del}</deleteOnTermination></ebs></item>",
                ec2_id(Kind::Volume, *vid)
            )
        })
        .collect();
    let groups: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT g.id, g.name FROM security_groups g JOIN instance_security_groups i ON i.sg_id = g.id WHERE i.vm_id = ? ORDER BY g.name",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let group_xml: String = groups
        .iter()
        .map(|(gid, name)| format!("<item><groupId>{}</groupId><groupName>{}</groupName></item>", ec2_id(Kind::SecurityGroup, *gid), xml_escape(name)))
        .collect();
    let ports: Vec<(Uuid, Option<String>, Option<String>, Option<String>)> =
        crate::db::query_as("SELECT id, subnet_id, mac_address, private_ip FROM ports WHERE vm_id = ? ORDER BY created_at")
            .bind(id)
            .fetch_all(&state.pool)
            .await?;
    let nics: String = ports
        .iter()
        .enumerate()
        .map(|(n, (pid, subnet, mac, ip))| {
            let subnet = subnet.as_deref().and_then(|s| Uuid::parse_str(s).ok()).map(|u| ec2_id(Kind::Subnet, u)).unwrap_or_default();
            format!(
                "<item><networkInterfaceId>{}</networkInterfaceId><subnetId>{subnet}</subnetId><status>in-use</status><macAddress>{}</macAddress><privateIpAddress>{}</privateIpAddress><attachment><deviceIndex>{n}</deviceIndex><status>attached</status><deleteOnTermination>false</deleteOnTermination></attachment><groupSet>{group_xml}</groupSet></item>",
                ec2_id(Kind::Port, *pid),
                xml_escape(mac.as_deref().unwrap_or("")),
                xml_escape(ip.as_deref().unwrap_or(""))
            )
        })
        .collect();
    let attrs = super::instance_attrs::load(state, id).await?;
    let group = attrs.placement_group.as_deref().map(|g| format!("<groupName>{}</groupName>", xml_escape(g))).unwrap_or_default();
    Ok(format!(
        "<imageId>{}</imageId><placement><availabilityZone>{}</availabilityZone>{group}<tenancy>default</tenancy></placement><rootDeviceName>/dev/vda</rootDeviceName><rootDeviceType>ebs</rootDeviceType><blockDeviceMapping>{block}</blockDeviceMapping><monitoring><state>{}</state></monitoring><ebsOptimized>{}</ebsOptimized><sourceDestCheck>true</sourceDestCheck><metadataOptions><state>applied</state><httpTokens>optional</httpTokens><httpPutResponseHopLimit>{}</httpPutResponseHopLimit><httpEndpoint>{}</httpEndpoint></metadataOptions><networkInterfaceSet>{nics}</networkInterfaceSet><groupSet>{group_xml}</groupSet>",
        xml_escape(&image),
        xml_escape(&zone),
        if attrs.monitoring { "enabled" } else { "disabled" },
        attrs.ebs_optimized,
        attrs.metadata_hop_limit,
        if attrs.metadata_endpoint { "enabled" } else { "disabled" }
    ))
}

/// Replace the empty `<imageId/>` in `instance_core` with this, and drop the duplicate tag.
pub fn splice(core: &str, extra: &str) -> String {
    core.replace("<imageId/>", extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_fills_the_empty_image_slot() {
        let core = "<item><instanceId>i-1</instanceId><imageId/><instanceType>m1.small</instanceType></item>";
        let out = splice(core, "<imageId>ami-1</imageId><placement><availabilityZone>host-a</availabilityZone></placement>");
        assert!(!out.contains("<imageId/>"));
        assert!(out.contains("ami-1"));
        assert!(out.contains("m1.small"));
    }

    async fn insert_vm(state: &AppState, observed: &str) -> Uuid {
        let id = Uuid::new_v4();
        let cluster = Uuid::new_v4();
        crate::db::query("INSERT INTO clusters (id, name) VALUES (?, 'c')").bind(cluster).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO vms (id, cluster_id, name, spec_json, observed_state) VALUES (?, ?, 'web', '{}', ?)")
            .bind(id)
            .bind(cluster)
            .bind(observed)
            .execute(&state.pool)
            .await
            .unwrap();
        id
    }

    async fn insert_task(state: &AppState, vm: Uuid, status: &str, message: Option<&str>, created: &str) {
        crate::db::query(
            "INSERT INTO tasks (id, operation, status, resource_type, resource_id, message, created_at) VALUES (?, 'vm.apply', ?, 'vm', ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(status)
        .bind(vm)
        .bind(message)
        .bind(created)
        .execute(&state.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_failed_launch_is_reported_terminated_with_the_cause() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let vm = insert_vm(&state, "unknown").await;
        let eid = ec2_id(Kind::Vm, vm);
        // while the launch task is still pending the instance is pending
        insert_task(&state, vm, "pending", None, "2026-01-01 00:00:00").await;
        let x = super::super::describe_instances(&state, &p(&[("InstanceId.1", &eid)])).await.unwrap();
        assert!(x.contains("<code>0</code><name>pending</name>"), "{x}");
        assert!(!x.contains("stateReason"));
        // the task fails: terminated, with the task's error as the reason
        crate::db::query("UPDATE tasks SET status = 'failed', message = ? WHERE resource_id = ?")
            .bind("template image download failed: root@10.0.0.1: Permission denied <x>")
            .bind(vm)
            .execute(&state.pool)
            .await
            .unwrap();
        let x = super::super::describe_instances(&state, &p(&[("InstanceId.1", &eid)])).await.unwrap();
        assert!(x.contains("<code>48</code><name>terminated</name>"), "{x}");
        assert!(x.contains("<stateReason><code>Server.InternalError</code><message>Server.InternalError: template image download failed: root@10.0.0.1: Permission denied &lt;x&gt;</message></stateReason>"), "{x}");
        assert!(x.contains("<reason>Server.InternalError: template image download failed"), "{x}");
        let f = parse_filter_state(&state, "terminated").await;
        assert!(f.contains(&eid));
        // status lists it only with IncludeAllInstances, as not-applicable
        let s = describe_instance_status(&state, &p(&[])).await.unwrap();
        assert!(!s.contains(&eid));
        let s = describe_instance_status(&state, &p(&[("IncludeAllInstances", "true")])).await.unwrap();
        assert!(s.contains("<name>terminated</name>") && s.contains("not-applicable"), "{s}");
        // start/stop and attach carry the cause
        let e = super::super::power(&state, &admin(), &p(&[("InstanceId.1", &eid)]), true).await.unwrap_err();
        assert_eq!(e.code, "IncorrectInstanceState");
        assert!(e.message.contains("Permission denied") && e.message.contains("never started"), "{}", e.message);
        let cause = launch_failure_of(&state, vm).await.unwrap().unwrap();
        assert!(cause.contains("Permission denied"));
    }

    #[tokio::test]
    async fn a_later_run_or_a_known_host_state_overrides_the_failure() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        // failed, then a newer apply completed
        let a = insert_vm(&state, "unknown").await;
        insert_task(&state, a, "failed", Some("boom"), "2026-01-01 00:00:00").await;
        insert_task(&state, a, "completed", None, "2026-01-01 00:05:00").await;
        assert!(launch_failure_of(&state, a).await.unwrap().is_none());
        // failed apply, but the host reports a state: the host wins
        let b = Uuid::new_v4();
        crate::db::query("INSERT INTO vms (id, name, spec_json, observed_state) VALUES (?, 'b', '{}', 'shutoff')").bind(b).execute(&state.pool).await.unwrap();
        insert_task(&state, b, "failed", Some("boom"), "2026-01-01 00:00:00").await;
        assert!(launch_failure_of(&state, b).await.unwrap().is_none());
        // empty message still gives a cause
        let c = insert_vm_named(&state, "c", "unknown").await;
        insert_task(&state, c, "failed", None, "2026-01-01 00:00:00").await;
        assert!(launch_failure_of(&state, c).await.unwrap().unwrap().contains("without an error message"));
    }

    async fn insert_vm_named(state: &AppState, name: &str, observed: &str) -> Uuid {
        let id = Uuid::new_v4();
        crate::db::query("INSERT INTO vms (id, name, spec_json, observed_state) VALUES (?, ?, '{}', ?)")
            .bind(id)
            .bind(name)
            .bind(observed)
            .execute(&state.pool)
            .await
            .unwrap();
        id
    }

    async fn parse_filter_state(state: &AppState, name: &str) -> String {
        super::super::describe_instances(state, &p(&[("Filter.1.Name", "instance-state-name"), ("Filter.1.Value.1", name)])).await.unwrap()
    }

    fn admin() -> crate::auth::AuthUser {
        crate::auth::AuthUser { username: "t".into(), role: "admin".into(), auth_source: None }
    }

    fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct KubevirtSshExpose {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
struct VmSummaryMetaResponse {
    #[serde(default)]
    rows: Vec<VmSummaryRow>,
}

#[derive(Debug, Deserialize)]
struct VmSummaryRow {
    name: String,
    namespace: String,
    #[serde(default)]
    node_internal_ip: Option<String>,
    #[serde(default)]
    guest_ip: Option<String>,
}

fn service_matches_vm(
    selector: &serde_json::Map<String, serde_json::Value>,
    vm_name: &str,
) -> bool {
    selector.values().any(|v| v.as_str() == Some(vm_name))
        || selector
            .get("kubevirt.io/domain")
            .and_then(|v| v.as_str())
            .is_some_and(|d| d == vm_name)
        || selector
            .get("kubevirt.io/vm")
            .and_then(|v| v.as_str())
            .is_some_and(|d| d == vm_name)
}

fn parse_target_port(v: &serde_json::Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn extract_ssh_node_port(svc: &serde_json::Value) -> Option<u16> {
    let ports = svc.get("spec")?.get("ports")?.as_array()?;
    for p in ports {
        let svc_port = p.get("port")?.as_u64()?;
        let target = p
            .get("targetPort")
            .and_then(parse_target_port)
            .unwrap_or(svc_port);
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let is_ssh = target == 22 || svc_port == 22 || name.eq_ignore_ascii_case("ssh");
        if !is_ssh {
            continue;
        }
        if let Some(np) = p.get("nodePort").and_then(|v| v.as_u64()) {
            if np > 0 && np <= u16::MAX as u64 {
                return Some(np as u16);
            }
        }
    }
    None
}

fn extract_load_balancer_host(svc: &serde_json::Value) -> Option<String> {
    svc.get("status")
        .and_then(|s| s.get("loadBalancer"))
        .and_then(|lb| lb.get("ingress"))
        .and_then(|ing| ing.as_array())
        .and_then(|items| items.first())
        .and_then(|item| {
            item.get("ip")
                .or_else(|| item.get("hostname"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

async fn daemon_get_json(url: &str) -> Option<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json().await.ok()
}

async fn kubevirt_vm_row(
    daemon_base_url: &str,
    namespace: &str,
    vm_name: &str,
) -> Option<VmSummaryRow> {
    let url = format!(
        "{}/api/v1/k8s/kubevirt/vm-summary?namespace={}&meta=1",
        daemon_base_url.trim_end_matches('/'),
        urlencoding::encode(namespace)
    );
    let v = daemon_get_json(&url).await?;
    let parsed: VmSummaryMetaResponse = serde_json::from_value(v).ok()?;
    parsed
        .rows
        .into_iter()
        .find(|r| r.namespace == namespace && r.name == vm_name)
}

/// Discover SSH NodePort (or LoadBalancer) exposure for a KubeVirt VM via the co-located daemon kubectl API.
pub async fn discover_kubevirt_ssh_expose(
    daemon_base_url: &str,
    namespace: &str,
    vm_name: &str,
) -> Option<KubevirtSshExpose> {
    let row = kubevirt_vm_row(daemon_base_url, namespace, vm_name).await;
    let url = format!(
        "{}/api/v1/k8s/services?namespace={}",
        daemon_base_url.trim_end_matches('/'),
        urlencoding::encode(namespace)
    );
    let services = daemon_get_json(&url).await?;
    let items = services.get("items")?.as_array()?;

    let mut node_port: Option<u16> = None;
    let mut lb_host: Option<String> = None;

    for svc in items {
        let selector = svc
            .get("spec")
            .and_then(|s| s.get("selector"))
            .and_then(|s| s.as_object());
        let Some(selector) = selector else {
            continue;
        };
        if !service_matches_vm(selector, vm_name) {
            continue;
        }
        if node_port.is_none() {
            node_port = extract_ssh_node_port(svc);
        }
        if lb_host.is_none() {
            lb_host = extract_load_balancer_host(svc);
        }
    }

    let port = node_port?;
    let host = lb_host
        .or_else(|| {
            row.as_ref()
                .and_then(|r| r.node_internal_ip.clone())
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            row.as_ref()
                .and_then(|r| r.guest_ip.clone())
                .filter(|s| !s.trim().is_empty())
        })?;

    Some(KubevirtSshExpose {
        host: host.trim().to_string(),
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_kubevirt_service_selector() {
        let mut sel = serde_json::Map::new();
        sel.insert(
            "kubevirt.io/domain".into(),
            serde_json::Value::String("my-vm".into()),
        );
        assert!(service_matches_vm(&sel, "my-vm"));
        assert!(!service_matches_vm(&sel, "other"));
    }
}

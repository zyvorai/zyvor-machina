// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::detect::run_cmd;
use super::profiles::profile_by_name;
use super::types::{FirewallBackend, FirewallPlanRequest, FirewallPlanResult};
use super::zones::resolve_source_cidr;
use crate::LibvirtError;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize)]
pub struct K8sPolicyManifest {
    pub kind: String,
    pub namespace: String,
    pub name: String,
    pub yaml: String,
}

pub fn compile_k8s_policies(
    namespace: &str,
    profile_name: &str,
    zones: &HashMap<String, String>,
) -> Result<(Vec<K8sPolicyManifest>, Vec<String>), LibvirtError> {
    let profile = profile_by_name(profile_name)
        .ok_or_else(|| LibvirtError::Invalid(format!("Unknown profile: {profile_name}")))?;
    let safe_ns = if namespace.is_empty() {
        "default"
    } else {
        namespace
    };
    let safe_name = format!(
        "zeus-{}",
        profile_name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            })
            .collect::<String>()
    );

    let mut ingress = Vec::new();
    let mut warnings = Vec::new();
    for rule in &profile.rules {
        if rule.direction != "inbound" || rule.action != "allow" {
            continue;
        }
        let from = if rule.sources.is_empty() || rule.sources.iter().any(|s| s == "any") {
            "- ipBlock:\n      cidr: 0.0.0.0/0".to_string()
        } else {
            rule.sources
                .iter()
                .map(|s| {
                    if let Some(cidr) = resolve_source_cidr(s, zones) {
                        format!("- ipBlock:\n      cidr: {cidr}")
                    } else {
                        // `s` is a zone label ("admin-network"/"monitoring"/"bmc-vlan") with no
                        // entry in MACHINA_FIREWALL_ZONES — emitting it as `ipBlock.cidr` produces
                        // YAML the API server rejects outright (cidr must be real CIDR notation),
                        // silently applying zero restriction. Fall back to same-cluster traffic
                        // and surface why, since the operator likely expects this to be enforced.
                        warnings.push(format!(
                            "Rule for port {} references zone \"{s}\" with no configured CIDR — restricted to same-cluster traffic instead. Set MACHINA_FIREWALL_ZONES to enforce the intended source.",
                            rule.ports
                        ));
                        "- namespaceSelector: {}".to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n    ")
        };
        let port_block = if rule.ports == "*" {
            String::new()
        } else {
            format!(
                "\n        - protocol: {}\n          port: {}",
                rule.protocol.to_uppercase(),
                rule.ports
            )
        };
        ingress.push(format!("    - from:\n    {from}\n      ports:{port_block}"));
    }

    let np_yaml = format!(
        "apiVersion: networking.k8s.io/v1\nkind: NetworkPolicy\nmetadata:\n  name: {safe_name}\n  namespace: {safe_ns}\n  labels:\n    app.kubernetes.io/managed-by: zeus-firewall\nspec:\n  podSelector: {{}}\n  policyTypes:\n  - Ingress\n  ingress:\n{}\n",
        ingress.join("\n")
    );

    Ok((
        vec![K8sPolicyManifest {
            kind: "NetworkPolicy".into(),
            namespace: safe_ns.into(),
            name: safe_name,
            yaml: np_yaml,
        }],
        warnings,
    ))
}

pub fn apply_k8s_plan(
    namespace: &str,
    req: &FirewallPlanRequest,
) -> Result<FirewallPlanResult, LibvirtError> {
    let profile = req
        .profile
        .clone()
        .or_else(|| req.preset.clone())
        .unwrap_or_else(|| "ProductionServer".into());
    let (manifests, warnings) = compile_k8s_policies(namespace, &profile, &req.zone_cidrs)?;
    let mut operations = Vec::new();
    for m in &manifests {
        operations.push(format!("kubectl apply -f - <<'EOF'\n{}\nEOF", m.yaml));
    }
    if req.dry_run {
        let mut diff = super::diff::compute_diff(&[], &[]);
        diff.warnings.extend(warnings);
        return Ok(FirewallPlanResult { diff, operations });
    }
    for m in &manifests {
        let status = std::process::Command::new("kubectl")
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(m.yaml.as_bytes())?;
                }
                child.wait()
            })
            .map_err(LibvirtError::map_op("kubectl apply"))?;
        if !status.success() {
            return Err(LibvirtError::Operation(format!(
                "kubectl apply failed for {}",
                m.name
            )));
        }
    }
    let mut diff = super::diff::compute_diff(&[], &[]);
    diff.warnings.extend(warnings);
    Ok(FirewallPlanResult { diff, operations })
}

pub fn k8s_cluster_ready() -> bool {
    run_cmd("kubectl", &["version", "--client"]).is_ok()
        && run_cmd("kubectl", &["get", "nodes", "--request-timeout=5s"]).is_ok()
}

pub fn detect_k8s_backend() -> FirewallBackend {
    if run_cmd(
        "kubectl",
        &["get", "networkpolicy", "-A", "--request-timeout=3s"],
    )
    .is_ok()
    {
        FirewallBackend::K8sNetworkPolicy
    } else {
        FirewallBackend::Unknown
    }
}

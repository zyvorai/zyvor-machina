// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::guest_context::{self, GuestAiSnapshot};

use super::llm::{self, CompletionRequest};
use super::routing::TaskClass;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestInsightRow {
    pub title: String,
    pub severity: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestRecommendation {
    pub label: String,
    pub action: String,
    pub risk: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuestAiInsightsReport {
    pub vm_id: String,
    pub vm_name: String,
    pub snapshot: GuestAiSnapshot,
    pub summary: String,
    pub insights: Vec<GuestInsightRow>,
    pub recommendations: Vec<GuestRecommendation>,
    pub llm_powered: bool,
}

pub async fn generate_insights(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
    refresh: bool,
    focus: Option<&str>,
) -> anyhow::Result<GuestAiInsightsReport> {
    let snap = guest_context::snapshot_for_vm(pool, cfg, vm_id, refresh).await?;
    let deterministic = deterministic_insights(&snap, focus);

    if super::settings::llm_enabled(pool).await.unwrap_or(false) {
        let system = "You are a virtualization SRE analyzing QEMU guest-agent telemetry. \
Respond with ONLY valid JSON matching this schema: \
{\"summary\":\"string\",\"insights\":[{\"title\":\"string\",\"severity\":\"info|warn|error\",\"detail\":\"string\"}],\
\"recommendations\":[{\"label\":\"string\",\"action\":\"guest.sync_time|guest.fstrim|guest.install_tools|guest.graceful_shutdown|snapshot.quiesce|none\",\"risk\":\"low|medium|high\",\"rationale\":\"string\"}]}. \
Use Machina actions only. Flag time drift >5s, missing QGA, unexpected users, full disks >90%, frozen FS.";
        let user = format!(
            "VM: {}\nFocus: {}\nSnapshot JSON:\n{}",
            snap.vm_name,
            focus.unwrap_or("general health"),
            serde_json::to_string_pretty(&snap)?
        );
        if let Ok(Some(text)) = llm::complete(
            pool,
            CompletionRequest {
                task_class: TaskClass::Infrastructure,
                system: system.to_string(),
                user,
                agent_id: None,
                user_id: None,
            },
        )
        .await
        {
            if let Ok(parsed) = serde_json::from_str::<LlmInsightsBody>(&extract_json(&text)) {
                return Ok(GuestAiInsightsReport {
                    vm_id: snap.vm_id.clone(),
                    vm_name: snap.vm_name.clone(),
                    snapshot: snap,
                    summary: parsed.summary,
                    insights: parsed.insights,
                    recommendations: parsed.recommendations,
                    llm_powered: true,
                });
            }
        }
    }

    Ok(GuestAiInsightsReport {
        vm_id: snap.vm_id.clone(),
        vm_name: snap.vm_name.clone(),
        snapshot: snap,
        summary: deterministic.summary,
        insights: deterministic.insights,
        recommendations: deterministic.recommendations,
        llm_powered: false,
    })
}

#[derive(Debug, Deserialize)]
struct LlmInsightsBody {
    summary: String,
    insights: Vec<GuestInsightRow>,
    recommendations: Vec<GuestRecommendation>,
}

struct DeterministicBundle {
    summary: String,
    insights: Vec<GuestInsightRow>,
    recommendations: Vec<GuestRecommendation>,
}

fn deterministic_insights(s: &GuestAiSnapshot, focus: Option<&str>) -> DeterministicBundle {
    let mut insights = Vec::new();
    let mut recommendations = Vec::new();

    if !s.agent_ping {
        insights.push(GuestInsightRow {
            title: "Guest agent not responding".into(),
            severity: if s.install_state == "channel_only" {
                "warn".into()
            } else {
                "error".into()
            },
            detail: s.summary.clone(),
        });
        recommendations.push(GuestRecommendation {
            label: "Install guest tools".into(),
            action: "guest.install_tools".into(),
            risk: "low".into(),
            rationale: "Attach virtio channel and install guestkit-agent inside the VM.".into(),
        });
    } else {
        insights.push(GuestInsightRow {
            title: "Guest agent active".into(),
            severity: "info".into(),
            detail: format!("{} · {}", s.os_pretty_name, s.agent_version),
        });
    }

    if let Some(ms) = s.time_delta_ms {
        if ms.abs() > 5000 {
            insights.push(GuestInsightRow {
                title: "Guest clock drift".into(),
                severity: "warn".into(),
                detail: format!("Guest time differs from host by {ms} ms"),
            });
            recommendations.push(GuestRecommendation {
                label: "Sync guest time to host".into(),
                action: "guest.sync_time".into(),
                risk: "low".into(),
                rationale: "Correct time skew after migration or suspend/resume.".into(),
            });
        }
    }

    for fs in &s.filesystems {
        if fs.used_pct >= 90 {
            insights.push(GuestInsightRow {
                title: format!("Filesystem {} nearly full", fs.mountpoint),
                severity: "warn".into(),
                detail: format!("{}% used", fs.used_pct),
            });
        }
    }

    if !s.users.is_empty() && s.agent_ping {
        insights.push(GuestInsightRow {
            title: "Active guest sessions".into(),
            severity: "info".into(),
            detail: format!(
                "{} logged-in user(s): {}",
                s.users.len(),
                s.users
                    .iter()
                    .map(|u| u.username.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    if s.fs_frozen == Some(true) {
        insights.push(GuestInsightRow {
            title: "Filesystems frozen".into(),
            severity: "warn".into(),
            detail: "Guest FS freeze is active — thaw before normal I/O.".into(),
        });
    }

    if focus == Some("snapshot") && s.agent_ping {
        recommendations.push(GuestRecommendation {
            label: "Quiesce filesystems for snapshot".into(),
            action: "snapshot.quiesce".into(),
            risk: "medium".into(),
            rationale: "Application-consistent snapshot for database or mail workloads.".into(),
        });
        if s.filesystems.iter().any(|f| f.used_pct > 50) {
            recommendations.push(GuestRecommendation {
                label: "TRIM before snapshot".into(),
                action: "guest.fstrim".into(),
                risk: "low".into(),
                rationale: "Reclaim thin-provisioned space before backup.".into(),
            });
        }
    }

    let summary = if insights.is_empty() {
        format!("{} appears healthy from guest-agent telemetry.", s.vm_name)
    } else {
        format!(
            "{} — {} insight(s) from guest agent.",
            s.vm_name,
            insights.len()
        )
    };

    DeterministicBundle {
        summary,
        insights,
        recommendations,
    }
}

fn extract_json(text: &str) -> String {
    // Guard start <= end so a stray '}' before the first '{' (malformed LLM
    // output) can't panic text[start..=end] with an inverted slice range.
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if start <= end {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_flags_drift() {
        let s = GuestAiSnapshot {
            vm_id: "u".into(),
            vm_name: "vm".into(),
            observed_state: "running".into(),
            inventory_source: "libvirt".into(),
            install_state: "running".into(),
            agent_ping: true,
            agent_reachable: true,
            healthy: true,
            os_pretty_name: "Ubuntu".into(),
            os_kernel: String::new(),
            os_arch: String::new(),
            guest_hostname: String::new(),
            guest_ip: String::new(),
            agent_version: String::new(),
            ipv4_addresses: vec![],
            users: vec![],
            time_delta_ms: Some(12_000),
            fs_frozen: None,
            filesystems: vec![],
            issues: vec![],
            summary: String::new(),
            cloud_init_status: None,
            collected_at: String::new(),
        };
        let d = deterministic_insights(&s, None);
        assert!(d.insights.iter().any(|i| i.title.contains("drift")));
        assert!(d
            .recommendations
            .iter()
            .any(|r| r.action == "guest.sync_time"));
    }

    #[test]
    fn extract_json_handles_inverted_braces() {
        // Well-formed: returns the object span.
        assert_eq!(extract_json("noise {\"a\":1} tail"), "{\"a\":1}");
        // Inverted: a '}' before the first '{' must NOT panic (start > end).
        assert_eq!(extract_json("ok} then {name"), "ok} then {name");
        // No braces at all: passthrough.
        assert_eq!(extract_json("plain text"), "plain text");
    }
}

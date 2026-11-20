// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Guest-agent tools for Zyra copilot — read tools inline, write tools via ai_actions approval.

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::guest_context::{self, GuestAiSnapshot};

use super::actions::{self, CreateActionBody};

#[derive(Debug, Clone, Serialize)]
pub struct GuestToolResult {
    pub tool: String,
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<GuestAiSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    pub requires_approval: bool,
}

pub async fn execute_read(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
    tool: &str,
    _refresh: bool,
) -> anyhow::Result<GuestToolResult> {
    match tool {
        "get_guest_snapshot" => {
            let snap = guest_context::snapshot_for_vm(pool, cfg, vm_id, false).await?;
            Ok(GuestToolResult {
                tool: tool.into(),
                ok: true,
                message: guest_context::context_chip(&snap),
                snapshot: Some(snap),
                action_id: None,
                requires_approval: false,
            })
        }
        "refresh_guest_snapshot" => {
            let snap = guest_context::snapshot_for_vm(pool, cfg, vm_id, true).await?;
            Ok(GuestToolResult {
                tool: tool.into(),
                ok: true,
                message: "Guest snapshot refreshed".into(),
                snapshot: Some(snap),
                action_id: None,
                requires_approval: false,
            })
        }
        _ => anyhow::bail!("unknown read tool: {tool}"),
    }
}

pub async fn propose_write(
    pool: &DbPool,
    vm_id: Uuid,
    tool: &str,
    actor: &str,
) -> anyhow::Result<GuestToolResult> {
    let (action_type, label, risk, review) = match tool {
        "propose_sync_guest_time" => (
            "guest.sync_time",
            "Sync guest time to host",
            "low",
            format!("Sync guest clock for VM {vm_id} via QEMU guest agent"),
        ),
        "propose_fstrim" => (
            "guest.fstrim",
            "TRIM guest filesystems",
            "low",
            format!("Run guest-fstrim for VM {vm_id}"),
        ),
        "propose_graceful_shutdown" => (
            "vm.shutdown_agent",
            "Graceful shutdown via guest agent",
            "medium",
            format!("Guest-agent shutdown for VM {vm_id}"),
        ),
        "propose_quiesced_snapshot" => (
            "vm.snapshot_quiesce",
            "Quiesced snapshot",
            "medium",
            format!("Create quiesced snapshot for VM {vm_id}"),
        ),
        _ => anyhow::bail!("unknown write tool: {tool}"),
    };

    let row = actions::create_action(
        pool,
        &CreateActionBody {
            action_type: action_type.to_string(),
            label: label.to_string(),
            review: review.clone(),
            risk: risk.to_string(),
            object_ref: serde_json::json!({
                "vm_id": vm_id.to_string(),
                "tool": tool,
            }),
            source: "guest_ai".into(),
        },
        actor,
    )
    .await?;

    Ok(GuestToolResult {
        tool: tool.into(),
        ok: true,
        message: format!("Pending approval: {label}"),
        snapshot: None,
        action_id: Some(row.id.to_string()),
        requires_approval: true,
    })
}

/// Parse simple `tool_call` JSON from LLM text.
#[derive(Debug, Deserialize)]
pub struct LlmToolCall {
    pub tool: String,
    #[serde(default)]
    pub vm_id: Option<String>,
}

pub fn parse_tool_call(text: &str) -> Option<LlmToolCall> {
    let json = {
        let start = text.find('{')?;
        text.get(start..)?.to_string()
    };
    serde_json::from_str(&json).ok()
}

pub const WRITE_TOOLS: &[&str] = &[
    "propose_sync_guest_time",
    "propose_fstrim",
    "propose_graceful_shutdown",
    "propose_quiesced_snapshot",
];

pub const READ_TOOLS: &[&str] = &["get_guest_snapshot", "refresh_guest_snapshot"];

pub fn tools_system_prompt() -> &'static str {
    "You may request tools by replying with JSON only: {\"tool\":\"get_guest_snapshot\"} or write tools: \
propose_sync_guest_time, propose_fstrim, propose_graceful_shutdown, propose_quiesced_snapshot. \
Write tools require human approval."
}

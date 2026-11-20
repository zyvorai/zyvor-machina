// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Machina as an MCP server (streamable-HTTP transport, JSON responses): `POST /api/v1/mcp` takes one JSON-RPC 2.0
//! message. Any MCP client (Claude Desktop/Code, an IDE, another agent) can look at the fleet and *propose*
//! changes. It uses the same tool registry as Zyra's built-in agent, so the safety model is identical: reads are
//! live, and the only write is a proposal queued in the approvals queue for a human. Authenticated like every
//! other controller route (operator role or above).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::ai::agent_loop;
use crate::state::AppState;

const DEFAULT_PROTOCOL: &str = "2025-03-26";

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Handle one JSON-RPC message. `None` means a notification: no response body.
pub async fn handle_message(state: &AppState, actor: &str, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let Some(id) = id else {
        return None; // notification (e.g. notifications/initialized)
    };
    if msg.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") || method.is_empty() {
        return Some(err(id, -32600, "invalid JSON-RPC request"));
    }
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let proto = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or(DEFAULT_PROTOCOL);
            ok(
                id,
                json!({
                    "protocolVersion": proto,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "machina", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "Look at Machina's machines and hosts. propose_action only queues a change; a person approves it in Zyra approvals."
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({"tools": agent_loop::mcp_tools()})),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
                return Some(err(id, -32602, "tools/call needs a tool name"));
            };
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (text, is_error) = agent_loop::call_tool(state, actor, name, args).await;
            ok(
                id,
                json!({"content": [{"type": "text", "text": text}], "isError": is_error}),
            )
        }
        _ => err(id, -32601, "method not found"),
    })
}

pub async fn mcp_post(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(msg): Json<Value>,
) -> Result<Response, ApiError> {
    require_operator(&actor)?;
    if msg.is_array() {
        return Err(ApiError::bad_request("batch requests are not supported"));
    }
    Ok(match handle_message(&state, &actor.username, &msg).await {
        Some(body) => Json(body).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_shape() {
        let e = err(json!(1), -32601, "x");
        assert_eq!(e["error"]["code"], -32601);
        assert_eq!(e["id"], 1);
    }

    #[test]
    fn tools_are_listed_with_input_schemas() {
        let tools = agent_loop::mcp_tools();
        assert!(tools.iter().any(|t| t["name"] == "propose_action"));
        assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    }
}

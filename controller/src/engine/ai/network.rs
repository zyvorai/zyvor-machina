// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct NetworkExplainResult {
    pub can_reach: bool,
    pub explanation: String,
    pub hops: Vec<String>,
    pub remediation: String,
}

pub async fn explain_reach(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    vm_a_name: &str,
    vm_b_name: &str,
    port: Option<i32>,
) -> anyhow::Result<NetworkExplainResult> {
    let path = super::infra_graph::explain_path(
        pool,
        cfg,
        &super::infra_graph::PathRequest {
            from: vm_a_name.into(),
            to: vm_b_name.into(),
            port,
        },
    )
    .await?;
    Ok(NetworkExplainResult {
        can_reach: path.can_reach,
        explanation: path.explanation,
        hops: path.hops,
        remediation: path
            .blockers
            .first()
            .map(|b| b.remediation.clone())
            .unwrap_or_else(|| "Verify guest firewalls and network policies.".into()),
    })
}

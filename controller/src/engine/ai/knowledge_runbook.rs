// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct KnowledgeRunbook {
    pub query: String,
    pub diagnosis_summary: String,
    pub runbook_title: String,
    pub steps: Vec<String>,
    pub commands: Vec<String>,
    pub summary: String,
}

pub async fn from_query(pool: &DbPool, query: &str) -> anyhow::Result<KnowledgeRunbook> {
    let diagnosis = super::knowledge_diagnose::diagnose(pool, query).await?;

    let incident = if query.to_lowercase().contains("backup") {
        "backup_failed"
    } else if query.to_lowercase().contains("migrat") {
        "migration_failed"
    } else if query.to_lowercase().contains("slow") || query.to_lowercase().contains("latency") {
        "high_latency"
    } else {
        "generic"
    };

    let ctx = serde_json::json!({
        "query": query,
        "hypotheses": diagnosis.hypotheses,
        "related_vms": diagnosis.related_vm_count,
    });
    let rb = super::runbook::generate(pool, incident, &ctx).await?;

    Ok(KnowledgeRunbook {
        query: query.into(),
        diagnosis_summary: diagnosis.summary,
        runbook_title: rb.title,
        steps: rb.steps,
        commands: rb.commands,
        summary: rb
            .summary
            .unwrap_or_else(|| format!("Runbook for: {query}")),
    })
}

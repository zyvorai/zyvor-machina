// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct GeneratedBlueprint {
    pub name: String,
    pub description: String,
    pub actions: Vec<String>,
    pub suggested_vms: Vec<String>,
    pub notes: String,
}

pub fn generate_from_nl(prompt: &str) -> GeneratedBlueprint {
    let pl = prompt.to_lowercase();
    let mut actions = vec!["backup".to_string()];
    let mut notes = String::new();

    if pl.contains("start") || pl.contains("app server") {
        actions.push("start".into());
    }
    if pl.contains("stop") {
        actions.push("stop".into());
    }
    if pl.contains("ha") || pl.contains("database") || pl.contains("db") {
        notes.push_str("Enable HA on database tier VMs. ");
    }
    if (pl.contains("backup") || pl.contains("daily")) && !actions.contains(&"backup".to_string()) {
        actions.push("backup".into());
    }

    let name = if pl.contains("ha") {
        "ha-stack".into()
    } else if pl.contains("app") {
        "app-tier".into()
    } else {
        "generated-blueprint".into()
    };

    GeneratedBlueprint {
        description: prompt.chars().take(200).collect(),
        name,
        actions,
        suggested_vms: vec![],
        notes: notes.trim().to_string(),
    }
}

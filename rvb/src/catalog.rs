// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! YAML template catalog (distro + release + packages → mkosi `[Distribution]` / `[Content]`).

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const BUILTIN_CATALOG_YAML: &str = include_str!("../templates/catalog.yaml");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Template {
    pub id: String,
    pub distribution: String,
    pub release: String,
    /// mkosi `Format=` (e.g. `disk`).
    pub format: String,
    #[serde(default = "default_true")]
    pub bootable: bool,
    #[serde(default)]
    pub packages: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    templates: Vec<Template>,
}

pub fn load_builtin_catalog() -> Result<HashMap<String, Template>> {
    let f: CatalogFile =
        serde_yaml::from_str(BUILTIN_CATALOG_YAML).context("parse builtin catalog YAML")?;
    let mut m = HashMap::new();
    for t in f.templates {
        if m.insert(t.id.clone(), t).is_some() {
            return Err(anyhow!("duplicate template id in catalog"));
        }
    }
    Ok(m)
}

pub fn get(templates: &HashMap<String, Template>, id: &str) -> Result<Template> {
    templates
        .get(id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown template '{id}' — run `rvb list`"))
}

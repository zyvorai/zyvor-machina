// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmTemplate {
    pub name: String,
    pub version: String,
    pub source_disk: String,
    #[serde(default)]
    pub cloud_init: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_family: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloneKind {
    #[default]
    Full,
    Linked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloneRequest {
    pub source_vm: String,
    pub new_name: String,
    pub kind: CloneKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_hostname: Option<String>,
}

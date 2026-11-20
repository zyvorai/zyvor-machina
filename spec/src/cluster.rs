// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cluster {
    pub api_version: String,
    pub kind: String,
    pub metadata: ClusterMetadata,
    pub spec: ClusterSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterMetadata {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterSpec {
    #[serde(default)]
    pub ha: bool,
    #[serde(default = "default_placement")]
    pub placement_policy: String,
    #[serde(default = "default_storage_backend")]
    pub storage_backend: String,
    #[serde(default = "default_network_backend")]
    pub network_backend: String,
    #[serde(default = "default_failure_domain")]
    pub failure_domain: String,
}

fn default_placement() -> String {
    "balanced".into()
}
fn default_storage_backend() -> String {
    "directory".into()
}
fn default_network_backend() -> String {
    "linux-bridge".into()
}
fn default_failure_domain() -> String {
    "host".into()
}

impl Cluster {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            api_version: crate::API_VERSION.into(),
            kind: "Cluster".into(),
            metadata: ClusterMetadata { name: name.into() },
            spec: ClusterSpec {
                ha: false,
                placement_policy: default_placement(),
                storage_backend: default_storage_backend(),
                network_backend: default_network_backend(),
                failure_domain: default_failure_domain(),
            },
        }
    }
}

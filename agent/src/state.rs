// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::sync::Arc;

use machina_spec::HostState;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AgentState {
    pub host_id: String,
    pub hostname: String,
    pub maintenance: HostState,
}

impl AgentState {
    pub fn new(hostname: String) -> Self {
        Self {
            host_id: Uuid::new_v4().to_string(),
            hostname,
            maintenance: HostState::Online,
        }
    }
}

pub type SharedAgentState = Arc<RwLock<AgentState>>;

pub fn shared_state(hostname: String) -> SharedAgentState {
    Arc::new(RwLock::new(AgentState::new(hostname)))
}

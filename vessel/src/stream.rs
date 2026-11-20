// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::models::ContainerStats;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub stream: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    Container { id: String, action: String },
    Pod { id: String, action: String },
    Other { message: String },
}

pub type StatsItem = Result<ContainerStats, crate::VesselError>;
pub type LogItem = Result<LogLine, crate::VesselError>;
pub type EventItem = Result<EngineEvent, crate::VesselError>;

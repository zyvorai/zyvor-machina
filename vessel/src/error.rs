// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use thiserror::Error;

#[derive(Error, Debug)]
pub enum VesselError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unsupported: {0}")]
    Unsupported(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl VesselError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Connection(_) => "vessel_unavailable",
            Self::Api(_) => "vessel_api_error",
            Self::NotFound(_) => "not_found",
            Self::Unsupported(_) => "vessel_unsupported",
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn remediation(&self) -> Option<&'static str> {
        match self {
            Self::Connection(_) => {
                Some("Start Podman or Docker, or set [vessel].socket / CONTAINER_HOST")
            }
            Self::Unsupported(_) => {
                Some("Pod operations require Podman; switch engine or use container APIs only")
            }
            _ => None,
        }
    }
}

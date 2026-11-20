// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! OpenStack resource ids are Machina UUIDs (string form). Helpers for parsing path ids.

use uuid::Uuid;

use super::OsError;

pub fn parse_uuid(id: &str) -> Result<Uuid, OsError> {
    Uuid::parse_str(id).map_err(|_| OsError::bad_request(format!("invalid id: {id}")))
}

pub fn nova_status(observed: &str) -> &'static str {
    match observed {
        "running" | "blocked" => "ACTIVE",
        "shutoff" | "stopped" | "paused" | "suspended" | "pmsuspended" => "SHUTOFF",
        "shutdown" => "SHUTOFF",
        "crashed" => "ERROR",
        "missing" => "ERROR",
        _ => "BUILD",
    }
}

pub fn cinder_status(status: &str) -> &str {
    match status {
        "available" | "in-use" | "creating" | "deleting" | "error" | "attaching" | "detaching" => {
            status
        }
        "attached" => "in-use",
        other => other,
    }
}

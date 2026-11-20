// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;
use virt::secret::Secret;

use virt::sys;

use crate::LibvirtError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretInfo {
    pub uuid: String,
    pub usage_type: String,
    pub usage_id: String,
    pub xml: String,
}

pub fn list_secrets(conn: &Connect) -> Result<Vec<SecretInfo>, LibvirtError> {
    let secrets = conn
        .list_all_secrets(0)
        .map_err(LibvirtError::map_op("Failed to list secrets"))?;

    let mut result = Vec::new();
    for secret in secrets {
        let uuid = secret.get_uuid_string().unwrap_or_default();
        let xml = secret.get_xml_desc(0).unwrap_or_default();
        let usage_type = crate::xml::extract_attr(&xml, "usage", "type").unwrap_or_default();
        let usage_id = crate::xml::extract_text(&xml, "name")
            .or_else(|| crate::xml::extract_text(&xml, "volume"))
            .or_else(|| crate::xml::extract_text(&xml, "target"))
            .unwrap_or_default();

        result.push(SecretInfo {
            uuid,
            usage_type,
            usage_id,
            xml,
        });
    }
    Ok(result)
}

pub fn get_secret_xml(conn: &Connect, uuid: &str) -> Result<String, LibvirtError> {
    let secret = Secret::lookup_by_uuid_string(conn, uuid)
        .map_err(|e| LibvirtError::NotFound(format!("Secret '{}' not found: {}", uuid, e)))?;
    secret
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get secret XML"))
}

pub fn delete_secret(conn: &Connect, uuid: &str) -> Result<(), LibvirtError> {
    let secret = Secret::lookup_by_uuid_string(conn, uuid)
        .map_err(|e| LibvirtError::NotFound(format!("Secret '{}' not found: {}", uuid, e)))?;
    secret
        .undefine()
        .map_err(LibvirtError::map_op("Failed to delete secret"))
}

/// Define a secret from `xml` (`virSecretDefineXML`), optionally set its value (`virSecretSetValue`).
/// Returns the secret UUID string.
pub fn define_secret_with_value(
    conn: &Connect,
    xml: &str,
    value: Option<&[u8]>,
    define_validate: bool,
    set_value_flags: u32,
) -> Result<String, LibvirtError> {
    let define_flags: u32 = if define_validate {
        sys::VIR_SECRET_DEFINE_VALIDATE
    } else {
        0
    };
    let secret = Secret::define_xml(conn, xml, define_flags)
        .map_err(|e| LibvirtError::Operation(format!("virSecretDefineXML: {e}")))?;
    let uuid = secret
        .get_uuid_string()
        .map_err(|e| LibvirtError::Operation(format!("secret uuid: {e}")))?;
    if let Some(v) = value {
        secret
            .set_value(v, set_value_flags)
            .map_err(|e| LibvirtError::Operation(format!("virSecretSetValue: {e}")))?;
    }
    Ok(uuid)
}

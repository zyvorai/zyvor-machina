// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;
use virt::domain::MigrateParameters;

use super::domain::lookup_domain;
use crate::LibvirtError;

const ALLOWED_URI_SCHEMES: &[&str] = &[
    "qemu://",
    "qemu+ssh://",
    "qemu+tcp://",
    "qemu+tls://",
    "qemu+unix://",
];

pub fn validate_migrate_uri(uri: &str) -> Result<(), LibvirtError> {
    if !ALLOWED_URI_SCHEMES
        .iter()
        .any(|scheme| uri.starts_with(scheme))
    {
        return Err(LibvirtError::Invalid(format!(
            "Invalid migration URI scheme. Allowed: {}",
            ALLOWED_URI_SCHEMES.join(", ")
        )));
    }
    if uri.contains([';', '|', '&', '$', '`', '\n']) {
        return Err(LibvirtError::Invalid(
            "Migration URI contains invalid characters".into(),
        ));
    }
    Ok(())
}

/// Optional tuning for [`Domain::migrate_to_uri3`] — mirrors `virt::domain::MigrateParameters`.
#[derive(Clone, serde::Deserialize, Default)]
pub struct MigrateParametersApi {
    #[serde(default)]
    pub auto_converge_increment: Option<i32>,
    #[serde(default)]
    pub auto_converge_initial: Option<i32>,
    #[serde(default)]
    pub bandwidth: Option<u64>,
    #[serde(default)]
    pub bandwidth_postcopy: Option<u64>,
    #[serde(default)]
    pub compression: Option<String>,
    #[serde(default)]
    pub compression_mt_dthreads: Option<i32>,
    #[serde(default)]
    pub compression_mt_level: Option<i32>,
    #[serde(default)]
    pub compression_mt_threads: Option<i32>,
    #[serde(default)]
    pub compression_xbzrle_cache: Option<u64>,
    #[serde(default)]
    pub compression_zlib_level: Option<i32>,
    #[serde(default)]
    pub compression_zstd_level: Option<i32>,
    #[serde(default)]
    pub dest_name: Option<String>,
    #[serde(default)]
    pub dest_xml: Option<String>,
    #[serde(default)]
    pub disks_port: Option<i32>,
    #[serde(default)]
    pub disks_uri: Option<String>,
    #[serde(default)]
    pub graphics_uri: Option<String>,
    #[serde(default)]
    pub listen_address: Option<String>,
    #[serde(default)]
    pub migrate_disks: Vec<String>,
    #[serde(default)]
    pub parallel_connections: Option<i32>,
    #[serde(default)]
    pub persist_xml: Option<String>,
    #[serde(default)]
    pub tls_destination: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
}

fn migrate_params_to_virt(a: &MigrateParametersApi) -> MigrateParameters {
    MigrateParameters {
        auto_converge_increment: a.auto_converge_increment,
        auto_converge_initial: a.auto_converge_initial,
        bandwidth: a.bandwidth,
        bandwidth_postcopy: a.bandwidth_postcopy,
        compression: a.compression.clone(),
        compression_mt_dthreads: a.compression_mt_dthreads,
        compression_mt_level: a.compression_mt_level,
        compression_mt_threads: a.compression_mt_threads,
        compression_xbzrle_cache: a.compression_xbzrle_cache,
        compression_zlib_level: a.compression_zlib_level,
        compression_zstd_level: a.compression_zstd_level,
        dest_name: a.dest_name.clone(),
        dest_xml: a.dest_xml.clone(),
        disks_port: a.disks_port,
        disks_uri: a.disks_uri.clone(),
        graphics_uri: a.graphics_uri.clone(),
        listen_address: a.listen_address.clone(),
        migrate_disks: a.migrate_disks.clone(),
        parallel_connections: a.parallel_connections,
        persist_xml: a.persist_xml.clone(),
        tls_destination: a.tls_destination.clone(),
        uri: a.uri.clone(),
    }
}

pub fn migrate_vm_uri(
    conn: &Connect,
    name: &str,
    dest_uri: &str,
    live: bool,
    extra: Option<&MigrateParametersApi>,
    extra_flags: u32,
) -> Result<(), LibvirtError> {
    validate_migrate_uri(dest_uri)?;
    let domain = lookup_domain(conn, name)?;

    let mut flags =
        virt::sys::VIR_MIGRATE_PEER2PEER | virt::sys::VIR_MIGRATE_PERSIST_DEST | extra_flags;
    if live {
        flags |= virt::sys::VIR_MIGRATE_LIVE;
    }

    if let Some(api) = extra {
        let mp = migrate_params_to_virt(api);
        domain
            .migrate_to_uri3(Some(dest_uri), mp, flags)
            .map_err(|e| LibvirtError::Operation(format!("Failed to migrate VM '{name}': {e}")))?;
    } else {
        domain
            .migrate_to_uri(dest_uri, flags, None, 0)
            .map_err(|e| LibvirtError::Operation(format!("Failed to migrate VM '{name}': {e}")))?;
    }

    Ok(())
}

/// Limit migration bandwidth (MiB/s as in libvirt `virDomainMigrateSetMaxSpeed`).
pub fn migrate_set_max_speed(
    conn: &Connect,
    name: &str,
    bandwidth_mib_per_sec: u64,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    domain
        .migrate_set_max_speed(bandwidth_mib_per_sec, 0)
        .map_err(|e| LibvirtError::Operation(format!("migrate_set_max_speed '{name}': {e}")))?;
    Ok(())
}

pub fn migrate_get_max_speed(conn: &Connect, name: &str) -> Result<u64, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    domain
        .migrate_get_max_speed(0)
        .map_err(|e| LibvirtError::Operation(format!("migrate_get_max_speed '{name}': {e}")))
}

/// Set max downtime (nanoseconds) for live migration.
pub fn migrate_set_max_downtime(
    conn: &Connect,
    name: &str,
    downtime_ns: u64,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    domain
        .migrate_set_max_downtime(downtime_ns, 0)
        .map_err(|e| LibvirtError::Operation(format!("migrate_set_max_downtime '{name}': {e}")))?;
    Ok(())
}

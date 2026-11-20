// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Libvirt connections: single configured URI or dual `qemu:///system` + `qemu:///session`.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use virt::connect::Connect;

use crate::config::LibvirtConfig;
use crate::libvirt::migrate::validate_migrate_uri;
use crate::libvirt::{domain, metrics};
use crate::state::{VmInfo, VmMetrics};
use crate::LibvirtError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibvirtTarget {
    /// Non-dual mode: `[libvirt] uri`.
    Primary,
    System,
    Session,
}

#[derive(Clone)]
struct UriSlot {
    uri: String,
    conn: Arc<Mutex<Connect>>,
}

/// Short label for `VmInfo.libvirt_connection` (query param / UI badge).
fn connection_label(uri: &str) -> String {
    if uri.contains("///session") {
        return "session".into();
    }
    if uri.contains("///system") && !uri.contains('+') {
        return "system".into();
    }
    for prefix in ["qemu+ssh://", "qemu+tls://", "qemu+tcp://", "qemu://"] {
        if let Some(rest) = uri.strip_prefix(prefix) {
            let host = rest.split('/').next().unwrap_or(rest);
            let host = host.split('@').next_back().unwrap_or(host);
            let host = host.split(':').next().unwrap_or(host);
            if !host.is_empty() {
                return host.to_string();
            }
        }
    }
    uri.chars().take(32).collect()
}

#[derive(Clone)]
pub struct LibvirtManager {
    dual: bool,
    primary: Option<UriSlot>,
    primary_uri: String,
    system: Option<UriSlot>,
    session: Option<UriSlot>,
    /// Extra hypervisor URIs (read-only merge into VM lists).
    extra: Vec<UriSlot>,
    extra_uri_labels: Vec<String>,
    /// Per-VM-name async locks for read-modify-write device operations
    /// (get_xml_desc → compute a change → attach/update/detach). The
    /// connection mutex only serializes libvirt *calls*; without this, two
    /// concurrent requests for the *same* VM (a client retry after a timeout,
    /// a double-click) can each read the same starting XML and then both
    /// attach/update/detach, racing each other or duplicating work — the
    /// mechanism behind a real incident where a client-timeout retry started a
    /// second `guestkit` run against a VM's disk while the first was still
    /// applying. Callers take the guard around the whole read-then-write
    /// sequence, not just the final libvirt call.
    vm_locks: Arc<Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl LibvirtManager {
    pub fn new(cfg: &LibvirtConfig) -> Result<Self, LibvirtError> {
        let vm_locks = Arc::new(Mutex::new(std::collections::HashMap::new()));
        if cfg.dual_connection {
            let mut system = None;
            let mut session = None;
            match Connect::open(Some("qemu:///system")) {
                Ok(c) => {
                    system = Some(UriSlot {
                        uri: "qemu:///system".into(),
                        conn: Arc::new(Mutex::new(c)),
                    });
                }
                Err(e) => tracing::warn!("dual libvirt: qemu:///system unavailable: {e}"),
            }
            match Connect::open(Some("qemu:///session")) {
                Ok(c) => {
                    session = Some(UriSlot {
                        uri: "qemu:///session".into(),
                        conn: Arc::new(Mutex::new(c)),
                    });
                }
                Err(e) => tracing::warn!("dual libvirt: qemu:///session unavailable: {e}"),
            }
            if system.is_none() && session.is_none() {
                return Err(LibvirtError::Connection(
                    "dual_connection enabled but neither qemu:///system nor qemu:///session could be opened"
                        .into(),
                ));
            }
            tracing::info!(
                "Libvirt dual mode: system={}, session={}",
                system.is_some(),
                session.is_some()
            );
            let (extra, extra_uri_labels) = Self::open_extra_uris(&cfg.extra_uris);
            return Ok(Self {
                dual: true,
                primary: None,
                primary_uri: String::new(),
                system,
                session,
                extra,
                extra_uri_labels,
                vm_locks,
            });
        }

        let uri = cfg.uri.clone();
        let conn = Connect::open(Some(&uri))
            .map_err(|e| LibvirtError::Connection(format!("Failed to connect to libvirt: {e}")))?;
        let (extra, extra_uri_labels) = Self::open_extra_uris(&cfg.extra_uris);
        Ok(Self {
            dual: false,
            primary: Some(UriSlot {
                uri: uri.clone(),
                conn: Arc::new(Mutex::new(conn)),
            }),
            primary_uri: uri,
            system: None,
            session: None,
            extra,
            extra_uri_labels,
            vm_locks,
        })
    }

    /// Acquire the async lock for `vm_name`, creating its slot on first use.
    ///
    /// Hold the returned guard around the whole read-XML → compute → mutate
    /// sequence for a device operation (CD-ROM insert/eject/detach, guest-agent
    /// channel setup, offline registry edits), not just the final libvirt call —
    /// the race is between two concurrent *reads* of the starting state, not
    /// just the writes.
    pub async fn lock_vm(&self, vm_name: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let handle = {
            let mut map = self.vm_locks.lock().unwrap_or_else(|e| e.into_inner());
            map.entry(vm_name.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        handle.lock_owned().await
    }

    fn open_extra_uris(uris: &[String]) -> (Vec<UriSlot>, Vec<String>) {
        let mut extra = Vec::new();
        let mut labels = Vec::new();
        for uri in uris {
            let uri = uri.trim();
            if uri.is_empty() {
                continue;
            }
            if let Err(e) = validate_migrate_uri(uri) {
                tracing::warn!("libvirt extra_uris: skip invalid URI: {e}");
                continue;
            }
            match Connect::open(Some(uri)) {
                Ok(c) => {
                    tracing::info!("libvirt extra URI connected: {uri}");
                    labels.push(connection_label(uri));
                    extra.push(UriSlot {
                        uri: uri.to_string(),
                        conn: Arc::new(Mutex::new(c)),
                    });
                }
                Err(e) => tracing::warn!("libvirt extra_uris: failed to open {uri}: {e}"),
            }
        }
        (extra, labels)
    }

    #[inline]
    pub fn dual_enabled(&self) -> bool {
        self.dual
    }

    #[inline]
    pub fn primary_uri_display(&self) -> &str {
        if self.dual {
            "(dual: qemu:///system + qemu:///session)"
        } else {
            &self.primary_uri
        }
    }

    /// URI string for subprocess tools (`virt-xml`, `virt-install`).
    pub fn virt_uri_for_target(&self, target: LibvirtTarget) -> String {
        match (self.dual, target) {
            (false, _) => self.primary_uri.clone(),
            (true, LibvirtTarget::Session) => "qemu:///session".into(),
            (true, _) => "qemu:///system".into(),
        }
    }

    /// Resolve optional `?connection=system|session` (Cockpit-style). Single-URI mode ignores it.
    pub fn resolve_query(&self, connection: Option<&str>) -> LibvirtTarget {
        if !self.dual {
            return LibvirtTarget::Primary;
        }
        match connection.unwrap_or("system") {
            "session" => LibvirtTarget::Session,
            _ => LibvirtTarget::System,
        }
    }

    pub fn default_target(&self) -> LibvirtTarget {
        if self.dual {
            LibvirtTarget::System
        } else {
            LibvirtTarget::Primary
        }
    }

    fn slot_for(&self, target: LibvirtTarget) -> Result<&UriSlot, LibvirtError> {
        match (self.dual, target) {
            (false, LibvirtTarget::Primary) | (false, LibvirtTarget::System) => {
                self.primary.as_ref()
            }
            (false, LibvirtTarget::Session) => None,
            (true, LibvirtTarget::Primary) | (true, LibvirtTarget::System) => self.system.as_ref(),
            (true, LibvirtTarget::Session) => self.session.as_ref(),
        }
        .ok_or_else(|| {
            LibvirtError::Connection(format!(
                "libvirt connection not available for target {target:?}"
            ))
        })
    }

    fn open_uri(uri: &str) -> Result<Connect, LibvirtError> {
        Connect::open(Some(uri)).map_err(|e| LibvirtError::Connection(e.to_string()))
    }

    fn with_slot<F, R>(slot: &UriSlot, f: F) -> Result<R, LibvirtError>
    where
        F: FnOnce(&Connect) -> Result<R, LibvirtError>,
    {
        let mut conn = slot.conn.lock().unwrap_or_else(|e| {
            tracing::warn!("Recovering from poisoned libvirt mutex");
            e.into_inner()
        });

        if conn.is_alive().unwrap_or(false) {
            return f(&conn);
        }

        tracing::warn!("Libvirt connection lost, reconnecting to {}", slot.uri);
        crate::obs_counters::inc_libvirt_reconnect();
        let _ = conn.close();
        match Self::open_uri(&slot.uri) {
            Ok(new_conn) => {
                *conn = new_conn;
                tracing::info!("Reconnected to libvirt ({})", slot.uri);
                f(&conn)
            }
            Err(e) => Err(LibvirtError::Connection(format!(
                "Failed to reconnect to {}: {e}",
                slot.uri
            ))),
        }
    }

    pub fn with_conn_target<F, R>(&self, target: LibvirtTarget, f: F) -> Result<R, LibvirtError>
    where
        F: FnOnce(&Connect) -> Result<R, LibvirtError>,
    {
        let slot = self.slot_for(target)?;
        Self::with_slot(slot, f)
    }

    /// Same as single-hypervisor default: configured URI, or **system** when dual.
    pub fn with_conn<F, R>(&self, f: F) -> Result<R, LibvirtError>
    where
        F: FnOnce(&Connect) -> Result<R, LibvirtError>,
    {
        self.with_conn_target(self.default_target(), f)
    }

    fn append_extra_vms(&self, out: &mut Vec<VmInfo>) {
        for (label, slot) in self.extra_uri_labels.iter().zip(self.extra.iter()) {
            match Self::with_slot(slot, domain::list_vms) {
                Ok(mut vms) => {
                    for vm in &mut vms {
                        vm.libvirt_connection = Some(label.clone());
                    }
                    out.extend(vms);
                }
                Err(e) => tracing::warn!("list_vms on {}: {e}", slot.uri),
            }
        }
    }

    pub fn list_all_vms(&self) -> Result<Vec<VmInfo>, LibvirtError> {
        let mut out = if !self.dual {
            self.with_conn_target(LibvirtTarget::Primary, domain::list_vms)?
        } else {
            let mut merged = Vec::new();
            if let Some(ref slot) = self.system {
                let mut vms = Self::with_slot(slot, domain::list_vms)?;
                for vm in &mut vms {
                    vm.libvirt_connection = Some("system".into());
                }
                merged.extend(vms);
            }
            if let Some(ref slot) = self.session {
                let mut vms = Self::with_slot(slot, domain::list_vms)?;
                for vm in &mut vms {
                    vm.libvirt_connection = Some("session".into());
                }
                merged.extend(vms);
            }
            merged
        };
        self.append_extra_vms(&mut out);
        Ok(out)
    }

    pub fn merge_all_metrics(&self) -> Result<Vec<VmMetrics>, LibvirtError> {
        let mut out = if !self.dual {
            self.with_conn_target(LibvirtTarget::Primary, metrics::get_all_vm_metrics)?
        } else {
            let mut merged = Vec::new();
            if let Some(ref slot) = self.system {
                let mut m = Self::with_slot(slot, metrics::get_all_vm_metrics)?;
                for x in &mut m {
                    x.libvirt_connection = Some("system".into());
                }
                merged.extend(m);
            }
            if let Some(ref slot) = self.session {
                let mut m = Self::with_slot(slot, metrics::get_all_vm_metrics)?;
                for x in &mut m {
                    x.libvirt_connection = Some("session".into());
                }
                merged.extend(m);
            }
            merged
        };
        for (label, slot) in self.extra_uri_labels.iter().zip(self.extra.iter()) {
            if let Ok(mut m) = Self::with_slot(slot, metrics::get_all_vm_metrics) {
                for x in &mut m {
                    x.libvirt_connection = Some(label.clone());
                }
                out.extend(m);
            }
        }
        Ok(out)
    }

    /// UI / discovery: dual mode and which sockets opened at daemon start.
    #[must_use]
    pub fn api_connection_summary(&self) -> serde_json::Value {
        let primary_connected = self.primary.is_some();
        let qemu_system_connected = self.system.is_some();
        let qemu_session_connected = self.session.is_some();
        serde_json::json!({
            "dual_connection": self.dual,
            "primary_connected": primary_connected,
            "qemu_system_connected": qemu_system_connected,
            "qemu_session_connected": qemu_session_connected,
            "libvirt_connected": primary_connected || qemu_system_connected || qemu_session_connected,
            "configured_uri": self.primary_uri_display(),
            "extra_uris": self.extra.iter().map(|s| s.uri.clone()).collect::<Vec<_>>(),
            "extra_uri_labels": self.extra_uri_labels.clone(),
        })
    }
}

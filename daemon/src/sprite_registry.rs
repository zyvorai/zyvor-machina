// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! In-memory registry + TTL reaper for disposable "sprite" microVMs.
//!
//! Mirrors `job_registry.rs`'s shape (`Arc<Mutex<HashMap<...>>>`, cleared on
//! daemon restart) rather than persisting to a database — sprites are
//! disposable by design, so losing track of them across a daemon restart is
//! an acceptable tradeoff, not a shortcut. See the sprites design doc for
//! why this isn't wired into the controller's `vms` table / reconciler at
//! all.
//!
//! Backend-agnostic: a sprite boots on libvirt/QEMU
//! (`machina_core::libvirt::sprite`), Cloud Hypervisor
//! (`machina_core::cloud_hypervisor::sprite`), or Firecracker
//! (`machina_core::firecracker::sprite`) — see `SpriteBackendHandle`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use machina_core::cloud_hypervisor::sprite::teardown_sprite_chv;
use machina_core::firecracker::sprite::teardown_sprite_fc;
use machina_core::libvirt::domain::{delete_vm_with_options, UndefineOptions};
use machina_core::LibvirtManager;
use machina_spec::{SpriteBackend, SpriteHandle, SpriteState};

/// Sized for a lot of short-lived sandboxes churning through, unlike
/// `JobRegistry`'s `MAX_JOBS=250` (long-running build/export jobs are much
/// rarer). Oldest-first eviction isn't implemented here (unlike
/// `JobRegistry`'s `order` deque) — a sprite past this count almost
/// certainly means the reaper has fallen behind or stalled, which is worth
/// surfacing as registration failures rather than silently dropping
/// bookkeeping for a VM that's still running.
const MAX_SPRITES: usize = 2000;

/// How often the reaper scans for expired sprites. Sprites are meant to
/// live seconds-to-minutes (see `spec::sprite`'s `MAX_TTL_SECONDS`), so this
/// trades a little teardown-latency slop for simplicity over a per-sprite
/// `tokio::time::sleep_until` timer — revisit only if TTL precision turns
/// out to matter in practice.
const REAP_INTERVAL: Duration = Duration::from_secs(5);

/// Guest vsock CIDs are arbitrated host-wide by the kernel's `vhost_vsock`
/// module regardless of hypervisor — 0/1/2 are reserved
/// (hypervisor/reserved/host), so assignable guest CIDs start at 3.
const FIRST_VSOCK_CID: u32 = 3;

/// How long a *suspended* sprite's snapshot artifacts are kept on disk
/// before the reaper deletes them. Deliberately not the original
/// `ttl_seconds` — a suspended sprite holds no RAM/vCPU, so its constraint
/// is disk-retention policy, a different (much coarser) concern than "how
/// long can this occupy host resources." 24h as a first cut; revisit if
/// suspend turns out to be used for anything longer-lived. Expressed in
/// seconds (not `chrono::Duration::hours`) to match `register()`'s existing
/// `chrono::Duration::seconds(..)` construction below.
const SUSPENDED_GRACE_SECONDS: i64 = 24 * 3600;

/// Identifies which hypervisor booted a sprite and what's needed to tear it
/// down. Libvirt sprites are supervised by libvirtd (`domain_name` is
/// enough to find and destroy the domain); Cloud Hypervisor and Firecracker
/// sprites are supervised directly by this daemon as a child process, so
/// their handles carry everything `teardown_sprite_chv`/`teardown_sprite_fc`
/// need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpriteBackendHandle {
    Libvirt {
        domain_name: String,
    },
    CloudHypervisor {
        pid: u32,
        api_socket: PathBuf,
        disk_path: PathBuf,
        vsock_socket: PathBuf,
        /// `Some` when the sprite was booted with `network_egress` — needed
        /// so teardown can remove the TAP device.
        tap_name: Option<String>,
    },
    Firecracker {
        pid: u32,
        api_socket: PathBuf,
        disk_path: PathBuf,
        vsock_socket: PathBuf,
        /// `Some` when the sprite was booted with `network_egress` — needed
        /// so teardown can remove the TAP device.
        tap_name: Option<String>,
    },
}

impl SpriteBackendHandle {
    /// The API-visible `SpriteBackend` this handle represents — derived from
    /// the variant itself (not a separately-tracked field) so it can never
    /// drift from which backend actually booted the sprite.
    fn kind(&self) -> SpriteBackend {
        match self {
            SpriteBackendHandle::Libvirt { .. } => SpriteBackend::Libvirt,
            SpriteBackendHandle::CloudHypervisor { .. } => SpriteBackend::CloudHypervisor,
            SpriteBackendHandle::Firecracker { .. } => SpriteBackend::Firecracker,
        }
    }

    /// Short label for reaper/audit log lines.
    pub(crate) fn describe(&self) -> String {
        match self {
            SpriteBackendHandle::Libvirt { domain_name } => domain_name.clone(),
            SpriteBackendHandle::CloudHypervisor { pid, .. } => {
                format!("cloud-hypervisor pid {pid}")
            }
            SpriteBackendHandle::Firecracker { pid, .. } => format!("firecracker pid {pid}"),
        }
    }
}

/// On-disk artifacts left behind once a sprite is suspended (snapshotted,
/// process killed) — enough for `restore_sprite_chv`/`_fc` to bring it back,
/// and for the reaper to clean up if it expires unrestored instead. No
/// libvirt variant: sprite suspend/snapshot is Cloud-Hypervisor/Firecracker
/// only for now (libvirt already has `virDomainSuspend`/snapshot support at
/// the libvirt layer — wiring sprite suspend through to it is a separate,
/// later change).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpriteSnapshotArtifacts {
    CloudHypervisor {
        snapshot_dir: PathBuf,
        disk_path: PathBuf,
        tap_name: Option<String>,
    },
    Firecracker {
        snapshot_path: PathBuf,
        mem_file_path: PathBuf,
        disk_path: PathBuf,
        tap_name: Option<String>,
    },
}

/// A sprite's current backend state: either a live, directly-supervised (or
/// libvirt-supervised) process (`Live`), or suspended to a snapshot with no
/// running process at all (`Suspended`). Replaces the old bare
/// `SpriteBackendHandle` as `SpriteEntry`'s backend field so a suspended
/// sprite — which has no pid/sockets — can be represented without adding an
/// `Option` to every `SpriteBackendHandle` variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendState {
    Live(SpriteBackendHandle),
    Suspended(SpriteSnapshotArtifacts),
}

impl BackendState {
    /// Short label for reaper/audit log lines — mirrors
    /// `SpriteBackendHandle::describe()` for the `Live` case.
    pub(crate) fn describe(&self) -> String {
        match self {
            BackendState::Live(h) => h.describe(),
            BackendState::Suspended(SpriteSnapshotArtifacts::CloudHypervisor { .. }) => {
                "cloud-hypervisor (suspended)".into()
            }
            BackendState::Suspended(SpriteSnapshotArtifacts::Firecracker { .. }) => {
                "firecracker (suspended)".into()
            }
        }
    }
}

struct SpriteEntry {
    handle: SpriteHandle,
    backend: BackendState,
    expires_at: DateTime<Utc>,
    /// Captured at `register()` — used as `restore()`'s default
    /// `ttl_seconds` when the restore request doesn't override it.
    ttl_seconds: u64,
}

#[derive(Clone)]
pub struct SpriteRegistry {
    inner: Arc<Mutex<HashMap<String, SpriteEntry>>>,
    /// CIDs currently in use by *either* backend — populated eagerly by
    /// `next_vsock_cid()` for Cloud Hypervisor (which must pick a CID before
    /// it can boot) and by `register()` for libvirt (whose CID is only known
    /// after boot, once its `<cid auto='yes'/>` assignment is read back from
    /// the running domain's XML). A single shared set is what actually
    /// closes the collision this replaced a blind per-backend counter for:
    /// two independent counters both starting at `FIRST_VSOCK_CID` reliably
    /// collided the first time each backend's first sprite booted around the
    /// same time (reproduced live — see the sprites design notes).
    claimed_vsock_cids: Arc<Mutex<HashSet<u32>>>,
}

impl Default for SpriteRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            claimed_vsock_cids: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

impl SpriteRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hand out a fresh guest vsock CID for a Cloud Hypervisor sprite,
    /// distinct from every CID any *currently running* sprite of either
    /// backend holds (checked against `claimed_vsock_cids`, which
    /// `register()` populates for libvirt sprites too — see that field's
    /// doc comment for why a shared set, not a per-backend counter, is what
    /// this needs). Claims the CID immediately, under the same lock as the
    /// scan, so two concurrent Cloud Hypervisor creations can never both
    /// pick the same value. Reused after a sprite is torn down (`remove()`
    /// releases its CID) — safe, since by then nothing holds it — but never
    /// explicitly released if the boot that requested it then fails; sprite
    /// churn stays well below `u32`'s range, so a CID leaked on a failed
    /// boot isn't worth the extra plumbing to reclaim.
    pub fn next_vsock_cid(&self) -> u32 {
        let mut claimed = self
            .claimed_vsock_cids
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut candidate = FIRST_VSOCK_CID;
        while claimed.contains(&candidate) {
            candidate += 1;
        }
        claimed.insert(candidate);
        candidate
    }

    /// Record a sprite whose backend is already running (boot happens
    /// synchronously in the route handler before this is called — there's
    /// no separate async "booting" phase to track).
    ///
    /// `sprite_id` must be the same id the caller used when booting (e.g.
    /// to derive `domain_name` via `sprite_domain_name()`, or to namespace
    /// a Cloud Hypervisor sprite's run directory) — minting a fresh id here
    /// instead would desync the id handed back to API clients from the
    /// actual running sprite, making `virsh`/manual ops on a returned id
    /// impossible.
    #[allow(clippy::too_many_arguments)]
    pub fn register(
        &self,
        sprite_id: String,
        backend: SpriteBackendHandle,
        ttl_seconds: u64,
        vsock_cid: Option<u32>,
        network_egress: bool,
        vcpus: u32,
        memory_mb: u64,
    ) -> Result<SpriteHandle, &'static str> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() >= MAX_SPRITES {
            return Err("sprite registry is full — the reaper may have fallen behind");
        }
        if let Some(cid) = vsock_cid {
            // Idempotent for Cloud Hypervisor (already claimed by
            // next_vsock_cid()); this is the only claim point for libvirt,
            // whose CID is only known after boot.
            self.claimed_vsock_cids
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(cid);
        }
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(ttl_seconds as i64);
        let handle = SpriteHandle {
            sprite_id: sprite_id.clone(),
            state: SpriteState::Running,
            created_at: now.to_rfc3339(),
            expires_at: expires_at.to_rfc3339(),
            vsock_cid,
            backend: backend.kind(),
            network_egress,
            vcpus,
            memory_mb,
            suspended_at: None,
        };
        g.insert(
            sprite_id,
            SpriteEntry {
                handle: handle.clone(),
                backend: BackendState::Live(backend),
                expires_at,
                ttl_seconds,
            },
        );
        Ok(handle)
    }

    pub fn get(&self, id: &str) -> Option<SpriteHandle> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.get(id).map(|e| e.handle.clone())
    }

    pub fn list(&self) -> Vec<SpriteHandle> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.values().map(|e| e.handle.clone()).collect()
    }

    /// Non-removing clone of a sprite's current backend state — used by the
    /// pause/resume/resize/snapshot/restore route handlers to find the pid/
    /// sockets (or snapshot artifacts) they need without tearing the
    /// registry entry down the way `remove()` does.
    pub fn peek_backend(&self, id: &str) -> Option<BackendState> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.get(id).map(|e| e.backend.clone())
    }

    /// Remove the bookkeeping entry and return its backend state for the
    /// caller to actually tear down. Bookkeeping-only: does not touch
    /// libvirt or spawn any process itself, so it's safe to call from
    /// either the reaper or an explicit `DELETE /v1/sprites/{id}` handler
    /// without a teardown call in the registry's lock scope. Releases the
    /// removed sprite's vsock CID (if any) back to `claimed_vsock_cids` so
    /// `next_vsock_cid()` can hand it out again — including for a
    /// `Suspended` entry, which keeps its CID claimed for the whole
    /// suspended interval (see `mark_suspended`'s doc comment).
    pub fn remove(&self, id: &str) -> Option<BackendState> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = g.remove(id)?;
        if let Some(cid) = entry.handle.vsock_cid {
            self.claimed_vsock_cids
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&cid);
        }
        Some(entry.backend)
    }

    /// `Running` -> `Paused`. `expires_at`/TTL is untouched — a paused
    /// sprite still holds its full allocation, so it keeps counting down
    /// normally (see `SUSPENDED_GRACE_SECONDS`'s doc comment for why
    /// `Suspended` is different).
    pub fn mark_paused(&self, id: &str) -> Result<SpriteHandle, &'static str> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = g.get_mut(id).ok_or("sprite not found")?;
        entry.handle.state = SpriteState::Paused;
        Ok(entry.handle.clone())
    }

    /// `Paused` -> `Running`.
    pub fn mark_running(&self, id: &str) -> Result<SpriteHandle, &'static str> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = g.get_mut(id).ok_or("sprite not found")?;
        entry.handle.state = SpriteState::Running;
        Ok(entry.handle.clone())
    }

    /// `Live(..)` -> `Suspended(artifacts)`. Replaces `expires_at` with a
    /// fresh `SUSPENDED_GRACE_SECONDS` deadline (a disk-retention window,
    /// not a resumption of the original TTL countdown — see that const's
    /// doc comment) and sets `suspended_at`. Deliberately does **not**
    /// release `vsock_cid` — kept claimed for the whole suspended interval
    /// so `restore()` can hand back the exact same CID a caller may already
    /// have baked into a guest-side vsock connector.
    pub fn mark_suspended(
        &self,
        id: &str,
        artifacts: SpriteSnapshotArtifacts,
    ) -> Result<SpriteHandle, &'static str> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = g.get_mut(id).ok_or("sprite not found")?;
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(SUSPENDED_GRACE_SECONDS);
        entry.backend = BackendState::Suspended(artifacts);
        entry.expires_at = expires_at;
        entry.handle.state = SpriteState::Suspended;
        entry.handle.expires_at = expires_at.to_rfc3339();
        entry.handle.suspended_at = Some(now.to_rfc3339());
        Ok(entry.handle.clone())
    }

    /// `Suspended(..)` -> `Live(new_backend)`. Gets a **fresh** full TTL
    /// window (`now + ttl_seconds`, defaulting to the sprite's original
    /// create-time `ttl_seconds` when the restore request didn't override
    /// it) — restore is "boot again, from a snapshot," so it behaves like
    /// `create` TTL-wise, not like a paused countdown resuming mid-flight.
    pub fn mark_restored(
        &self,
        id: &str,
        new_backend: SpriteBackendHandle,
        ttl_seconds: Option<u64>,
    ) -> Result<SpriteHandle, &'static str> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = g.get_mut(id).ok_or("sprite not found")?;
        let ttl = ttl_seconds.unwrap_or(entry.ttl_seconds);
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(ttl as i64);
        entry.backend = BackendState::Live(new_backend);
        entry.expires_at = expires_at;
        entry.ttl_seconds = ttl;
        entry.handle.state = SpriteState::Running;
        entry.handle.expires_at = expires_at.to_rfc3339();
        entry.handle.suspended_at = None;
        Ok(entry.handle.clone())
    }

    /// Cloud-Hypervisor-resize bookkeeping only — updates the API-visible
    /// `vcpus`/`memory_mb` after `resize_sprite_chv` has already applied the
    /// change live. Does not touch `state`/`expires_at`.
    pub fn update_sizing(
        &self,
        id: &str,
        vcpus: Option<u32>,
        memory_mb: Option<u64>,
    ) -> Result<SpriteHandle, &'static str> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = g.get_mut(id).ok_or("sprite not found")?;
        if let Some(v) = vcpus {
            entry.handle.vcpus = v;
        }
        if let Some(m) = memory_mb {
            entry.handle.memory_mb = m;
        }
        Ok(entry.handle.clone())
    }

    /// Sprite ids whose TTL has passed.
    fn expired_ids(&self) -> Vec<String> {
        let now = Utc::now();
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.iter()
            .filter(|(_, e)| e.expires_at <= now)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// Tear down one sprite, dispatching on which backend booted it. Shared by
/// the reaper loop and the explicit `DELETE /v1/sprites/{id}` handler so
/// both paths do exactly the same cleanup.
///
/// Libvirt: `delete_disks: true` only ever unlinks the domain's *own*
/// `<source file>` (the overlay) — never the golden image, whose path lives
/// inside the overlay qcow2's internal backing header, not in domain XML
/// (verified by reading `domain::collect_disk_paths`). Correctly a no-op
/// (`Ok`) if the domain is already gone — see `delete_vm_with_options`'s
/// `NotFound` short-circuit — so a reaper retry after a partial failure is
/// safe.
///
/// Cloud Hypervisor and Firecracker: `teardown_sprite_chv`/`teardown_sprite_fc`
/// apply the same "already gone is success" rule for the same reason.
pub async fn teardown_sprite(
    manager: &LibvirtManager,
    backend: BackendState,
) -> Result<(), String> {
    let backend = match backend {
        BackendState::Live(live) => live,
        BackendState::Suspended(SpriteSnapshotArtifacts::CloudHypervisor {
            snapshot_dir,
            disk_path,
            ..
        }) => {
            let _ = std::fs::remove_dir_all(&snapshot_dir);
            let _ = std::fs::remove_file(&disk_path);
            if let Some(run_dir) = disk_path.parent() {
                let _ = std::fs::remove_dir_all(run_dir);
            }
            return Ok(());
        }
        BackendState::Suspended(SpriteSnapshotArtifacts::Firecracker {
            snapshot_path,
            mem_file_path,
            disk_path,
            ..
        }) => {
            let _ = std::fs::remove_file(&snapshot_path);
            let _ = std::fs::remove_file(&mem_file_path);
            let _ = std::fs::remove_file(&disk_path);
            if let Some(run_dir) = disk_path.parent() {
                let _ = std::fs::remove_dir_all(run_dir);
            }
            return Ok(());
        }
    };
    match backend {
        SpriteBackendHandle::Libvirt { domain_name } => {
            let mgr = manager.clone();
            tokio::task::spawn_blocking(move || {
                mgr.with_conn(|conn| {
                    delete_vm_with_options(
                        conn,
                        &domain_name,
                        &UndefineOptions {
                            delete_disks: true,
                            ..Default::default()
                        },
                    )
                })
            })
            .await
            .map_err(|e| format!("teardown task join error: {e}"))?
            .map_err(|e| e.to_string())?;
            Ok(())
        }
        SpriteBackendHandle::CloudHypervisor {
            pid,
            api_socket,
            disk_path,
            vsock_socket,
            tap_name,
        } => {
            teardown_sprite_chv(
                pid,
                &api_socket,
                &disk_path,
                &vsock_socket,
                tap_name.as_deref(),
            )
            .await
        }
        SpriteBackendHandle::Firecracker {
            pid,
            api_socket,
            disk_path,
            vsock_socket,
            tap_name,
        } => {
            teardown_sprite_fc(
                pid,
                &api_socket,
                &disk_path,
                &vsock_socket,
                tap_name.as_deref(),
            )
            .await
        }
    }
}

/// Background loop: every `REAP_INTERVAL`, tear down any sprite past its
/// `expires_at`. Started once from `server::create_app` alongside the
/// daemon's other background workers (`ObservabilityWorkers`,
/// `systemd::spawn_watchdog_pinger`).
pub fn spawn_reaper(manager: LibvirtManager, registry: SpriteRegistry) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(REAP_INTERVAL);
        loop {
            interval.tick().await;
            for id in registry.expired_ids() {
                let Some(backend) = registry.remove(&id) else {
                    // Raced with an explicit DELETE that already removed it.
                    continue;
                };
                let label = backend.describe();
                match teardown_sprite(&manager, backend).await {
                    Ok(()) => tracing::info!("sprite reaper: tore down {id} ({label})"),
                    Err(e) => {
                        tracing::warn!("sprite reaper: failed to tear down {id} ({label}): {e}")
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn libvirt_backend(domain_name: &str) -> SpriteBackendHandle {
        SpriteBackendHandle::Libvirt {
            domain_name: domain_name.into(),
        }
    }

    fn chv_backend(pid: u32) -> SpriteBackendHandle {
        SpriteBackendHandle::CloudHypervisor {
            pid,
            api_socket: PathBuf::from(format!("/var/lib/machina/sprite-run/{pid}/api.sock")),
            disk_path: PathBuf::from(format!("/var/lib/machina/sprite-run/{pid}/overlay.qcow2")),
            vsock_socket: PathBuf::from(format!("/var/lib/machina/sprite-run/{pid}/vsock.sock")),
            tap_name: None,
        }
    }

    fn fc_backend(pid: u32) -> SpriteBackendHandle {
        SpriteBackendHandle::Firecracker {
            pid,
            api_socket: PathBuf::from(format!("/var/lib/machina/sprite-run/{pid}/api.sock")),
            disk_path: PathBuf::from(format!("/var/lib/machina/sprite-run/{pid}/disk.raw")),
            vsock_socket: PathBuf::from(format!("/var/lib/machina/sprite-run/{pid}/vsock.sock")),
            tap_name: None,
        }
    }

    fn chv_snapshot_artifacts(pid: u32) -> SpriteSnapshotArtifacts {
        SpriteSnapshotArtifacts::CloudHypervisor {
            snapshot_dir: PathBuf::from(format!("/var/lib/machina/sprite-run/{pid}/snapshot")),
            disk_path: PathBuf::from(format!("/var/lib/machina/sprite-run/{pid}/disk.qcow2")),
            tap_name: None,
        }
    }

    #[test]
    fn register_then_get_round_trips() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register(
                "abc".into(),
                libvirt_backend("sprite-abc"),
                300,
                Some(3),
                false,
                1,
                512,
            )
            .unwrap();
        assert_eq!(handle.sprite_id, "abc");
        let fetched = reg
            .get(&handle.sprite_id)
            .expect("registered sprite present");
        assert_eq!(fetched.sprite_id, handle.sprite_id);
        assert_eq!(fetched.vsock_cid, Some(3));
        assert_eq!(fetched.state, SpriteState::Running);
        assert_eq!(fetched.backend, SpriteBackend::Libvirt);
        assert_eq!(fetched.vcpus, 1);
        assert_eq!(fetched.memory_mb, 512);
    }

    #[test]
    fn register_reports_network_egress() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register(
                "egress1".into(),
                libvirt_backend("sprite-egress1"),
                300,
                Some(3),
                true,
                1,
                512,
            )
            .unwrap();
        assert!(handle.network_egress);
        assert!(reg.get(&handle.sprite_id).unwrap().network_egress);
    }

    #[test]
    fn register_reports_cloud_hypervisor_backend_kind() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register(
                "chv-kind".into(),
                chv_backend(1),
                300,
                Some(3),
                false,
                1,
                512,
            )
            .unwrap();
        assert_eq!(handle.backend, SpriteBackend::CloudHypervisor);
    }

    #[test]
    fn register_reports_firecracker_backend_kind() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register("fc-kind".into(), fc_backend(1), 300, Some(3), false, 1, 512)
            .unwrap();
        assert_eq!(handle.backend, SpriteBackend::Firecracker);
    }

    #[test]
    fn get_unknown_id_is_none() {
        let reg = SpriteRegistry::new();
        assert!(reg.get("does-not-exist").is_none());
    }

    #[test]
    fn remove_returns_backend_handle_once() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register(
                "xyz".into(),
                libvirt_backend("sprite-xyz"),
                300,
                None,
                false,
                1,
                512,
            )
            .unwrap();
        assert_eq!(
            reg.remove(&handle.sprite_id),
            Some(BackendState::Live(libvirt_backend("sprite-xyz")))
        );
        // Second remove is a no-op, not an error — the reaper and an explicit
        // DELETE could race on the same id.
        assert_eq!(reg.remove(&handle.sprite_id), None);
        assert!(reg.get(&handle.sprite_id).is_none());
    }

    #[test]
    fn register_and_remove_round_trips_cloud_hypervisor_backend() {
        let reg = SpriteRegistry::new();
        let backend = chv_backend(4242);
        let handle = reg
            .register("chv1".into(), backend.clone(), 300, Some(7), false, 1, 512)
            .unwrap();
        assert_eq!(reg.get(&handle.sprite_id).unwrap().vsock_cid, Some(7));
        assert_eq!(
            reg.remove(&handle.sprite_id),
            Some(BackendState::Live(backend))
        );
    }

    #[test]
    fn register_and_remove_round_trips_firecracker_backend() {
        let reg = SpriteRegistry::new();
        let backend = fc_backend(5252);
        let handle = reg
            .register("fc1".into(), backend.clone(), 300, Some(8), false, 1, 512)
            .unwrap();
        assert_eq!(reg.get(&handle.sprite_id).unwrap().vsock_cid, Some(8));
        assert_eq!(
            reg.remove(&handle.sprite_id),
            Some(BackendState::Live(backend))
        );
    }

    #[test]
    fn expired_ids_only_returns_past_ttl() {
        let reg = SpriteRegistry::new();
        let long_lived = reg
            .register(
                "long".into(),
                libvirt_backend("sprite-long"),
                3600,
                None,
                false,
                1,
                512,
            )
            .unwrap();
        let already_expired = reg
            .register(
                "expired".into(),
                libvirt_backend("sprite-expired"),
                0,
                None,
                false,
                1,
                512,
            )
            .unwrap();
        // ttl_seconds=0 means expires_at == created_at, which is <= now by
        // the time expired_ids() runs.
        let expired = reg.expired_ids();
        assert!(expired.contains(&already_expired.sprite_id));
        assert!(!expired.contains(&long_lived.sprite_id));
    }

    #[test]
    fn list_returns_all_registered_handles() {
        let reg = SpriteRegistry::new();
        reg.register(
            "a".into(),
            libvirt_backend("sprite-a"),
            300,
            None,
            false,
            1,
            512,
        )
        .unwrap();
        reg.register("b".into(), chv_backend(99), 300, None, false, 1, 512)
            .unwrap();
        reg.register("c".into(), fc_backend(100), 300, None, false, 1, 512)
            .unwrap();
        assert_eq!(reg.list().len(), 3);
    }

    #[test]
    fn next_vsock_cid_hands_out_distinct_increasing_values() {
        let reg = SpriteRegistry::new();
        let a = reg.next_vsock_cid();
        let b = reg.next_vsock_cid();
        let c = reg.next_vsock_cid();
        assert!(a >= FIRST_VSOCK_CID);
        assert!(b > a);
        assert!(c > b);
    }

    /// Reproduces a real collision seen live: a libvirt sprite's
    /// kernel-auto-assigned CID and a Cloud Hypervisor sprite's
    /// registry-allocated CID both independently landed on
    /// `FIRST_VSOCK_CID` because each backend's allocator started fresh.
    /// `next_vsock_cid()` must see the libvirt sprite's already-registered
    /// CID and skip past it, not just avoid its own prior allocations.
    #[test]
    fn next_vsock_cid_skips_a_cid_a_registered_libvirt_sprite_already_holds() {
        let reg = SpriteRegistry::new();
        reg.register(
            "libvirt-first".into(),
            libvirt_backend("sprite-libvirt-first"),
            300,
            Some(FIRST_VSOCK_CID),
            false,
            1,
            512,
        )
        .unwrap();

        let allocated = reg.next_vsock_cid();
        assert_ne!(allocated, FIRST_VSOCK_CID);
    }

    #[test]
    fn removing_a_sprite_frees_its_vsock_cid_for_reuse() {
        let reg = SpriteRegistry::new();
        let cid = reg.next_vsock_cid();
        let handle = reg
            .register("chv1".into(), chv_backend(1), 300, Some(cid), false, 1, 512)
            .unwrap();
        reg.remove(&handle.sprite_id);

        // The registry has no other claims left, so the freed CID is the
        // lowest one available again.
        assert_eq!(reg.next_vsock_cid(), cid);
    }

    #[test]
    fn pause_then_resume_round_trips_state() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register("p1".into(), chv_backend(1), 300, Some(3), false, 1, 512)
            .unwrap();
        let expires_before = handle.expires_at.clone();

        let paused = reg.mark_paused(&handle.sprite_id).unwrap();
        assert_eq!(paused.state, SpriteState::Paused);
        // Pausing must not touch the TTL — see mark_paused's doc comment.
        assert_eq!(paused.expires_at, expires_before);

        let resumed = reg.mark_running(&handle.sprite_id).unwrap();
        assert_eq!(resumed.state, SpriteState::Running);
    }

    #[test]
    fn mark_paused_unknown_id_errors() {
        let reg = SpriteRegistry::new();
        assert!(reg.mark_paused("does-not-exist").is_err());
    }

    #[test]
    fn suspend_replaces_ttl_and_keeps_vsock_cid_claimed() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register("s1".into(), chv_backend(1), 60, Some(3), false, 1, 512)
            .unwrap();
        let expires_before = handle.expires_at.clone();

        let suspended = reg
            .mark_suspended(&handle.sprite_id, chv_snapshot_artifacts(1))
            .unwrap();
        assert_eq!(suspended.state, SpriteState::Suspended);
        assert!(suspended.suspended_at.is_some());
        // Suspend replaces the TTL deadline with SUSPENDED_GRACE_SECONDS —
        // a 24h grace window is always later than a 60s original TTL.
        assert_ne!(suspended.expires_at, expires_before);

        // The CID stays claimed through suspension — next_vsock_cid() must
        // not hand it out to a concurrent create.
        assert_ne!(reg.next_vsock_cid(), 3);

        assert_eq!(
            reg.peek_backend(&handle.sprite_id),
            Some(BackendState::Suspended(chv_snapshot_artifacts(1)))
        );
    }

    #[test]
    fn restore_gets_a_fresh_ttl_defaulting_to_original() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register("r1".into(), chv_backend(1), 60, Some(3), false, 1, 512)
            .unwrap();
        reg.mark_suspended(&handle.sprite_id, chv_snapshot_artifacts(1))
            .unwrap();

        let restored = reg
            .mark_restored(&handle.sprite_id, chv_backend(2), None)
            .unwrap();
        assert_eq!(restored.state, SpriteState::Running);
        assert!(restored.suspended_at.is_none());
        assert_eq!(
            reg.peek_backend(&handle.sprite_id),
            Some(BackendState::Live(chv_backend(2)))
        );
    }

    #[test]
    fn restore_honors_an_explicit_ttl_override() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register("r2".into(), chv_backend(1), 60, Some(3), false, 1, 512)
            .unwrap();
        reg.mark_suspended(&handle.sprite_id, chv_snapshot_artifacts(1))
            .unwrap();

        let short = reg
            .mark_restored(&handle.sprite_id, chv_backend(2), Some(1))
            .unwrap();
        // A 1-second TTL expires almost immediately — the default (60s)
        // would not.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(reg.expired_ids().contains(&short.sprite_id));
    }

    #[test]
    fn update_sizing_only_touches_requested_fields() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register("z1".into(), chv_backend(1), 300, Some(3), false, 1, 512)
            .unwrap();

        let resized = reg.update_sizing(&handle.sprite_id, Some(4), None).unwrap();
        assert_eq!(resized.vcpus, 4);
        assert_eq!(resized.memory_mb, 512);

        let resized_again = reg
            .update_sizing(&handle.sprite_id, None, Some(2048))
            .unwrap();
        assert_eq!(resized_again.vcpus, 4);
        assert_eq!(resized_again.memory_mb, 2048);
    }

    #[test]
    fn peek_backend_does_not_remove_the_entry() {
        let reg = SpriteRegistry::new();
        let handle = reg
            .register("pk1".into(), chv_backend(1), 300, Some(3), false, 1, 512)
            .unwrap();
        assert!(reg.peek_backend(&handle.sprite_id).is_some());
        // Still there — peek_backend must not consume the entry the way
        // remove() does.
        assert!(reg.get(&handle.sprite_id).is_some());
    }
}

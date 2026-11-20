// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};

use crate::error::SpecError;
use crate::vm::validate_name;

/// Request body for `POST /v1/sprites`. Deliberately **not** a reuse of
/// [`crate::VirtualMachine`]: sprites are throwaway, headless, single-host
/// sandboxes — HA/backup/Atlas-storage/network-attachment/cloud-init fields
/// on `VmSpec` are either meaningless or actively wrong here (e.g. `ha`
/// would insert an `ha_policies` row for a VM that's gone in seconds).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpriteCreateRequest {
    /// Key into the daemon's golden-image registry (not an arbitrary
    /// filesystem path — see `daemon/src/routes/sprites.rs`).
    pub golden_image: String,
    #[serde(default = "default_vcpus")]
    pub vcpus: u32,
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u64,
    #[serde(default = "default_ttl_seconds")]
    pub ttl_seconds: u64,
    #[serde(default)]
    pub backend: SpriteBackend,
    /// Attach to the host's existing libvirt "default" NAT network
    /// (`virbr0`) for outbound-only internet access. Off by default —
    /// sprites stay vsock-only unless a caller explicitly opts in. Shares
    /// the same network (and posture) any regular VM created on this host
    /// already gets; there's no per-sprite isolation or domain allow-list
    /// (a `vm:` scoped native eBPF `tc_allow` policy is the way to add one).
    #[serde(default)]
    pub network_egress: bool,
}

/// Which hypervisor boots the sprite. Defaults to `Libvirt` so existing
/// callers/tests that don't set this field are unaffected.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpriteBackend {
    #[default]
    Libvirt,
    CloudHypervisor,
    Firecracker,
}

fn default_vcpus() -> u32 {
    1
}
fn default_memory_mb() -> u64 {
    512
}
fn default_ttl_seconds() -> u64 {
    300
}

/// Upper bounds for sprite sizing — deliberately far below `VirtualMachine`'s
/// (64 sockets/128 cores, 64 TiB memory): a sandbox that needs durable-VM-scale
/// resources isn't a sprite, it's a regular VM and should go through the
/// normal `POST /v1/vms` path instead.
const MAX_SPRITE_VCPUS: u32 = 8;
const MAX_SPRITE_MEMORY_MB: u64 = 8192;
/// 1 hour. Sprites are meant to live seconds-to-minutes; this is a hard
/// ceiling against a caller forgetting to set `ttl_seconds` and effectively
/// creating a durable VM the reconciler will never know exists (see
/// `daemon/src/routes/sprites.rs` reaper).
const MAX_TTL_SECONDS: u64 = 3600;

impl SpriteCreateRequest {
    pub fn validate(&self) -> Result<(), SpecError> {
        // `golden_image` ends up as a filename component
        // (`core::libvirt::sprite::resolve_golden_image` joins it under a
        // fixed directory) — `validate_golden_image_key` enforces the same
        // alphanumeric/-/_/ charset as a VM name, which structurally rules
        // out '/' or '..' path-traversal segments. `core` independently
        // re-validates with its own `validate::validate_name` before
        // touching the filesystem (see `resolve_golden_image`); this earlier
        // check exists so a malformed key is rejected as a 400 at the API
        // boundary instead of surfacing as an opaque 500/404 further down.
        validate_golden_image_key(&self.golden_image)?;
        if self.vcpus == 0 {
            return Err(SpecError::Validation("vcpus must be >= 1".into()));
        }
        if self.vcpus > MAX_SPRITE_VCPUS {
            return Err(SpecError::Validation(format!(
                "vcpus must be <= {MAX_SPRITE_VCPUS} for a sprite (use a regular VM for larger workloads)"
            )));
        }
        if self.memory_mb == 0 {
            return Err(SpecError::Validation("memory_mb must be >= 1".into()));
        }
        if self.memory_mb > MAX_SPRITE_MEMORY_MB {
            return Err(SpecError::Validation(format!(
                "memory_mb must be <= {MAX_SPRITE_MEMORY_MB} for a sprite (use a regular VM for larger workloads)"
            )));
        }
        if self.ttl_seconds == 0 {
            return Err(SpecError::Validation("ttl_seconds must be >= 1".into()));
        }
        if self.ttl_seconds > MAX_TTL_SECONDS {
            return Err(SpecError::Validation(format!(
                "ttl_seconds must be <= {MAX_TTL_SECONDS} (1 hour) for a sprite"
            )));
        }
        Ok(())
    }
}

/// Request body for `POST /v1/sprites/{id}/resize`. Cloud-Hypervisor-only —
/// Firecracker has no live-resize API. At least one field must be set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SpriteResizeRequest {
    #[serde(default)]
    pub vcpus: Option<u32>,
    #[serde(default)]
    pub memory_mb: Option<u64>,
}

impl SpriteResizeRequest {
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.vcpus.is_none() && self.memory_mb.is_none() {
            return Err(SpecError::Validation(
                "resize request must set vcpus and/or memory_mb".into(),
            ));
        }
        if let Some(vcpus) = self.vcpus {
            if vcpus == 0 {
                return Err(SpecError::Validation("vcpus must be >= 1".into()));
            }
            if vcpus > MAX_SPRITE_VCPUS {
                return Err(SpecError::Validation(format!(
                    "vcpus must be <= {MAX_SPRITE_VCPUS} for a sprite"
                )));
            }
        }
        if let Some(memory_mb) = self.memory_mb {
            if memory_mb == 0 {
                return Err(SpecError::Validation("memory_mb must be >= 1".into()));
            }
            if memory_mb > MAX_SPRITE_MEMORY_MB {
                return Err(SpecError::Validation(format!(
                    "memory_mb must be <= {MAX_SPRITE_MEMORY_MB} for a sprite"
                )));
            }
        }
        Ok(())
    }
}

/// Request body for `POST /v1/sprites/{id}/restore`. Empty body is valid —
/// `ttl_seconds` defaults to the sprite's original create-time value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SpriteRestoreRequest {
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

impl SpriteRestoreRequest {
    pub fn validate(&self) -> Result<(), SpecError> {
        if let Some(ttl_seconds) = self.ttl_seconds {
            if ttl_seconds == 0 {
                return Err(SpecError::Validation("ttl_seconds must be >= 1".into()));
            }
            if ttl_seconds > MAX_TTL_SECONDS {
                return Err(SpecError::Validation(format!(
                    "ttl_seconds must be <= {MAX_TTL_SECONDS} (1 hour) for a sprite"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SpriteState {
    Booting,
    Running,
    /// Process alive, not executing — still holds its full vcpu/memory
    /// allocation, still counts against its original `ttl_seconds`.
    Paused,
    /// Snapshotted to disk, VMM process killed. `expires_at` at this point
    /// is a disk-retention deadline (`SUSPENDED_GRACE`), not the original
    /// TTL — see `daemon/src/sprite_registry.rs`.
    Suspended,
    Reaping,
    Gone,
}

/// Returned by `POST /v1/sprites` and `GET /v1/sprites/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpriteHandle {
    pub sprite_id: String,
    pub state: SpriteState,
    /// RFC3339.
    pub created_at: String,
    /// RFC3339. `daemon`'s reaper tears the sprite down once this passes.
    /// Meaning depends on `state` — see `SpriteState::Suspended`.
    pub expires_at: String,
    /// `None` until the domain is defined and a CID has been assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vsock_cid: Option<u32>,
    /// Which hypervisor this sprite actually booted on — echoes the
    /// request's `backend` (or its default), not re-derived from anything
    /// else, so it stays correct even if a future backend shares a trait
    /// like "has a vsock_cid" with an existing one.
    #[serde(default)]
    pub backend: SpriteBackend,
    /// Echoes the request's `network_egress` — whether this sprite is
    /// attached to the host's "default" NAT network.
    #[serde(default)]
    pub network_egress: bool,
    /// Current vcpu count — echoes the create/resize request. Only
    /// meaningful while `state` is `Booting`/`Running`/`Paused`.
    #[serde(default = "default_vcpus")]
    pub vcpus: u32,
    /// Current memory size in MiB — echoes the create/resize request.
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u64,
    /// RFC3339. `Some` only while `state == Suspended`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended_at: Option<String>,
}

/// Domain name a sprite's libvirt domain is created/looked-up under.
/// Centralized here (rather than inlined at each call site in
/// `daemon/src/routes/sprites.rs`) so the `"sprite-"` prefix — which the
/// reaper and any future admin tooling can use to recognize/filter sprite
/// domains apart from regular VMs in `virsh list` — stays a single source of
/// truth.
pub fn sprite_domain_name(sprite_id: &str) -> String {
    format!("sprite-{sprite_id}")
}

/// Validate a `golden_image` registry key using the same name rules as a VM
/// name (`crate::vm::validate_name`) — it's used as a lookup key and (in the
/// domain-name prefix style above) may end up embedded in filesystem paths,
/// so the same alphanumeric/`-`/`_` restriction applies.
pub fn validate_golden_image_key(key: &str) -> Result<(), SpecError> {
    validate_name(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_request() -> SpriteCreateRequest {
        SpriteCreateRequest {
            golden_image: "python-minimal".into(),
            vcpus: default_vcpus(),
            memory_mb: default_memory_mb(),
            ttl_seconds: default_ttl_seconds(),
            backend: SpriteBackend::default(),
            network_egress: false,
        }
    }

    #[test]
    fn validate_golden_path() {
        assert!(base_request().validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_golden_image() {
        let mut req = base_request();
        req.golden_image = "".into();
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_path_traversal_in_golden_image() {
        // golden_image becomes a filename component in
        // core::libvirt::sprite::resolve_golden_image — a key containing '/'
        // or '..' must never reach the filesystem.
        let mut req = base_request();
        req.golden_image = "../../etc/passwd".into();
        assert!(req.validate().is_err());
        req.golden_image = "images/python-minimal".into();
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_vcpus() {
        let mut req = base_request();
        req.vcpus = 0;
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_oversized_vcpus() {
        let mut req = base_request();
        req.vcpus = MAX_SPRITE_VCPUS + 1;
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_memory() {
        let mut req = base_request();
        req.memory_mb = 0;
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_oversized_memory() {
        let mut req = base_request();
        req.memory_mb = MAX_SPRITE_MEMORY_MB + 1;
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_ttl() {
        let mut req = base_request();
        req.ttl_seconds = 0;
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_oversized_ttl() {
        let mut req = base_request();
        req.ttl_seconds = MAX_TTL_SECONDS + 1;
        assert!(req.validate().is_err());
    }

    #[test]
    fn backend_defaults_to_libvirt_when_omitted() {
        let req: SpriteCreateRequest =
            serde_json::from_str(r#"{"golden_image":"python-minimal"}"#).unwrap();
        assert_eq!(req.backend, SpriteBackend::Libvirt);
    }

    #[test]
    fn backend_round_trips_cloud_hypervisor() {
        let mut req = base_request();
        req.backend = SpriteBackend::CloudHypervisor;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""backend":"cloudhypervisor""#));
        let round_tripped: SpriteCreateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped.backend, SpriteBackend::CloudHypervisor);
    }

    #[test]
    fn backend_round_trips_firecracker() {
        let mut req = base_request();
        req.backend = SpriteBackend::Firecracker;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""backend":"firecracker""#));
        let round_tripped: SpriteCreateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped.backend, SpriteBackend::Firecracker);
    }

    #[test]
    fn network_egress_defaults_to_false_when_omitted() {
        let req: SpriteCreateRequest =
            serde_json::from_str(r#"{"golden_image":"python-minimal"}"#).unwrap();
        assert!(!req.network_egress);
    }

    #[test]
    fn network_egress_round_trips_true() {
        let mut req = base_request();
        req.network_egress = true;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""network_egress":true"#));
        let round_tripped: SpriteCreateRequest = serde_json::from_str(&json).unwrap();
        assert!(round_tripped.network_egress);
    }

    #[test]
    fn sprite_domain_name_has_prefix() {
        assert_eq!(sprite_domain_name("abc123"), "sprite-abc123");
    }

    #[test]
    fn handle_round_trips_without_vsock_cid() {
        let handle = SpriteHandle {
            sprite_id: "abc123".into(),
            state: SpriteState::Booting,
            created_at: "2026-01-01T00:00:00Z".into(),
            expires_at: "2026-01-01T00:05:00Z".into(),
            vsock_cid: None,
            backend: SpriteBackend::Libvirt,
            network_egress: false,
            vcpus: default_vcpus(),
            memory_mb: default_memory_mb(),
            suspended_at: None,
        };
        let json = serde_json::to_string(&handle).unwrap();
        assert!(!json.contains("vsock_cid"));
        assert!(!json.contains("suspended_at"));
        let round_tripped: SpriteHandle = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, handle);
    }

    #[test]
    fn resize_validate_rejects_empty_request() {
        assert!(SpriteResizeRequest::default().validate().is_err());
    }

    #[test]
    fn resize_validate_accepts_vcpus_only() {
        let req = SpriteResizeRequest {
            vcpus: Some(2),
            memory_mb: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn resize_validate_rejects_oversized_vcpus() {
        let req = SpriteResizeRequest {
            vcpus: Some(MAX_SPRITE_VCPUS + 1),
            memory_mb: None,
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn resize_validate_rejects_oversized_memory() {
        let req = SpriteResizeRequest {
            vcpus: None,
            memory_mb: Some(MAX_SPRITE_MEMORY_MB + 1),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn resize_validate_rejects_zero_values() {
        assert!(SpriteResizeRequest {
            vcpus: Some(0),
            memory_mb: None
        }
        .validate()
        .is_err());
        assert!(SpriteResizeRequest {
            vcpus: None,
            memory_mb: Some(0)
        }
        .validate()
        .is_err());
    }

    #[test]
    fn restore_validate_accepts_empty_request() {
        assert!(SpriteRestoreRequest::default().validate().is_ok());
    }

    #[test]
    fn restore_validate_rejects_oversized_ttl() {
        let req = SpriteRestoreRequest {
            ttl_seconds: Some(MAX_TTL_SECONDS + 1),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn restore_validate_rejects_zero_ttl() {
        let req = SpriteRestoreRequest {
            ttl_seconds: Some(0),
        };
        assert!(req.validate().is_err());
    }
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! FluxVM (`../fluxvm`, `fluxvm-api`) as a second VM backend next to libvirt,
//! reached over its REST API.

pub mod client;
pub mod migrate;
pub mod types;

pub use client::FluxvmClient;
pub use types::{map_status, FluxMetrics, FluxRecord, BACKEND_NAME, FLUXVM_HYPERVISORS};

/// True for `backend=fluxvm` (case-insensitive); empty / `libvirt` → false.
pub fn is_fluxvm(backend: Option<&str>) -> bool {
    backend
        .map(|b| b.trim().eq_ignore_ascii_case(BACKEND_NAME))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn is_fluxvm_matches_only_fluxvm() {
        assert!(super::is_fluxvm(Some("fluxvm")));
        assert!(super::is_fluxvm(Some(" FluxVM ")));
        assert!(!super::is_fluxvm(Some("libvirt")));
        assert!(!super::is_fluxvm(Some("")));
        assert!(!super::is_fluxvm(None));
    }
}

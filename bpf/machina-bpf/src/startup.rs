// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Startup safety checks shared by the bpfd binary and unit tests.

/// Enforce that a production bpfd contains the embedded kernel programs.
/// Development builds stay permissive unless the caller explicitly requires
/// programs, preserving the existing non-Linux / lightweight build workflow.
pub fn require_programs(required: bool, programs_compiled: bool) -> anyhow::Result<()> {
    if required && !programs_compiled {
        anyhow::bail!(
            "machina-bpfd was built without eBPF programs; production requires the datapath (run `make bpf-deps` and rebuild)"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_build_may_start_without_programs() {
        assert!(require_programs(false, false).is_ok());
    }

    #[test]
    fn production_build_starts_with_programs() {
        assert!(require_programs(true, true).is_ok());
    }

    #[test]
    fn production_build_refuses_empty_datapath() {
        let e = require_programs(true, false).unwrap_err().to_string();
        assert!(e.contains("production requires the datapath"));
        assert!(e.contains("make bpf-deps"));
    }
}

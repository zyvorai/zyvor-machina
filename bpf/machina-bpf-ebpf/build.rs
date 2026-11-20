// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Rebuild when bpf-linker changes; the linker is an undeclared dependency.
fn main() {
    if let Ok(p) = which::which("bpf-linker") {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}

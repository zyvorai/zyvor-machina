// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;
use std::process::Command;

/// Resolve the KVM/QEMU emulator binary (RHEL uses /usr/libexec/qemu-kvm).
pub fn find_qemu_binary() -> String {
    let candidates = [
        "/usr/bin/qemu-system-x86_64",
        "/usr/libexec/qemu-kvm",
        "/usr/bin/qemu-kvm",
    ];
    for path in &candidates {
        if Path::new(path).is_file() {
            return path.to_string();
        }
    }
    for name in &["qemu-system-x86_64", "qemu-kvm"] {
        if let Ok(out) = Command::new("which").arg(name).output() {
            if out.status.success() {
                let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !p.is_empty() {
                    return p;
                }
            }
        }
    }
    "/usr/bin/qemu-system-x86_64".to_string()
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! "Trusted launch" for new VMs: UEFI with Secure Boot enforced (Microsoft and distribution keys
//! pre-enrolled) plus an emulated TPM 2.0. Windows 11 needs both; hardened Linux images want them.
//!
//! Secure Boot uses libvirt's firmware auto-selection (`<os firmware='efi'>` with the
//! `secure-boot` and `enrolled-keys` features), so libvirt picks the signed OVMF build and a
//! matching variable store from the host's firmware descriptors instead of Machina hard-coding file
//! paths. The TPM is a `swtpm` emulator, the same device the hot-attach call adds.

use std::path::Path;

use crate::LibvirtError;

/// Where QEMU installs firmware descriptors (system) and where admins override them.
pub const FIRMWARE_DESCRIPTOR_DIRS: [&str; 2] = ["/usr/share/qemu/firmware", "/etc/qemu/firmware"];

/// Emulated TPM 2.0 device for a new domain.
pub const TPM_DEVICE_XML: &str =
    "<tpm model='tpm-crb'>\n      <backend type='emulator' version='2.0'/>\n    </tpm>";

/// `<os>` block for UEFI with Secure Boot enforced and keys enrolled.
pub const SECURE_BOOT_OS_XML: &str = "<os firmware='efi'>\n    <type arch='x86_64' machine='q35'>hvm</type>\n    <firmware>\n      <feature enabled='yes' name='enrolled-keys'/>\n      <feature enabled='yes' name='secure-boot'/>\n    </firmware>\n  </os>";

/// Secure Boot on q35 needs System Management Mode so the guest cannot write the variable store
/// outside firmware.
pub const SMM_FEATURE_XML: &str = "<smm state='on'/>";

/// virt-install arguments (after `--boot`) that ask for the same firmware selection.
pub const VIRT_INSTALL_SECURE_BOOT: &str = "firmware=efi,firmware.feature0.name=enrolled-keys,firmware.feature0.enabled=yes,firmware.feature1.name=secure-boot,firmware.feature1.enabled=yes";

/// virt-install `--tpm` value.
pub const VIRT_INSTALL_TPM: &str = "backend.type=emulator,backend.version=2.0,model=tpm-crb";

/// Reject combinations that cannot work, before anything is defined.
pub fn validate(secure_boot: bool, vtpm: bool, firmware: &str) -> Result<(), LibvirtError> {
    if secure_boot && firmware != "uefi" {
        return Err(LibvirtError::Invalid(
            "secure_boot needs firmware 'uefi' (BIOS has no Secure Boot)".into(),
        ));
    }
    let _ = vtpm; // a TPM works with BIOS or UEFI
    Ok(())
}

/// True when any firmware descriptor under `dirs` advertises both `secure-boot` and
/// `enrolled-keys` (the OVMF builds that ship signed with keys enrolled).
pub fn secure_boot_supported(dirs: &[&Path]) -> bool {
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                if descriptor_has_secure_boot(&text) {
                    return true;
                }
            }
        }
    }
    false
}

/// A descriptor is a UEFI one that lists both features.
pub fn descriptor_has_secure_boot(json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    let feats: Vec<&str> = v
        .get("features")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let uefi = v
        .get("interface-types")
        .and_then(|f| f.as_array())
        .is_some_and(|a| a.iter().any(|x| x.as_str() == Some("uefi")));
    uefi && feats.contains(&"secure-boot") && feats.contains(&"enrolled-keys")
}

/// Host checks run before a trusted-launch create, with remediation in the error text.
pub fn preflight(secure_boot: bool, vtpm: bool) -> Result<(), LibvirtError> {
    if secure_boot {
        let dirs: Vec<&Path> = FIRMWARE_DESCRIPTOR_DIRS.iter().map(Path::new).collect();
        if !secure_boot_supported(&dirs) {
            return Err(LibvirtError::Operation(
                "Secure Boot firmware not found. Install edk2-ovmf (Fedora/RHEL) or ovmf (Debian/Ubuntu) with the signed secboot build.".into(),
            ));
        }
    }
    if vtpm && !swtpm_installed() {
        return Err(LibvirtError::Operation(
            "swtpm is not installed. Install swtpm (and swtpm-tools) for emulated TPM 2.0.".into(),
        ));
    }
    Ok(())
}

fn swtpm_installed() -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join("swtpm").is_file()))
        .unwrap_or(false)
        || ["/usr/bin/swtpm", "/usr/local/bin/swtpm"]
            .iter()
            .any(|p| Path::new(p).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECBOOT_JSON: &str = r#"{"interface-types":["uefi"],"features":["acpi-s3","enrolled-keys","requires-smm","secure-boot","verbose-dynamic"]}"#;
    const PLAIN_JSON: &str = r#"{"interface-types":["uefi"],"features":["acpi-s3"]}"#;
    const BIOS_JSON: &str = r#"{"interface-types":["bios"],"features":[]}"#;

    #[test]
    fn secure_boot_needs_uefi() {
        assert!(validate(true, false, "bios").is_err());
        assert!(validate(true, true, "uefi").is_ok());
        assert!(validate(false, true, "bios").is_ok());
        assert!(validate(false, false, "bios").is_ok());
    }

    #[test]
    fn descriptor_must_list_both_features_for_uefi() {
        assert!(descriptor_has_secure_boot(SECBOOT_JSON));
        assert!(!descriptor_has_secure_boot(PLAIN_JSON));
        assert!(!descriptor_has_secure_boot(BIOS_JSON));
        assert!(!descriptor_has_secure_boot("not json"));
        // secure-boot without enrolled keys is not enough for a guest that must boot signed media
        assert!(!descriptor_has_secure_boot(
            r#"{"interface-types":["uefi"],"features":["secure-boot"]}"#
        ));
    }

    #[test]
    fn finds_a_descriptor_directory_entry() {
        let dir = std::env::temp_dir().join(format!("machina-fwdesc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!secure_boot_supported(&[dir.as_path()]));
        std::fs::write(dir.join("30-edk2-ovmf-x64-sb.json"), PLAIN_JSON).unwrap();
        std::fs::write(dir.join("notes.txt"), SECBOOT_JSON).unwrap(); // wrong extension: ignored
        assert!(!secure_boot_supported(&[dir.as_path()]));
        std::fs::write(dir.join("40-edk2-ovmf-x64-sb-enrolled.json"), SECBOOT_JSON).unwrap();
        assert!(secure_boot_supported(&[dir.as_path()]));
        let missing = std::env::temp_dir().join("machina-fwdesc-missing");
        assert!(!secure_boot_supported(&[missing.as_path()]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn xml_fragments_are_what_libvirt_expects() {
        assert!(SECURE_BOOT_OS_XML.contains("firmware='efi'"));
        assert!(SECURE_BOOT_OS_XML.contains("name='secure-boot'"));
        assert!(SECURE_BOOT_OS_XML.contains("name='enrolled-keys'"));
        assert!(!SECURE_BOOT_OS_XML.contains("<loader"));
        assert!(TPM_DEVICE_XML.contains("version='2.0'"));
        assert_eq!(SMM_FEATURE_XML, "<smm state='on'/>");
        assert!(VIRT_INSTALL_SECURE_BOOT.starts_with("firmware=efi"));
    }
}

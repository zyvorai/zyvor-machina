// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Create and remove local UNIX accounts (`useradd` / `userdel`, or `homectl` when
//! `systemd-homed` is active), plus `chpasswd`. Intended for daemons running as root;
//! callers must enforce policy (e.g. only session users in wheel/sudo).

use std::io::Write;
use std::process::{Command, Stdio};

use crate::config::RunAsUserConfig;
use crate::run_as_user;
use crate::LibvirtError;

fn command(
    exec_as: Option<(&RunAsUserConfig, &str)>,
    program: &str,
    args: &[&str],
) -> Result<Command, LibvirtError> {
    match exec_as {
        Some((cfg, user)) => run_as_user::command_as_user(cfg, user, program, args),
        None => {
            let mut c = Command::new(program);
            c.args(args);
            Ok(c)
        }
    }
}

fn status(
    exec_as: Option<(&RunAsUserConfig, &str)>,
    program: &str,
    args: &[&str],
) -> Result<std::process::ExitStatus, LibvirtError> {
    match exec_as {
        Some((cfg, user)) => run_as_user::status_as_user(cfg, Some(user), program, args),
        None => run_as_user::status_as_user(&RunAsUserConfig::default(), None, program, args),
    }
}

fn output(
    exec_as: Option<(&RunAsUserConfig, &str)>,
    program: &str,
    args: &[&str],
) -> Result<std::process::Output, LibvirtError> {
    match exec_as {
        Some((cfg, user)) => run_as_user::output_as_user(cfg, Some(user), program, args),
        None => run_as_user::output_as_user(&RunAsUserConfig::default(), None, program, args),
    }
}

/// Groups that conventionally grant `sudo` on common distros (membership checked via NSS).
const PRIVILEGED_GROUPS: &[&str] = &["wheel", "sudo", "admin"];

/// `systemd-homed` is the active systemd unit (may still coexist with `/etc/passwd` users).
pub fn systemd_homed_is_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "systemd-homed"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false)
}

/// `homectl` is installed and runnable.
pub fn homectl_available() -> bool {
    Command::new("homectl")
        .arg("--version")
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Prefer `wheel` (RHEL/Fedora), then `sudo`, then `admin` (legacy Debian), when present in NSS.
pub fn sudo_supplementary_group() -> Option<&'static str> {
    if getent_line("group", "wheel").is_some() {
        return Some("wheel");
    }
    if getent_line("group", "sudo").is_some() {
        return Some("sudo");
    }
    if getent_line("group", "admin").is_some() {
        return Some("admin");
    }
    None
}

/// `"systemd-homed"` when homed is active and `homectl` exists; otherwise `"traditional"`.
pub fn os_user_account_backend() -> &'static str {
    if systemd_homed_is_active() && homectl_available() {
        "systemd-homed"
    } else {
        "traditional"
    }
}

fn is_homed_managed_user(username: &str) -> bool {
    Command::new("homectl")
        .args(["inspect", username])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Standard UNIX group for `qemu:///system` socket/policy on Fedora/RHEL/Debian derivatives.
pub const LIBVIRT_UNIX_GROUP: &str = "libvirt";

/// Whether `getent group libvirt` succeeds (group exists on host).
pub fn libvirt_unix_group_exists() -> bool {
    Command::new("getent")
        .args(["group", LIBVIRT_UNIX_GROUP])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Same character rules as web login (`daemon/src/auth.rs` login_handler).
fn validate_login_username(name: &str) -> Result<(), LibvirtError> {
    if name.is_empty() {
        return Err(LibvirtError::Invalid("Username cannot be empty".into()));
    }
    if name.len() > 32 {
        return Err(LibvirtError::Invalid(
            "Username too long for UNIX account (max 32)".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(LibvirtError::Invalid(
            "Invalid username characters (allowed: letters, digits, _, -, .)".into(),
        ));
    }
    // Some call sites pass the username as a bare positional argv element to
    // `homectl`/`id` without a preceding `--` separator; a leading dash would let
    // it be parsed as an option (e.g. `homectl create --uid=0 <name>`-style flag
    // injection) instead of the account name.
    if name.starts_with('-') {
        return Err(LibvirtError::Invalid(
            "Username must not start with '-'".into(),
        ));
    }
    Ok(())
}

/// One line from `getent <db> <key>` (trimmed), if exit success.
fn getent_line(db: &str, key: &str) -> Option<String> {
    let out = Command::new("getent").args([db, key]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Whether a local/NSS UNIX account exists for `username`.
pub fn unix_user_exists(username: &str) -> bool {
    if validate_login_username(username).is_err() {
        return false;
    }
    getent_line("passwd", username).is_some()
}

/// Primary group name for `username` via `getent passwd` + `getent group <gid>`.
fn passwd_primary_group_name(username: &str) -> Option<String> {
    let line = getent_line("passwd", username)?;
    let gid = line.split(':').nth(3)?;
    let gline = getent_line("group", gid)?;
    gline.split(':').next().map(|s| s.to_string())
}

/// Parse `groups(1)` output: `user : g1 g2` or `user: g1 g2`.
fn parse_groups_output(line: &str) -> Vec<String> {
    let tail = if let Some(i) = line.find(':') {
        &line[i + 1..]
    } else {
        line
    };
    tail.split_whitespace().map(|s| s.to_string()).collect()
}

/// All group names for `username` (primary + supplementary), best-effort via `groups(1)` and passwd.
fn unix_all_group_names(username: &str) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(pg) = passwd_primary_group_name(username) {
        names.push(pg);
    }
    if let Ok(out) = Command::new("groups").arg(username).output() {
        if out.status.success() {
            let line = String::from_utf8_lossy(&out.stdout);
            names.extend(parse_groups_output(&line));
        }
    }
    names.sort();
    names.dedup();
    names
}

/// True if `username` is listed in `getent group <group>` member field.
fn user_in_group_getent(username: &str, group: &str) -> bool {
    let Some(line) = getent_line("group", group) else {
        return false;
    };
    let members = line.split(':').nth(3).unwrap_or("");
    members.split(',').any(|m| m.trim() == username)
}

/// True if `username` may administer host accounts: root, or in wheel / sudo / admin (any NSS path).
///
/// Uses `groups(1)` + primary GID from `getent passwd` (GNU `id -Gn` is **supplementary-only** and
/// missed users whose primary group is `sudo`/`wheel`). Also checks `getent group` membership lines.
pub fn unix_user_may_use_sudo(username: &str) -> bool {
    if username == "root" {
        return true;
    }
    if validate_login_username(username).is_err() {
        return false;
    }
    let groups = unix_all_group_names(username);
    if groups
        .iter()
        .any(|g| PRIVILEGED_GROUPS.contains(&g.as_str()))
    {
        return true;
    }
    PRIVILEGED_GROUPS
        .iter()
        .any(|g| user_in_group_getent(username, g))
}

/// Result of [`create_local_user`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalUserCreateOutcome {
    /// User was added to the host `libvirt` supplementary group (`usermod -aG libvirt`).
    pub libvirt_group_attached: bool,
}

/// Create a new local user with home directory and `/bin/bash`, set password via `chpasswd`,
/// and optionally append the user to the `libvirt` group for `qemu:///system` access.
///
/// When `systemd-homed` is active and `homectl` is available, uses `homectl create` with
/// `--member-of` for the host `wheel`/`sudo` group plus optional `libvirt`; otherwise uses
/// `useradd` / `usermod`.
pub fn create_local_user(
    new_username: &str,
    password: &str,
    add_to_libvirt_group: bool,
    exec_as: Option<(&RunAsUserConfig, &str)>,
) -> Result<LocalUserCreateOutcome, LibvirtError> {
    validate_login_username(new_username)?;
    if new_username.eq_ignore_ascii_case("root") {
        return Err(LibvirtError::Invalid("Cannot create root".into()));
    }
    if password.is_empty() {
        return Err(LibvirtError::Invalid("Password is required".into()));
    }
    if password.len() > 4096 {
        return Err(LibvirtError::Invalid("Password too long".into()));
    }
    if password.contains(':') || password.contains('\n') || password.contains('\0') {
        return Err(LibvirtError::Invalid(
            "Password cannot contain ':', newline, or NUL".into(),
        ));
    }

    if add_to_libvirt_group && !libvirt_unix_group_exists() {
        return Err(LibvirtError::Invalid(format!(
            "UNIX group '{}' is not defined on this host (install libvirt / libvirt-daemon)",
            LIBVIRT_UNIX_GROUP
        )));
    }

    let exists = status(exec_as, "id", &[new_username])
        .map_err(|e| LibvirtError::Operation(format!("id: {e}")))?;
    if exists.success() {
        return Err(LibvirtError::Invalid(format!(
            "User '{}' already exists",
            new_username
        )));
    }

    let use_homed = os_user_account_backend() == "systemd-homed";
    let mut libvirt_attached = false;

    if use_homed {
        let Some(sudo_g) = sudo_supplementary_group() else {
            return Err(LibvirtError::Invalid(
                "systemd-homed is active but no wheel, sudo, or admin group exists in NSS; cannot assign sudo membership"
                    .into(),
            ));
        };
        let mut args: Vec<String> = vec![
            "create".into(),
            "--shell=/bin/bash".into(),
            format!("--member-of={sudo_g}"),
            "--storage=directory".into(),
        ];
        if add_to_libvirt_group {
            args.push(format!("--member-of={LIBVIRT_UNIX_GROUP}"));
            libvirt_attached = true;
        }
        args.push(new_username.to_string());
        let st = status(
            exec_as,
            "homectl",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .map_err(|e| LibvirtError::Operation(format!("homectl: {e}")))?;
        if !st.success() {
            return Err(LibvirtError::Operation(
                "homectl create failed (see journal for details)".into(),
            ));
        }
    } else {
        let st = status(
            exec_as,
            "useradd",
            &["-m", "-s", "/bin/bash", "--", new_username],
        )
        .map_err(|e| LibvirtError::Operation(format!("useradd: {e}")))?;
        if !st.success() {
            return Err(LibvirtError::Operation(
                "useradd failed (see journal for details)".into(),
            ));
        }
        if let Some(g) = sudo_supplementary_group() {
            let um = output(exec_as, "usermod", &["-aG", g, "--", new_username])
                .map_err(|e| LibvirtError::Operation(format!("usermod: {e}")))?;
            if !um.status.success() {
                let err = String::from_utf8_lossy(&um.stderr);
                return Err(LibvirtError::Operation(format!(
                    "usermod -aG {g} failed: {}",
                    err.trim()
                )));
            }
        }
        if add_to_libvirt_group {
            let um = output(
                exec_as,
                "usermod",
                &["-aG", LIBVIRT_UNIX_GROUP, "--", new_username],
            )
            .map_err(|e| LibvirtError::Operation(format!("usermod: {e}")))?;
            if !um.status.success() {
                let err = String::from_utf8_lossy(&um.stderr);
                return Err(LibvirtError::Operation(format!(
                    "usermod -aG {} failed: {}",
                    LIBVIRT_UNIX_GROUP,
                    err.trim()
                )));
            }
            libvirt_attached = true;
        }
    }

    let mut child = command(exec_as, "chpasswd", &[])?
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| LibvirtError::Operation(format!("chpasswd: {e}")))?;
    let line = format!("{new_username}:{password}\n");
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| LibvirtError::Operation("chpasswd: no stdin".into()))?;
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| LibvirtError::Operation(format!("chpasswd write: {e}")))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| LibvirtError::Operation(format!("chpasswd: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(LibvirtError::Operation(format!(
            "chpasswd failed: {}",
            err.trim()
        )));
    }

    Ok(LocalUserCreateOutcome {
        libvirt_group_attached: libvirt_attached,
    })
}

/// Remove a local user and home directory. Uses `homectl remove` when the account is managed by
/// `systemd-homed` (`homectl inspect` succeeds); otherwise `userdel -r`.
pub fn delete_local_user(
    username: &str,
    exec_as: Option<(&RunAsUserConfig, &str)>,
) -> Result<(), LibvirtError> {
    validate_login_username(username)?;
    if username.eq_ignore_ascii_case("root") {
        return Err(LibvirtError::Invalid("Cannot delete root".into()));
    }

    let exists = status(exec_as, "id", &[username])
        .map_err(|e| LibvirtError::Operation(format!("id: {e}")))?;
    if !exists.success() {
        return Err(LibvirtError::NotFound(format!(
            "User '{username}' does not exist"
        )));
    }

    if is_homed_managed_user(username) {
        let st = status(exec_as, "homectl", &["remove", username])
            .map_err(|e| LibvirtError::Operation(format!("homectl: {e}")))?;
        if !st.success() {
            return Err(LibvirtError::Operation(
                "homectl remove failed (see journal for details)".into(),
            ));
        }
    } else {
        let st = status(exec_as, "userdel", &["-r", "--", username])
            .map_err(|e| LibvirtError::Operation(format!("userdel: {e}")))?;
        if !st.success() {
            return Err(LibvirtError::Operation(
                "userdel failed (see journal for details)".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_login_username_rejects_empty() {
        assert!(validate_login_username("").is_err());
    }

    #[test]
    fn validate_login_username_accepts_alma() {
        assert!(validate_login_username("alma-user_1").is_ok());
    }

    #[test]
    fn parse_groups_output_debianish() {
        let g = parse_groups_output("sus : sus sudo\n");
        assert!(g.contains(&"sudo".to_string()));
        assert!(g.contains(&"sus".to_string()));
    }

    #[test]
    fn parse_groups_output_rhel_no_space_after_colon() {
        let g = parse_groups_output("sus: sus wheel\n");
        assert!(g.contains(&"wheel".to_string()));
    }
}

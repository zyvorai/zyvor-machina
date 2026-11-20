// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Install setuid-root: `chown root:root && chmod u+s /usr/local/libexec/machina-run-as-user`
//!
//! Usage: machina-run-as-user <unix-user> <program> [args...]
//!
//! Security model: this binary runs setuid-root and is invocable by any local
//! user with fully attacker-controlled argv, so it must self-protect regardless
//! of the intended caller (the daemon):
//!   * The program is selected by *basename* from a fixed allow-list and then
//!     resolved to an absolute path inside a fixed set of trusted system
//!     directories. The caller-supplied path string is NEVER exec'd, so
//!     `machina-run-as-user root /tmp/evil/useradd` cannot run `/tmp/evil/...`.
//!   * The target user must be a real, non-root account (uid != 0, gid != 0).
//!     This blocks `machina-run-as-user root usermod -aG sudo attacker`.
//!   * Supplementary groups are reset to exactly the target user's groups via
//!     `initgroups` before dropping privileges, so the invoker's groups (e.g.
//!     libvirt/kvm, or root's) are not retained.

use std::env;
use std::ffi::CString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

const ALLOWED: &[&str] = &[
    "useradd", "userdel", "usermod", "homectl", "chpasswd", "id", "getent",
];

/// Trusted directories the allow-listed programs are resolved against, in order.
/// The caller cannot influence this set.
const TRUSTED_DIRS: &[&str] = &["/usr/sbin", "/usr/bin", "/sbin", "/bin"];

fn main() {
    if let Err(e) = run() {
        eprintln!("machina-run-as-user: {e}");
        process::exit(1);
    }
}

/// Resolve an allow-listed basename to an absolute path inside a trusted dir.
/// Rejects anything with a path separator and anything not found in a trusted dir.
fn resolve_program(program: &str) -> Result<PathBuf, String> {
    // Reject path separators outright — the argument must be a bare basename.
    if program.contains('/') {
        return Err(format!(
            "program '{program}' must be a bare command name, not a path"
        ));
    }
    let base = Path::new(program)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("invalid program name '{program}'"))?;
    if !ALLOWED.contains(&base) {
        return Err(format!("program '{base}' is not allow-listed"));
    }
    for dir in TRUSTED_DIRS {
        let candidate = Path::new(dir).join(base);
        // Must exist and be a regular file in a trusted dir. We deliberately do
        // not follow the caller's PATH.
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "allow-listed program '{base}' not found in any trusted directory"
    ))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        return Err("usage: machina-run-as-user <unix-user> <program> [args...]".into());
    }
    let target_user = &args[1];
    let program = &args[2];
    let prog_args: Vec<&str> = args[3..].iter().map(|s| s.as_str()).collect();

    let exec_path = resolve_program(program)?;

    let c_user = CString::new(target_user.as_str()).map_err(|e| e.to_string())?;
    let (uid, gid) = unsafe {
        let pwd = libc::getpwnam(c_user.as_ptr());
        if pwd.is_null() {
            return Err(format!("unknown user '{target_user}'"));
        }
        ((*pwd).pw_uid, (*pwd).pw_gid)
    };

    // Never run as root or a privileged (gid 0) target. The whole point of this
    // helper is to drop to an unprivileged OIDC-mapped user.
    if uid == 0 || gid == 0 {
        return Err(format!(
            "refusing to run as privileged user '{target_user}' (uid={uid}, gid={gid})"
        ));
    }

    // Reset supplementary groups to exactly the target user's groups, then drop
    // gid before uid. Order matters: setgroups/setgid must precede setuid while
    // we still hold root, otherwise the drop cannot complete.
    // `initgroups`' basegroup arg is c_int on some platforms (macOS) and gid_t on
    // others (Linux); `as _` coerces to whatever the target signature expects.
    if unsafe { libc::initgroups(c_user.as_ptr(), gid as _) } != 0 {
        return Err("initgroups failed (helper must be installed setuid root)".into());
    }
    if unsafe { libc::setgid(gid) } != 0 {
        return Err("setgid failed (helper must be installed setuid root)".into());
    }
    if unsafe { libc::setuid(uid) } != 0 {
        return Err("setuid failed (helper must be installed setuid root)".into());
    }
    // Defense in depth: confirm privileges are actually gone and cannot be
    // regained before exec'ing the target program.
    if unsafe { libc::setuid(0) } == 0 {
        return Err("privilege drop failed: still able to regain root".into());
    }

    let err = Command::new(&exec_path).args(prog_args).exec();
    Err(format!("exec {} failed: {err}", exec_path.display()))
}

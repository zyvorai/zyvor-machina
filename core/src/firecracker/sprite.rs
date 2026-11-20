// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Boot/tear down a disposable "sprite" microVM on Firecracker.
//!
//! Structurally mirrors `crate::cloud_hypervisor::sprite` (itself
//! mirroring `crate::libvirt::sprite`): same golden-image registry
//! (`crate::libvirt::sprite::resolve_golden_image`), same "throwaway,
//! headless, destroy-only" semantics, same directly-supervised-child-process
//! ownership model. Two real differences from Cloud Hypervisor, both by
//! design rather than trial-and-error this time (decided during planning):
//!
//! - **Raw disk, converted at boot time — and unpartitioned.** Firecracker's
//!   drive backend is raw-only (no qcow2, not even Cloud Hypervisor's
//!   rejected-but-attempted backing-file overlay), *and* it auto-appends
//!   `root=/dev/vda rw` to the kernel command line for whichever drive has
//!   `is_root_device: true` — always the bare device, never a partition
//!   number, and appended *after* whatever `boot_args` the caller supplied
//!   (confirmed live: a caller-supplied `root=/dev/vda1` gets silently
//!   overridden by Firecracker's own `root=/dev/vda`, since the kernel
//!   takes the last `root=` on the line — this is documented upstream,
//!   firecracker-microvm/firecracker#2709). Every golden image is a
//!   GPT-partitioned qcow2 (see `resolve_golden_image`), so `materialize_raw_disk`
//!   below doesn't just convert format — it extracts partition 1's content
//!   into an unpartitioned raw file, matching what Firecracker actually
//!   expects at `/dev/vda`. Runs on every boot rather than requiring a
//!   pre-extracted sibling file — simpler for operators, at the real cost
//!   of a multi-hundred-MB conversion per sprite instead of Cloud
//!   Hypervisor's near-instant reflink copy. Worth revisiting (e.g. a
//!   pre-extracted sibling fast path) if this boot latency turns out to
//!   matter in practice.
//! - **API-driven configuration, not CLI flags.** `firecracker` starts
//!   serving only its control API over the socket — every setting
//!   (vcpu/memory, boot source, drive, vsock, network interface) is a `PUT`
//!   call, and the machine only actually boots once `PUT /actions
//!   {"action_type":"InstanceStart"}` is sent. Each call below shells
//!   `curl --unix-socket` (see this module's parent doc comment for why).
//!
//! Firecracker boots a host-supplied kernel directly — no BIOS/UEFI, no
//! bootloader, and critically no reading of the guest's own installed
//! kernel from its `/boot` at all — a golden image's own kernel/initramfs
//! go completely unused when booted this way.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncBufReadExt;
use tokio::process::Command as TokioCommand;
use tokio::time::Instant;

use crate::sprite_net::{create_egress_tap, delete_egress_tap, tap_name_for, SPRITE_RUN_DIR};
use crate::LibvirtError;

use super::{find_firecracker_binary, find_firecracker_kernel};

/// Sprites are meant to be near-instant; the API socket typically appears
/// within tens of milliseconds of the process starting. This bounds how
/// long a caller waits before treating a stuck/slow boot as a failure.
const BOOT_READY_TIMEOUT: Duration = Duration::from_secs(5);
const BOOT_READY_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long to give `SIGTERM` to take effect before falling back to
/// `SIGKILL`.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// Kernel command line for every Firecracker sprite. Deliberately no
/// `root=`/`rw` here — Firecracker appends its own `root=/dev/vda rw` for
/// the `is_root_device: true` drive, and it wins regardless of what's
/// supplied here (see this module's doc comment), so adding a
/// conflicting one would be actively misleading to read.
const BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off";

pub struct FcBootRequest<'a> {
    /// Namespaces this sprite's run directory — not embedded in any
    /// Firecracker API call.
    pub sprite_id: &'a str,
    /// Still qcow2 — `boot_sprite_fc` converts it to raw itself (see this
    /// module's doc comment for why boot-time conversion, not a
    /// pre-converted sibling file).
    pub golden_image_path: &'a Path,
    pub vcpus: u32,
    pub memory_mb: u64,
    /// Must not collide with any other sprite's CID (libvirt-, Cloud
    /// Hypervisor-, or Firecracker-backed) currently running on this host —
    /// see `daemon::sprite_registry`'s CID allocator, which is already
    /// shared across all three backends.
    pub vsock_cid: u32,
    /// Attach a virtio-net TAP to the host's "default" NAT network
    /// (`virbr0`) instead of staying vsock-only. See
    /// `spec::SpriteCreateRequest::network_egress`'s doc comment for the
    /// isolation posture this implies.
    pub network_egress: bool,
}

pub struct FcBootResult {
    /// The converted `.raw` copy — same teardown ownership rule as the
    /// other backends: only ever delete this, never `golden_image_path`.
    pub disk_path: PathBuf,
    pub pid: u32,
    pub api_socket: PathBuf,
    pub vsock_socket: PathBuf,
    /// `Some` when `network_egress` was requested — `teardown_sprite_fc`
    /// needs this to remove the TAP device.
    pub tap_name: Option<String>,
}

/// Convert `src` (qcow2) to a raw disk at `dest`. Real, meaningful I/O —
/// see this module's doc comment for the tradeoff this was chosen over.
fn materialize_raw_disk(src: &Path, dest: &Path) -> Result<(), LibvirtError> {
    let tmp_full = dest.with_extension("full.raw.tmp");
    let out = std::process::Command::new("qemu-img")
        .args(["convert", "-O", "raw"])
        .arg(src)
        .arg(&tmp_full)
        .output()
        .map_err(|e| LibvirtError::Operation(format!("qemu-img convert: {e}")))?;
    if !out.status.success() {
        let _ = fs::remove_file(&tmp_full);
        return Err(LibvirtError::Operation(format!(
            "qemu-img convert -O raw failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }

    // Firecracker expects an unpartitioned root filesystem at /dev/vda (see
    // this module's doc comment) — extract partition 1's content rather
    // than handing it a full partitioned disk. Falls back to using the
    // whole converted image as-is when it has no partition table (already
    // a bare filesystem).
    match root_partition_extent(&tmp_full) {
        Some((start_sector, size_sectors)) => {
            let extract = std::process::Command::new("dd")
                .arg(format!("if={}", tmp_full.display()))
                .arg(format!("of={}", dest.display()))
                .arg("bs=512")
                .arg(format!("skip={start_sector}"))
                .arg(format!("count={size_sectors}"))
                .arg("status=none")
                .output()
                .map_err(|e| LibvirtError::Operation(format!("dd: {e}")))?;
            let _ = fs::remove_file(&tmp_full);
            if !extract.status.success() {
                let _ = fs::remove_file(dest);
                return Err(LibvirtError::Operation(format!(
                    "extracting root partition failed: {}",
                    String::from_utf8_lossy(&extract.stderr)
                )));
            }
            Ok(())
        }
        None => fs::rename(&tmp_full, dest)
            .map_err(|e| LibvirtError::Operation(format!("failed to finalize raw disk: {e}"))),
    }
}

/// Parse `sfdisk -d <disk>` output for partition 1's (start, size) in
/// 512-byte sectors — the units `dd` above extracts in. Returns `None` if
/// the disk has no partition table (`sfdisk` exits non-zero, e.g. a bare
/// filesystem image) or partition 1 isn't found there, in which case the
/// caller treats the whole converted disk as already being the root
/// filesystem directly.
fn root_partition_extent(disk: &Path) -> Option<(u64, u64)> {
    let out = std::process::Command::new("sfdisk")
        .arg("-d")
        .arg(disk)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // `sfdisk -d` lists one partition per line, e.g.:
    //   /path/to/disk1 : start=     2099200, size=     5240799, type=...
    // Match the exact `<disk>1 :` prefix so partition 1 isn't confused with
    // 14/15/16 (common on virt-builder's GPT layout: BIOS-boot/ESP/root).
    let prefix = format!("{}1 :", disk.display());
    let line = text.lines().find(|l| l.trim_start().starts_with(&prefix))?;
    let start = line
        .split("start=")
        .nth(1)?
        .split(',')
        .next()?
        .trim()
        .parse()
        .ok()?;
    let size = line
        .split("size=")
        .nth(1)?
        .split(',')
        .next()?
        .trim()
        .parse()
        .ok()?;
    Some((start, size))
}

/// One `PUT <path>` call against the Firecracker API socket, via
/// `curl --unix-socket` (see this module's parent doc comment). `-w
/// "\n%{http_code}"` appends the status code as a trailing line so both the
/// response body (useful in error messages) and the status are recoverable
/// from a single `curl` invocation.
async fn api_put(api_socket: &Path, path: &str, body: &str) -> Result<(), LibvirtError> {
    let url = format!("http://localhost{path}");
    let out = TokioCommand::new("curl")
        .arg("--unix-socket")
        .arg(api_socket)
        .arg("-sS")
        .arg("-X")
        .arg("PUT")
        .arg(&url)
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(body)
        .arg("-w")
        .arg("\n%{http_code}")
        .output()
        .await
        .map_err(|e| LibvirtError::Operation(format!("curl {path}: {e}")))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let (resp_body, status) = stdout
        .trim_end()
        .rsplit_once('\n')
        .unwrap_or((stdout.as_ref(), ""));
    let status_ok = status
        .trim()
        .parse::<u16>()
        .map(|c| (200..300).contains(&c))
        .unwrap_or(false);
    if !out.status.success() || !status_ok {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(LibvirtError::Operation(format!(
            "firecracker API {path} failed (http {}): {} {}",
            if status.is_empty() {
                "?"
            } else {
                status.trim()
            },
            resp_body.trim(),
            stderr.trim()
        )));
    }
    Ok(())
}

/// Convert `req.golden_image_path` to raw, spawn `firecracker` "paused"
/// (serving only its API), then drive it through the configuration
/// sequence and `InstanceStart` once its API socket appears.
pub async fn boot_sprite_fc(req: &FcBootRequest<'_>) -> Result<FcBootResult, LibvirtError> {
    let fc_binary = find_firecracker_binary()?;
    let kernel = find_firecracker_kernel()?;

    let run_dir = PathBuf::from(SPRITE_RUN_DIR).join(req.sprite_id);
    fs::create_dir_all(&run_dir)
        .map_err(|e| LibvirtError::Operation(format!("failed to create sprite run dir: {e}")))?;
    let disk_path = run_dir.join("disk.raw");
    let api_socket = run_dir.join("api.sock");
    let vsock_socket = run_dir.join("vsock.sock");

    let materialize_result = {
        let src = req.golden_image_path.to_path_buf();
        let dst = disk_path.clone();
        // Blocking (qemu-img convert, potentially a multi-hundred-MB
        // conversion) — off the async executor so it doesn't stall other
        // requests sharing this worker thread.
        tokio::task::spawn_blocking(move || materialize_raw_disk(&src, &dst))
            .await
            .unwrap_or_else(|e| {
                Err(LibvirtError::Internal(format!(
                    "materialize task join error: {e}"
                )))
            })
    };
    if let Err(e) = materialize_result {
        let _ = fs::remove_dir_all(&run_dir);
        return Err(e);
    }

    let tap_name = if req.network_egress {
        let tap = tap_name_for("fc-", req.sprite_id);
        if let Err(e) = create_egress_tap(&tap) {
            let _ = fs::remove_dir_all(&run_dir);
            return Err(e);
        }
        Some(tap)
    } else {
        None
    };

    let mut command = TokioCommand::new(fc_binary);
    command
        .arg("--api-sock")
        .arg(&api_socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            if let Some(tap) = &tap_name {
                delete_egress_tap(tap);
            }
            let _ = fs::remove_dir_all(&run_dir);
            return Err(LibvirtError::Operation(format!(
                "failed to spawn firecracker: {e}"
            )));
        }
    };
    let pid = match child.id() {
        Some(pid) => pid,
        None => {
            if let Some(tap) = &tap_name {
                delete_egress_tap(tap);
            }
            return Err(LibvirtError::Internal(
                "firecracker spawned without a pid".into(),
            ));
        }
    };

    // Same tee-into-tracing-and-last-line pattern
    // `cloud_hypervisor::sprite::boot_sprite_chv` uses: the pipe must be
    // drained continuously regardless (an unread pipe fills its OS buffer
    // and blocks the child's writes), and this doubles as the diagnostic
    // trail for a boot failure.
    let last_stderr_line = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    if let Some(stderr) = child.stderr.take() {
        let last_stderr_line = last_stderr_line.clone();
        let sprite_id = req.sprite_id.to_string();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::warn!("firecracker[{sprite_id}]: {line}");
                if let Ok(mut last) = last_stderr_line.lock() {
                    *last = line;
                }
            }
        });
    }

    // Poll for the API socket rather than a fixed sleep, same as Cloud
    // Hypervisor — but here the socket coming up only means the process is
    // ready to be *configured*, not booted; `InstanceStart` below is what
    // actually boots it.
    let deadline = Instant::now() + BOOT_READY_TIMEOUT;
    loop {
        if api_socket.exists() {
            break;
        }
        if let Ok(Some(status)) = child.try_wait() {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let last_line = last_stderr_line
                .lock()
                .map(|l| l.clone())
                .unwrap_or_default();
            if let Some(tap) = &tap_name {
                delete_egress_tap(tap);
            }
            let _ = fs::remove_dir_all(&run_dir);
            return Err(LibvirtError::Operation(format!(
                "firecracker exited before its API socket appeared (status: {status}): {last_line}"
            )));
        }
        if Instant::now() >= deadline {
            let _ = child.start_kill();
            if let Some(tap) = &tap_name {
                delete_egress_tap(tap);
            }
            let _ = fs::remove_dir_all(&run_dir);
            return Err(LibvirtError::Operation(
                "firecracker did not become ready within timeout".into(),
            ));
        }
        tokio::time::sleep(BOOT_READY_POLL_INTERVAL).await;
    }

    if let Err(e) =
        configure_and_start(req, &api_socket, &disk_path, &kernel, tap_name.as_deref()).await
    {
        let _ = child.start_kill();
        if let Some(tap) = &tap_name {
            delete_egress_tap(tap);
        }
        let _ = fs::remove_dir_all(&run_dir);
        return Err(e);
    }

    // Deliberately drop `child` rather than holding it — same reasoning as
    // `boot_sprite_chv`: tokio's orphan reaper keeps reaping it, and from
    // here the daemon controls this sprite's lifetime purely through `pid`
    // (see `teardown_sprite_fc`).
    drop(child);

    Ok(FcBootResult {
        disk_path,
        pid,
        api_socket,
        vsock_socket,
        tap_name,
    })
}

async fn configure_and_start(
    req: &FcBootRequest<'_>,
    api_socket: &Path,
    disk_path: &Path,
    kernel: &str,
    tap_name: Option<&str>,
) -> Result<(), LibvirtError> {
    api_put(
        api_socket,
        "/machine-config",
        &format!(
            r#"{{"vcpu_count":{},"mem_size_mib":{}}}"#,
            req.vcpus, req.memory_mb
        ),
    )
    .await?;

    api_put(
        api_socket,
        "/boot-source",
        &format!(
            r#"{{"kernel_image_path":{},"boot_args":{}}}"#,
            serde_json::to_string(kernel).unwrap_or_default(),
            serde_json::to_string(BOOT_ARGS).unwrap_or_default(),
        ),
    )
    .await?;

    api_put(
        api_socket,
        "/drives/rootfs",
        &format!(
            r#"{{"drive_id":"rootfs","path_on_host":{},"is_root_device":true,"is_read_only":false}}"#,
            serde_json::to_string(&disk_path.display().to_string()).unwrap_or_default(),
        ),
    )
    .await?;

    api_put(
        api_socket,
        "/vsock",
        &format!(
            r#"{{"vsock_id":"vsock0","guest_cid":{},"uds_path":{}}}"#,
            req.vsock_cid,
            serde_json::to_string(&req.vsock_socket_path_for_body()).unwrap_or_default(),
        ),
    )
    .await?;

    if let Some(tap) = tap_name {
        api_put(
            api_socket,
            "/network-interfaces/eth0",
            &format!(
                r#"{{"iface_id":"eth0","host_dev_name":{}}}"#,
                serde_json::to_string(tap).unwrap_or_default()
            ),
        )
        .await?;
    }

    api_put(api_socket, "/actions", r#"{"action_type":"InstanceStart"}"#).await
}

/// Tear down one Firecracker sprite: `SIGTERM` (Firecracker's own
/// host-driven graceful stop — no API action needed for this, unlike Cloud
/// Hypervisor's `ch-remote shutdown-vmm`, so this is actually simpler),
/// falling back to `SIGKILL` if the process is still around shortly after.
/// Treats "already gone" as success throughout, so a reaper retry after a
/// partial failure stays safe — same contract as
/// `cloud_hypervisor::sprite::teardown_sprite_chv`.
pub async fn teardown_sprite_fc(
    pid: u32,
    api_socket: &Path,
    disk_path: &Path,
    vsock_socket: &Path,
    tap_name: Option<&str>,
) -> Result<(), String> {
    let _ = crate::libvirt::extras::kill_host_process(pid, "TERM");

    tokio::time::sleep(SHUTDOWN_GRACE).await;

    // Ignore the result: if SIGTERM above already worked (or the process
    // crashed on its own), this returns an "already gone" style OS error
    // (ESRCH), which is the outcome we wanted anyway.
    let _ = crate::libvirt::extras::kill_host_process(pid, "KILL");

    if let Some(tap) = tap_name {
        delete_egress_tap(tap);
    }

    let _ = fs::remove_file(disk_path);
    let _ = fs::remove_file(api_socket);
    let _ = fs::remove_file(vsock_socket);
    if let Some(run_dir) = api_socket.parent() {
        let _ = fs::remove_dir_all(run_dir);
    }

    Ok(())
}

/// One `PATCH <path>` call against the Firecracker API socket — identical
/// shape to `api_put` above, but Firecracker's pause/resume endpoint
/// (`/vm`) is specifically a `PATCH`, not a `PUT`.
async fn api_patch(api_socket: &Path, path: &str, body: &str) -> Result<(), LibvirtError> {
    let url = format!("http://localhost{path}");
    let out = TokioCommand::new("curl")
        .arg("--unix-socket")
        .arg(api_socket)
        .arg("-sS")
        .arg("-X")
        .arg("PATCH")
        .arg(&url)
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(body)
        .arg("-w")
        .arg("\n%{http_code}")
        .output()
        .await
        .map_err(|e| LibvirtError::Operation(format!("curl {path}: {e}")))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let (resp_body, status) = stdout
        .trim_end()
        .rsplit_once('\n')
        .unwrap_or((stdout.as_ref(), ""));
    let status_ok = status
        .trim()
        .parse::<u16>()
        .map(|c| (200..300).contains(&c))
        .unwrap_or(false);
    if !out.status.success() || !status_ok {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(LibvirtError::Operation(format!(
            "firecracker API {path} failed (http {}): {} {}",
            if status.is_empty() {
                "?"
            } else {
                status.trim()
            },
            resp_body.trim(),
            stderr.trim()
        )));
    }
    Ok(())
}

/// Pause a running Firecracker sprite in place.
pub async fn pause_sprite_fc(api_socket: &Path) -> Result<(), LibvirtError> {
    api_patch(api_socket, "/vm", r#"{"state":"Paused"}"#).await
}

/// Resume a paused Firecracker sprite.
pub async fn resume_sprite_fc(api_socket: &Path) -> Result<(), LibvirtError> {
    api_patch(api_socket, "/vm", r#"{"state":"Resumed"}"#).await
}

pub struct FcSnapshotResult {
    pub snapshot_path: PathBuf,
    pub mem_file_path: PathBuf,
}

/// Snapshot a Firecracker sprite's full VM state. Caller must ensure the VM
/// is already `Paused` — Firecracker's `/snapshot/create` requires it — via
/// `daemon::sprite_registry`'s state machine, not a blind pause here (same
/// reasoning as the Cloud Hypervisor sibling).
pub async fn snapshot_sprite_fc(
    sprite_id: &str,
    api_socket: &Path,
) -> Result<FcSnapshotResult, LibvirtError> {
    let run_dir = PathBuf::from(SPRITE_RUN_DIR).join(sprite_id);
    let snapshot_path = run_dir.join("snapshot.bin");
    let mem_file_path = run_dir.join("mem.bin");
    api_put(
        api_socket,
        "/snapshot/create",
        &format!(
            r#"{{"snapshot_type":"Full","snapshot_path":{},"mem_file_path":{}}}"#,
            serde_json::to_string(&snapshot_path.display().to_string()).unwrap_or_default(),
            serde_json::to_string(&mem_file_path.display().to_string()).unwrap_or_default(),
        ),
    )
    .await?;
    Ok(FcSnapshotResult {
        snapshot_path,
        mem_file_path,
    })
}

/// Stop a Firecracker sprite's process as part of suspending it to a
/// snapshot — unlike `teardown_sprite_fc`, deliberately does **not** delete
/// `disk_path` or the run directory (the snapshot/mem files and the disk
/// they reference must survive for `restore_sprite_fc`); only cleans up the
/// now-stale api/vsock sockets and TAP.
pub async fn stop_sprite_fc_for_suspend(
    pid: u32,
    api_socket: &Path,
    vsock_socket: &Path,
    tap_name: Option<&str>,
) -> Result<(), String> {
    let _ = crate::libvirt::extras::kill_host_process(pid, "TERM");
    tokio::time::sleep(SHUTDOWN_GRACE).await;
    let _ = crate::libvirt::extras::kill_host_process(pid, "KILL");
    if let Some(tap) = tap_name {
        delete_egress_tap(tap);
    }
    let _ = fs::remove_file(api_socket);
    let _ = fs::remove_file(vsock_socket);
    Ok(())
}

pub struct FcRestoreRequest<'a> {
    pub sprite_id: &'a str,
    pub snapshot_path: &'a Path,
    pub mem_file_path: &'a Path,
    pub network_egress: bool,
}

pub struct FcRestoreResult {
    pub pid: u32,
    pub api_socket: PathBuf,
    pub vsock_socket: PathBuf,
    pub tap_name: Option<String>,
}

/// Restore a suspended Firecracker sprite from its snapshot: spawn a bare
/// `firecracker --api-sock <path>` (same spawn/poll pattern as
/// `boot_sprite_fc`'s first half), then `PUT /snapshot/load`. No `/vsock`
/// (or any other device) re-PUT beforehand — a Firecracker snapshot captures
/// full configured-device state including the vsock CID, so `/snapshot/load`
/// restores it directly; this is the one design assumption from the plan
/// that should get an empirical double-check against a real restore cycle.
pub async fn restore_sprite_fc(
    req: &FcRestoreRequest<'_>,
) -> Result<FcRestoreResult, LibvirtError> {
    let fc_binary = find_firecracker_binary()?;
    let run_dir = PathBuf::from(SPRITE_RUN_DIR).join(req.sprite_id);
    let api_socket = run_dir.join("api.sock");
    let vsock_socket = run_dir.join("vsock.sock");
    let _ = fs::remove_file(&api_socket);

    let tap_name = if req.network_egress {
        let tap = tap_name_for("fc-", req.sprite_id);
        create_egress_tap(&tap)?;
        Some(tap)
    } else {
        None
    };

    let mut command = TokioCommand::new(fc_binary);
    command
        .arg("--api-sock")
        .arg(&api_socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|e| {
        if let Some(tap) = &tap_name {
            delete_egress_tap(tap);
        }
        LibvirtError::Operation(format!("failed to spawn firecracker for restore: {e}"))
    })?;
    let pid = child
        .id()
        .ok_or_else(|| LibvirtError::Internal("firecracker spawned without a pid".into()))?;

    // Same tee-into-tracing-and-last-line pattern as boot_sprite_fc — a
    // restore that crashes the guest (e.g. resuming into a kernel panic
    // loop) previously left zero diagnostic trail, since this used to run
    // with stderr silenced entirely.
    let last_stderr_line = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    if let Some(stderr) = child.stderr.take() {
        let last_stderr_line = last_stderr_line.clone();
        let sprite_id = req.sprite_id.to_string();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::warn!("firecracker[{sprite_id}] (restore): {line}");
                if let Ok(mut last) = last_stderr_line.lock() {
                    *last = line;
                }
            }
        });
    }

    let deadline = Instant::now() + BOOT_READY_TIMEOUT;
    loop {
        if api_socket.exists() {
            break;
        }
        if let Ok(Some(status)) = child.try_wait() {
            if let Some(tap) = &tap_name {
                delete_egress_tap(tap);
            }
            let last_line = last_stderr_line
                .lock()
                .map(|l| l.clone())
                .unwrap_or_default();
            return Err(LibvirtError::Operation(format!(
                "firecracker exited before its API socket appeared during restore (status: {status}): {last_line}"
            )));
        }
        if Instant::now() >= deadline {
            let _ = child.start_kill();
            if let Some(tap) = &tap_name {
                delete_egress_tap(tap);
            }
            return Err(LibvirtError::Operation(
                "firecracker did not become ready within timeout during restore".into(),
            ));
        }
        tokio::time::sleep(BOOT_READY_POLL_INTERVAL).await;
    }

    if let Err(e) = api_put(
        &api_socket,
        "/snapshot/load",
        &format!(
            r#"{{"snapshot_path":{},"mem_backend":{{"backend_type":"File","backend_path":{}}},"resume_vm":true}}"#,
            serde_json::to_string(&req.snapshot_path.display().to_string()).unwrap_or_default(),
            serde_json::to_string(&req.mem_file_path.display().to_string()).unwrap_or_default(),
        ),
    )
    .await
    {
        let _ = child.start_kill();
        if let Some(tap) = &tap_name {
            delete_egress_tap(tap);
        }
        return Err(e);
    }

    // `/snapshot/load` returning success only means the load itself was
    // accepted — resuming into a guest that was mid-crash when snapshotted
    // can still bring the process down moments later (observed live: HTTP
    // 200 from /snapshot/load, then the process was gone within ~1s with no
    // error surfaced). A short liveness check catches that instead of
    // reporting a restore as successful when it silently isn't.
    tokio::time::sleep(Duration::from_millis(300)).await;
    if let Ok(Some(status)) = child.try_wait() {
        if let Some(tap) = &tap_name {
            delete_egress_tap(tap);
        }
        let last_line = last_stderr_line
            .lock()
            .map(|l| l.clone())
            .unwrap_or_default();
        return Err(LibvirtError::Operation(format!(
            "firecracker exited shortly after /snapshot/load during restore (status: {status}): {last_line}"
        )));
    }

    drop(child);
    Ok(FcRestoreResult {
        pid,
        api_socket,
        vsock_socket,
        tap_name,
    })
}

impl FcBootRequest<'_> {
    /// The vsock socket path this sprite will use, computed the same way
    /// `boot_sprite_fc` derives it (`SPRITE_RUN_DIR/<sprite_id>/vsock.sock`)
    /// — needed one call earlier than `FcBootResult` exists, since
    /// `/vsock`'s `uds_path` must be configured before `InstanceStart`.
    fn vsock_socket_path_for_body(&self) -> String {
        PathBuf::from(SPRITE_RUN_DIR)
            .join(self.sprite_id)
            .join("vsock.sock")
            .display()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite_net::EGRESS_BRIDGE;
    use std::process::Command;

    /// Regression test for a real bug found live: Firecracker auto-appends
    /// `root=/dev/vda rw` (unpartitioned) for the root drive, overriding
    /// any `root=/dev/vda1` a caller supplies — booting a full partitioned
    /// disk straight from `qemu-img convert` panics the guest kernel with
    /// "Unable to mount root fs" (see this module's doc comment). This
    /// locks in that `root_partition_extent`/`materialize_raw_disk`
    /// actually extract partition 1's content rather than handing
    /// Firecracker the whole partitioned disk.
    /// These tests build real disk images, so they need `qemu-img` (and, for the GPT one, `sfdisk`); on a host without them they
    /// skip rather than fail. (CI installs `qemu-utils`, so they run there.)
    fn tool_available(tool: &str, arg: &str) -> bool {
        Command::new(tool).arg(arg).output().map(|o| o.status.success()).unwrap_or(false)
    }

    #[test]
    fn materialize_raw_disk_extracts_partition_one_from_a_gpt_disk() {
        if !tool_available("sfdisk", "--version") || !tool_available("qemu-img", "--version") {
            eprintln!("skipping: sfdisk or qemu-img not installed on this host");
            return;
        }
        let tmp = std::env::temp_dir().join(format!("fc-part-extent-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let disk = tmp.join("disk.raw");

        // A small GPT disk with one ~16MiB partition starting at sector
        // 2048 — same shape (if not size) as the real golden images'
        // "partition 1 is root" layout.
        Command::new("qemu-img")
            .args(["create", "-f", "raw", disk.to_str().unwrap(), "32M"])
            .output()
            .expect("qemu-img create");
        let sfdisk_script =
            "label: gpt\nstart=2048, size=32000, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4\n";
        let mut child = Command::new("sfdisk")
            .arg(&disk)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sfdisk");
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(sfdisk_script.as_bytes())
            .unwrap();
        let status = child.wait().expect("sfdisk wait");
        if !status.success() {
            eprintln!("skipping: sfdisk failed to partition the test disk");
            let _ = std::fs::remove_dir_all(&tmp);
            return;
        }

        let extent = root_partition_extent(&disk);
        assert_eq!(
            extent,
            Some((2048, 32000)),
            "expected partition 1's parsed (start, size)"
        );

        let dest = tmp.join("root.raw");
        let source_qcow2 = tmp.join("source.qcow2");
        Command::new("qemu-img")
            .args([
                "convert",
                "-O",
                "qcow2",
                disk.to_str().unwrap(),
                source_qcow2.to_str().unwrap(),
            ])
            .output()
            .expect("qemu-img convert to qcow2");
        materialize_raw_disk(&source_qcow2, &dest).expect("materialize_raw_disk");

        let extracted_size = std::fs::metadata(&dest).unwrap().len();
        assert_eq!(
            extracted_size,
            32000 * 512,
            "extracted raw disk should be exactly partition 1's byte size, not the whole disk"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn root_partition_extent_is_none_for_an_unpartitioned_disk() {
        if !tool_available("qemu-img", "--version") {
            eprintln!("skipping: qemu-img not installed on this host");
            return;
        }
        let tmp = std::env::temp_dir().join(format!("fc-part-extent-blank-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let disk = tmp.join("blank.raw");
        Command::new("qemu-img")
            .args(["create", "-f", "raw", disk.to_str().unwrap(), "8M"])
            .output()
            .expect("qemu-img create");
        assert_eq!(root_partition_extent(&disk), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn boot_sprite_fc_rejects_bogus_request_without_leaking_run_dir() {
        // Deliberately doesn't assert *which* precondition trips first
        // (missing firecracker binary/kernel vs. missing golden image) —
        // this module's own tests run on hosts that install firecracker by
        // default (see install.sh's ensure_firecracker), so pinning to
        // find_firecracker_binary()'s NotFound would be true on a bare
        // dev/CI container but false on a real daemon host — either way, a
        // bogus golden image path must fail, and must not leave the
        // per-sprite run directory behind.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let sprite_id = "fc-unit-test-does-not-exist";
        let req = FcBootRequest {
            sprite_id,
            golden_image_path: Path::new("/nonexistent/golden.qcow2"),
            vcpus: 1,
            memory_mb: 512,
            vsock_cid: 3,
            network_egress: false,
        };
        let result = rt.block_on(boot_sprite_fc(&req));
        assert!(result.is_err());
        assert!(!PathBuf::from(SPRITE_RUN_DIR).join(sprite_id).exists());
    }

    /// Full boot -> teardown lifecycle against a *real* `firecracker`
    /// install — the one thing the test above deliberately doesn't cover.
    /// Skips cleanly rather than failing when firecracker/its kernel aren't
    /// installed (most dev/CI hosts) or `SPRITE_RUN_DIR` isn't writable
    /// (needs root, same as the real daemon) — meant to actually run
    /// wherever both are true, e.g. `sudo cargo test` on a deployed host.
    ///
    /// Uses a blank, non-bootable synthetic disk: this only needs to prove
    /// the VMM process starts, its API socket comes up, and `InstanceStart`
    /// succeeds at the host/VMM level — it doesn't depend on there being a
    /// real bootable guest OS on the disk (the guest itself would
    /// kernel-panic on a non-bootable rootfs, same as Cloud Hypervisor's
    /// equivalent test not needing a real guest either).
    #[test]
    fn boot_and_teardown_sprite_fc_full_lifecycle_against_real_firecracker() {
        if find_firecracker_binary().is_err() || find_firecracker_kernel().is_err() {
            eprintln!("skipping: firecracker or its kernel not installed on this host");
            return;
        }
        if fs::create_dir_all(SPRITE_RUN_DIR).is_err() {
            eprintln!(
                "skipping: cannot write to {SPRITE_RUN_DIR} (needs root, like the real daemon)"
            );
            return;
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let sprite_id = format!("fc-integration-test-{}", std::process::id());

        let golden_path = std::env::temp_dir().join(format!("{sprite_id}-golden.qcow2"));
        let out = Command::new("qemu-img")
            .args([
                "create",
                "-f",
                "qcow2",
                golden_path.to_str().unwrap(),
                "16M",
            ])
            .output()
            .expect("qemu-img create");
        assert!(
            out.status.success(),
            "qemu-img create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let boot_result = rt.block_on(boot_sprite_fc(&FcBootRequest {
            sprite_id: &sprite_id,
            golden_image_path: &golden_path,
            vcpus: 1,
            memory_mb: 128,
            // Not going through the daemon's registry allocator (this is a
            // unit test), so pick something unlikely to collide with a
            // sprite a live daemon on the same host is actually running.
            vsock_cid: 4500 + (std::process::id() % 1000),
            network_egress: false,
        }));

        let _ = fs::remove_file(&golden_path);

        let result = match boot_result {
            Ok(r) => r,
            Err(e) => panic!("boot_sprite_fc failed against a real firecracker install: {e}"),
        };
        assert!(result.api_socket.exists());
        assert!(result.disk_path.exists());
        assert!(result.tap_name.is_none());

        rt.block_on(teardown_sprite_fc(
            result.pid,
            &result.api_socket,
            &result.disk_path,
            &result.vsock_socket,
            result.tap_name.as_deref(),
        ))
        .expect("teardown_sprite_fc");

        assert!(
            !result.disk_path.exists(),
            "teardown should remove the raw disk copy"
        );
        assert!(
            !result.api_socket.exists(),
            "teardown should remove the api socket"
        );
    }

    /// Same lifecycle as above, but with `network_egress: true` — proves
    /// the TAP actually gets created, attached to `virbr0`, and cleaned up.
    #[test]
    fn boot_and_teardown_sprite_fc_with_network_egress() {
        if find_firecracker_binary().is_err() || find_firecracker_kernel().is_err() {
            eprintln!("skipping: firecracker or its kernel not installed on this host");
            return;
        }
        if fs::create_dir_all(SPRITE_RUN_DIR).is_err() {
            eprintln!(
                "skipping: cannot write to {SPRITE_RUN_DIR} (needs root, like the real daemon)"
            );
            return;
        }
        if !Command::new("ip")
            .args(["link", "show", EGRESS_BRIDGE])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            eprintln!("skipping: {EGRESS_BRIDGE} doesn't exist on this host (libvirt \"default\" network not active)");
            return;
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let sprite_id = format!("fc-integration-test-egress-{}", std::process::id());

        let golden_path = std::env::temp_dir().join(format!("{sprite_id}-golden.qcow2"));
        let out = Command::new("qemu-img")
            .args([
                "create",
                "-f",
                "qcow2",
                golden_path.to_str().unwrap(),
                "16M",
            ])
            .output()
            .expect("qemu-img create");
        assert!(
            out.status.success(),
            "qemu-img create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let boot_result = rt.block_on(boot_sprite_fc(&FcBootRequest {
            sprite_id: &sprite_id,
            golden_image_path: &golden_path,
            vcpus: 1,
            memory_mb: 128,
            vsock_cid: 4600 + (std::process::id() % 1000),
            network_egress: true,
        }));

        let _ = fs::remove_file(&golden_path);

        let result = match boot_result {
            Ok(r) => r,
            Err(e) => panic!("boot_sprite_fc with network_egress failed: {e}"),
        };
        let tap = result
            .tap_name
            .clone()
            .expect("network_egress requested a TAP");
        assert_eq!(tap, tap_name_for("fc-", &sprite_id));

        let tap_exists = Command::new("ip")
            .args(["link", "show", &tap])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(
            tap_exists,
            "TAP {tap} should exist while the sprite is running"
        );

        rt.block_on(teardown_sprite_fc(
            result.pid,
            &result.api_socket,
            &result.disk_path,
            &result.vsock_socket,
            result.tap_name.as_deref(),
        ))
        .expect("teardown_sprite_fc");

        let tap_exists_after = Command::new("ip")
            .args(["link", "show", &tap])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(!tap_exists_after, "teardown should remove the TAP device");
    }
}

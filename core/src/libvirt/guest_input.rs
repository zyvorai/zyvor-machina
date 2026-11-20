// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Send keys and capture screenshots via libvirt (Cockpit-style console helpers).

use virt::connect::Connect;
use virt::stream::Stream;
use virt::sys;

use super::domain::lookup_domain;
use crate::LibvirtError;

/// Linux evdev keycodes (input-event-codes.h) for common shortcuts.
pub const KEY_LEFTCTRL: u32 = 29;
pub const KEY_LEFTALT: u32 = 56;
pub const KEY_DELETE: u32 = 111;
pub const KEY_ESC: u32 = 1;
pub const KEY_TAB: u32 = 15;

/// Send keycodes using `VIR_KEYCODE_SET_LINUX` (holdtime ms between press/release simulation).
pub fn send_linux_keycodes(
    conn: &Connect,
    vm_name: &str,
    keycodes: &[u32],
    holdtime_ms: u32,
) -> Result<(), LibvirtError> {
    if keycodes.is_empty() {
        return Err(LibvirtError::Invalid("keycodes must not be empty".into()));
    }
    let domain = lookup_domain(conn, vm_name)?;
    // `virDomainSendKey` takes an explicit `nkeycodes` count — there is no NUL-terminator
    // convention for this array (unlike a C string). A trailing `0` used to be appended
    // and counted, which both injected a phantom keycode-0 press/release into every call
    // and could push a full-length (16-key) request one over libvirt's
    // VIR_DOMAIN_SEND_KEY_MAX_KEYS limit, failing a combination the caller actually sent.
    let mut codes: Vec<u32> = keycodes.to_vec();
    domain
        .send_key(
            sys::VIR_KEYCODE_SET_LINUX,
            holdtime_ms,
            codes.as_mut_ptr(),
            codes.len() as i32,
            0,
        )
        .map_err(|e| LibvirtError::Operation(format!("send_key '{vm_name}': {e}")))
}

/// Capture the VM display as PNG (or JPEG) bytes. VM should be running.
pub fn screenshot(
    conn: &Connect,
    vm_name: &str,
    screen: u32,
) -> Result<(Vec<u8>, String), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let stream =
        Stream::new(conn, 0).map_err(|e| LibvirtError::Operation(format!("stream new: {e}")))?;
    let mime = domain
        .screenshot(&stream, screen, 0)
        .map_err(|e| LibvirtError::Operation(format!("screenshot '{vm_name}': {e}")))?;

    let mut out = Vec::new();
    let mut buf = [0u8; 65_536];
    loop {
        let n = stream
            .recv(&mut buf)
            .map_err(|e| LibvirtError::Operation(format!("screenshot recv: {e}")))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    stream
        .finish()
        .map_err(|e| LibvirtError::Operation(format!("screenshot finish: {e}")))?;
    Ok((out, mime))
}

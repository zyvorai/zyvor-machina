// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM lifecycle webhook helpers for route handlers.

use machina_core::libvirt::automation::fire_vm_event;
use serde_json::json;

pub fn emit_vm_started(name: &str) {
    fire_vm_event("vm_started", name, &json!({}));
}

pub fn emit_vm_stopped(name: &str) {
    fire_vm_event("vm_stopped", name, &json!({}));
}

pub fn emit_vm_shutdown(name: &str) {
    fire_vm_event("vm_shutdown", name, &json!({}));
}

pub fn emit_vm_reboot(name: &str) {
    fire_vm_event("vm_reboot", name, &json!({}));
}

pub fn emit_vm_paused(name: &str) {
    fire_vm_event("vm_paused", name, &json!({}));
}

pub fn emit_vm_resumed(name: &str) {
    fire_vm_event("vm_resumed", name, &json!({}));
}

pub fn emit_vm_deleted(name: &str) {
    fire_vm_event("vm_deleted", name, &json!({}));
}

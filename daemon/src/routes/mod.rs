// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

mod advanced;
mod automation;
mod backup;
mod bpf;
mod console;
pub mod consolehub;
pub(crate) mod events;
mod extras;
mod fleet;
pub(crate) mod fluxvm;
mod guest_images;
mod guest_policy;
mod guestkit;
mod health;
mod host_network;
mod integrations;
mod jobs;
mod k8s;
mod kubevirt;
mod metrics;
pub(crate) mod netpol;
mod networks;
mod node;
mod platform_controller;
mod platform_ws;
mod prometheus;
mod snapshots;
mod sprites;
mod storage;
mod system;
mod templates;
mod vessel;
mod vm_guest;
mod vms;
mod ws;
mod zeus_firewall;

use axum::Router;
use machina_core::LibvirtManager;

pub fn api_routes() -> Router<LibvirtManager> {
    Router::new()
        .merge(events::event_routes())
        .merge(jobs::job_routes())
        .merge(vessel::vessel_routes())
        .merge(k8s::k8s_routes())
        .merge(kubevirt::kubevirt_routes())
        .merge(integrations::integrations_routes())
        .merge(fluxvm::fluxvm_routes())
        .merge(guestkit::guestkit_routes())
        .merge(zeus_firewall::zeus_firewall_routes())
        .merge(platform_controller::platform_controller_routes())
        .merge(vms::vm_routes())
        .merge(sprites::sprite_routes())
        .merge(vm_guest::vm_guest_routes())
        .merge(guest_policy::guest_policy_routes())
        .merge(snapshots::snapshot_routes())
        .merge(networks::network_routes())
        .merge(storage::storage_routes())
        .merge(node::node_routes())
        .merge(metrics::metrics_routes())
        .merge(health::health_routes())
        .merge(guest_images::guest_image_routes())
        .merge(templates::template_routes())
        .merge(prometheus::prometheus_routes())
        .merge(console::console_routes())
        .merge(consolehub::api_routes())
        .merge(advanced::advanced_routes())
        .merge(backup::backup_routes())
        .merge(bpf::bpf_routes())
        .merge(netpol::netpol_routes())
        .merge(host_network::host_network_routes())
        .merge(extras::extras_routes())
        .merge(automation::automation_routes())
        .merge(system::system_routes())
        .merge(fleet::fleet_routes())
}

pub fn websocket_routes() -> Router<LibvirtManager> {
    Router::new()
        .merge(ws::ws_routes())
        .merge(vessel::vessel_ws_routes())
        .merge(platform_ws::platform_ws_routes())
}

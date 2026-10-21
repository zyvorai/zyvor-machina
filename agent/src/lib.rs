// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

pub mod backup;
pub mod bpf_ops;
pub mod cloud_fabric;
pub mod console_ws;
pub mod eip;
pub mod enrol;
pub mod epoch;
pub mod fluxvm;
pub mod grpc;
pub mod imds;
pub mod jwt;
pub mod k3s;
pub mod lbprobe;
pub mod libvirt_invoke;
pub mod natgw;
pub mod libvirt_ops;
pub mod provision_ops;
pub mod state;
pub mod wake;

#[allow(clippy::result_large_err)]
pub mod pb {
    tonic::include_proto!("machina.agent.v1");
}

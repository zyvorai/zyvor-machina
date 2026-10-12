// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#![allow(clippy::result_large_err, clippy::type_complexity)]

pub mod agent_client;
pub mod api;
pub mod auth;
pub mod baremetal_sol;
pub mod config;
pub mod console;
pub mod consolehub;
pub mod db;
pub mod enrollment_tls;
pub mod engine;
pub mod jwt;
pub mod leader;
pub mod project_rbac;
pub mod resource_ids;
pub mod oidc_flow;
pub mod pki;
pub mod rate_limit;
pub mod state;
pub mod sync;
pub mod tasks;
pub mod ws_tokens;

pub use config::ControllerConfig;

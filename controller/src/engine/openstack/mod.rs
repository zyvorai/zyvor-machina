// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! First-party OpenStack importer: Keystone auth, inventory, Glance download, land as template.

pub mod client;
pub mod import;
pub mod inventory;
pub mod mock;

pub use client::{ConnectionConfig, OpenStackClient};
pub use import::{run_import_job, ImportRequest};
pub use inventory::{sync_inventory, InventoryItem, InventoryKind};

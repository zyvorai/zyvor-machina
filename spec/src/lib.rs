// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Declarative platform specs (`virt.zyvor.dev/v1`).

mod cloud;
mod cluster;
mod error;
mod ha;
mod host;
mod sprite;
mod task;
mod template;
mod vm;

pub use cloud::*;
pub use cluster::*;
pub use error::*;
pub use ha::*;
pub use host::*;
pub use sprite::*;
pub use task::*;
pub use template::*;
pub use vm::*;

pub const API_VERSION: &str = "virt.zyvor.dev/v1";

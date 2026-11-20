// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

mod http;
mod pty;
mod sessions;

pub use pty::run_ssh_terminal;
pub use sessions::{PendingSession, TerminalSessionStore};

pub fn http_routes() -> axum::Router<machina_core::LibvirtManager> {
    http::routes()
}

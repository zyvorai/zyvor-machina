// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

pub mod client;
pub mod engine;
pub mod engines;
pub mod error;
pub mod models;
pub mod stream;

pub use client::VesselClient;
pub use engine::Engine;
pub use error::VesselError;
pub use models::*;
pub use stream::{EngineEvent, LogLine};

#[cfg(test)]
mod connect_smoke {
    #[tokio::test]
    async fn connect_if_socket_present() {
        if crate::engines::discover_socket(None).is_none() {
            eprintln!("skip: no container socket");
            return;
        }
        match crate::VesselClient::connect_local(None).await {
            Ok(c) => {
                let info = c.host_info().await.expect("host_info");
                eprintln!(
                    "connected engine={:?} version={}",
                    info.engine, info.version
                );
                let list = c.list_containers(true).await.expect("list");
                eprintln!("containers={}", list.len());
                if c.capabilities().pods {
                    let pods = c.list_pods().await.expect("pods");
                    eprintln!("pods={}", pods.len());
                }
            }
            Err(e) => panic!("connect failed: {e}"),
        }
    }
}

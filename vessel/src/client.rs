// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::engine::Engine;
use crate::error::VesselError;
use crate::models::*;
use crate::stream::{EventItem, LogItem, StatsItem};
use futures::stream::BoxStream;
use std::sync::Arc;

/// Facade over an [`Engine`] implementation (bollard today).
#[derive(Clone)]
pub struct VesselClient {
    engine: Arc<dyn Engine>,
    kind: EngineKind,
    socket: String,
}

impl VesselClient {
    pub async fn connect_local(socket_override: Option<&str>) -> Result<Self, VesselError> {
        let (engine, kind, socket) = crate::engines::connect_local(socket_override).await?;
        Ok(Self {
            engine,
            kind,
            socket,
        })
    }

    pub fn engine_kind(&self) -> &EngineKind {
        &self.kind
    }

    pub fn socket(&self) -> &str {
        &self.socket
    }

    pub fn capabilities(&self) -> EngineCapabilities {
        self.engine.capabilities()
    }

    pub async fn host_info(&self) -> Result<HostInfo, VesselError> {
        self.engine.host_info().await
    }

    pub async fn list_containers(&self, all: bool) -> Result<Vec<ContainerSummary>, VesselError> {
        self.engine.list_containers(all).await
    }

    pub async fn create_container(
        &self,
        req: CreateContainerRequest,
    ) -> Result<CreateContainerResponse, VesselError> {
        self.engine.create_container(req).await
    }

    pub async fn start_container(&self, id: &str) -> Result<(), VesselError> {
        self.engine.start_container(id).await
    }

    pub async fn stop_container(&self, id: &str) -> Result<(), VesselError> {
        self.engine.stop_container(id).await
    }

    pub async fn restart_container(&self, id: &str) -> Result<(), VesselError> {
        self.engine.restart_container(id).await
    }

    pub async fn remove_container(&self, id: &str, force: bool) -> Result<(), VesselError> {
        self.engine.remove_container(id, force).await
    }

    pub fn stats_stream(&self, id: &str) -> BoxStream<'static, StatsItem> {
        self.engine.stats_stream(id)
    }

    pub fn logs_stream(
        &self,
        id: &str,
        follow: bool,
        tail: Option<u64>,
    ) -> BoxStream<'static, LogItem> {
        self.engine.logs_stream(id, follow, tail)
    }

    pub async fn list_pods(&self) -> Result<Vec<PodSummary>, VesselError> {
        self.engine.list_pods().await
    }

    pub async fn create_pod(
        &self,
        req: CreatePodRequest,
    ) -> Result<CreatePodResponse, VesselError> {
        self.engine.create_pod(req).await
    }

    pub async fn start_pod(&self, id: &str) -> Result<(), VesselError> {
        self.engine.start_pod(id).await
    }

    pub async fn stop_pod(&self, id: &str) -> Result<(), VesselError> {
        self.engine.stop_pod(id).await
    }

    pub async fn remove_pod(&self, id: &str, force: bool) -> Result<(), VesselError> {
        self.engine.remove_pod(id, force).await
    }

    pub fn events_stream(&self) -> BoxStream<'static, EventItem> {
        self.engine.events_stream()
    }
}

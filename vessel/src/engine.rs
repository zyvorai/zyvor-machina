// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::error::VesselError;
use crate::models::*;
use crate::stream::{EventItem, LogItem, StatsItem};
use async_trait::async_trait;
use futures::stream::BoxStream;

#[async_trait]
pub trait Engine: Send + Sync {
    fn kind(&self) -> EngineKind;
    fn capabilities(&self) -> EngineCapabilities;

    async fn host_info(&self) -> Result<HostInfo, VesselError>;

    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerSummary>, VesselError>;
    async fn create_container(
        &self,
        req: CreateContainerRequest,
    ) -> Result<CreateContainerResponse, VesselError>;
    async fn start_container(&self, id: &str) -> Result<(), VesselError>;
    async fn stop_container(&self, id: &str) -> Result<(), VesselError>;
    async fn restart_container(&self, id: &str) -> Result<(), VesselError>;
    async fn remove_container(&self, id: &str, force: bool) -> Result<(), VesselError>;

    fn stats_stream(&self, id: &str) -> BoxStream<'static, StatsItem>;
    fn logs_stream(&self, id: &str, follow: bool, tail: Option<u64>)
        -> BoxStream<'static, LogItem>;

    async fn list_pods(&self) -> Result<Vec<PodSummary>, VesselError>;
    async fn create_pod(&self, req: CreatePodRequest) -> Result<CreatePodResponse, VesselError>;
    async fn start_pod(&self, id: &str) -> Result<(), VesselError>;
    async fn stop_pod(&self, id: &str) -> Result<(), VesselError>;
    async fn remove_pod(&self, id: &str, force: bool) -> Result<(), VesselError>;

    fn events_stream(&self) -> BoxStream<'static, EventItem>;
}

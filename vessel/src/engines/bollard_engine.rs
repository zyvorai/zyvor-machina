// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use async_trait::async_trait;
use bollard::models::{ContainerCreateBody, ContainerStatsResponse};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, ListContainersOptionsBuilder, LogsOptionsBuilder,
    RemoveContainerOptionsBuilder, StatsOptionsBuilder,
};
use bollard::Docker;
use chrono::{TimeZone, Utc};
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::libpod::LibpodClient;
use crate::engine::Engine;
use crate::error::VesselError;
use crate::models::*;
use crate::stream::{EventItem, LogItem, LogLine, StatsItem};

pub struct BollardEngine {
    docker: Docker,
    kind: EngineKind,
    #[allow(dead_code)]
    socket: String,
    libpod: Option<LibpodClient>,
}

pub fn discover_socket(socket_override: Option<&str>) -> Option<String> {
    if let Some(s) = socket_override.map(str::trim).filter(|s| !s.is_empty()) {
        let path = s.strip_prefix("unix://").unwrap_or(s);
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
        return None;
    }

    for key in ["CONTAINER_HOST", "DOCKER_HOST"] {
        if let Ok(host) = std::env::var(key) {
            if let Some(path) = host.strip_prefix("unix://") {
                if Path::new(path).exists() {
                    return Some(path.to_string());
                }
            } else if host.starts_with('/') && Path::new(&host).exists() {
                return Some(host);
            }
        }
    }

    let mut candidates: Vec<String> = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        candidates.push(format!("{xdg}/podman/podman.sock"));
    }
    if let Ok(uid) = std::env::var("UID") {
        candidates.push(format!("/run/user/{uid}/podman/podman.sock"));
    }
    // Common rootless path when UID env is unset (probe a few likely uids is noisy —
    // rely on XDG_RUNTIME_DIR / CONTAINER_HOST instead).
    candidates.push("/run/podman/podman.sock".into());
    candidates.push("/var/run/docker.sock".into());
    candidates.push("/run/docker.sock".into());

    candidates.into_iter().find(|p| Path::new(p).exists())
}

pub async fn connect_local(
    socket_override: Option<&str>,
) -> Result<(Arc<dyn Engine>, EngineKind, String), VesselError> {
    let socket = discover_socket(socket_override).ok_or_else(|| {
        VesselError::Connection(
            "No Podman/Docker unix socket found (tried CONTAINER_HOST, DOCKER_HOST, rootless/system Podman, /var/run/docker.sock)".into(),
        )
    })?;

    let docker = Docker::connect_with_unix(&socket, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|e| VesselError::Connection(e.to_string()))?;

    let info = docker
        .info()
        .await
        .map_err(|e| VesselError::Connection(format!("engine info: {e}")))?;

    let os = info.operating_system.clone().unwrap_or_default();
    let name_hint = info.name.clone().unwrap_or_default().to_ascii_lowercase();
    let is_podman = os.to_ascii_lowercase().contains("podman")
        || name_hint.contains("podman")
        || std::env::var("CONTAINER_HOST").is_ok()
        || socket.contains("podman");

    let kind = if is_podman {
        EngineKind::Podman
    } else {
        EngineKind::Docker
    };

    let libpod = if matches!(kind, EngineKind::Podman) {
        Some(LibpodClient::new(socket.clone()))
    } else {
        None
    };

    let engine = BollardEngine {
        docker,
        kind: kind.clone(),
        socket: socket.clone(),
        libpod,
    };

    Ok((Arc::new(engine), kind, socket))
}

fn map_api(e: bollard::errors::Error) -> VesselError {
    let msg = e.to_string();
    if msg.to_ascii_lowercase().contains("no such") {
        VesselError::NotFound(msg)
    } else {
        VesselError::Api(msg)
    }
}

pub(crate) fn calculate_cpu_percent(stats: &ContainerStatsResponse) -> f64 {
    let cpu = stats.cpu_stats.as_ref();
    let precpu = stats.precpu_stats.as_ref();
    let (Some(cpu), Some(precpu)) = (cpu, precpu) else {
        return 0.0;
    };
    let total = cpu
        .cpu_usage
        .as_ref()
        .and_then(|u| u.total_usage)
        .unwrap_or(0) as f64;
    let pre_total = precpu
        .cpu_usage
        .as_ref()
        .and_then(|u| u.total_usage)
        .unwrap_or(0) as f64;
    let cpu_delta = total - pre_total;
    let system_delta =
        cpu.system_cpu_usage.unwrap_or(0) as f64 - precpu.system_cpu_usage.unwrap_or(0) as f64;
    let online = cpu.online_cpus.unwrap_or(1).max(1) as f64;
    if system_delta > 0.0 && cpu_delta > 0.0 {
        (cpu_delta / system_delta) * online * 100.0
    } else {
        0.0
    }
}

fn stats_to_model(stats: ContainerStatsResponse) -> ContainerStats {
    let memory_usage = stats
        .memory_stats
        .as_ref()
        .and_then(|m| m.usage)
        .unwrap_or(0);
    let memory_limit = stats
        .memory_stats
        .as_ref()
        .and_then(|m| m.limit)
        .unwrap_or(0);
    let mut network_rx = 0u64;
    let mut network_tx = 0u64;
    if let Some(nets) = stats.networks.as_ref() {
        for n in nets.values() {
            network_rx = network_rx.saturating_add(n.rx_bytes.unwrap_or(0));
            network_tx = network_tx.saturating_add(n.tx_bytes.unwrap_or(0));
        }
    }
    ContainerStats {
        cpu_percent: calculate_cpu_percent(&stats),
        memory_usage,
        memory_limit,
        network_rx,
        network_tx,
    }
}

#[derive(Debug, Deserialize)]
struct LibpodPodListItem {
    #[serde(default, rename = "Id")]
    id: String,
    #[serde(default, rename = "Name")]
    name: String,
    #[serde(default, rename = "Status")]
    status: String,
    #[serde(default, rename = "Created")]
    created: String,
    #[serde(default, rename = "Labels")]
    labels: Option<HashMap<String, String>>,
    #[serde(default, rename = "Containers")]
    containers: Option<Vec<LibpodPodContainer>>,
}

#[derive(Debug, Deserialize)]
struct LibpodPodContainer {
    #[serde(default, rename = "Id")]
    id: String,
    #[serde(default, rename = "Name")]
    name: String,
    #[serde(default, rename = "Status")]
    #[allow(dead_code)]
    status: String,
}

#[derive(Debug, Deserialize)]
struct LibpodIdResponse {
    #[serde(default, alias = "Id", alias = "id")]
    id: String,
}

#[async_trait]
impl Engine for BollardEngine {
    fn kind(&self) -> EngineKind {
        self.kind.clone()
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::for_engine(&self.kind)
    }

    async fn host_info(&self) -> Result<HostInfo, VesselError> {
        let info = self.docker.info().await.map_err(map_api)?;
        let version = self.docker.version().await.map_err(map_api)?;
        Ok(HostInfo {
            name: info.name.unwrap_or_else(|| "localhost".into()),
            engine: self.kind.clone(),
            version: version.version.unwrap_or_default(),
            os: info.operating_system.unwrap_or_default(),
            arch: info.architecture.unwrap_or_default(),
            cpus: info.ncpu.unwrap_or(0) as u32,
            memory_total: info.mem_total.unwrap_or(0) as u64,
            capabilities: self.capabilities(),
        })
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerSummary>, VesselError> {
        let options = ListContainersOptionsBuilder::default().all(all).build();
        let containers = self
            .docker
            .list_containers(Some(options))
            .await
            .map_err(map_api)?;

        Ok(containers
            .into_iter()
            .map(|c| {
                let name = c
                    .names
                    .as_ref()
                    .and_then(|n| n.first())
                    .map(|n| n.trim_start_matches('/').to_string())
                    .unwrap_or_default();
                let state = c
                    .state
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".into());
                let created = c
                    .created
                    .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                    .unwrap_or_else(Utc::now);
                let ports = c
                    .ports
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| PortMapping {
                        ip: p.ip,
                        private_port: p.private_port,
                        public_port: p.public_port,
                        typ: p.typ.map(|t| t.to_string()).unwrap_or_else(|| "tcp".into()),
                    })
                    .collect();
                ContainerSummary {
                    id: c.id.unwrap_or_default(),
                    name,
                    image: c.image.unwrap_or_default(),
                    status: parse_container_status(&state),
                    state,
                    created,
                    ports,
                    labels: c.labels.unwrap_or_default(),
                }
            })
            .collect())
    }

    async fn create_container(
        &self,
        req: CreateContainerRequest,
    ) -> Result<CreateContainerResponse, VesselError> {
        let name = req.name.trim();
        let image = req.image.trim();
        if name.is_empty() || image.is_empty() {
            return Err(VesselError::Api(
                "container name and image are required".into(),
            ));
        }

        let options = CreateContainerOptionsBuilder::default().name(name).build();

        let mut body = ContainerCreateBody {
            image: Some(image.to_string()),
            ..Default::default()
        };
        if !req.command.is_empty() {
            body.cmd = Some(req.command);
        }

        let resp = self
            .docker
            .create_container(Some(options), body)
            .await
            .map_err(map_api)?;
        let id = resp.id;
        if id.is_empty() {
            return Err(VesselError::Api(
                "engine returned empty container id".into(),
            ));
        }

        if req.start {
            self.docker
                .start_container(&id, None)
                .await
                .map_err(map_api)?;
        }

        Ok(CreateContainerResponse {
            id,
            name: name.to_string(),
        })
    }

    async fn start_container(&self, id: &str) -> Result<(), VesselError> {
        self.docker.start_container(id, None).await.map_err(map_api)
    }

    async fn stop_container(&self, id: &str) -> Result<(), VesselError> {
        self.docker.stop_container(id, None).await.map_err(map_api)
    }

    async fn restart_container(&self, id: &str) -> Result<(), VesselError> {
        self.docker
            .restart_container(id, None)
            .await
            .map_err(map_api)
    }

    async fn remove_container(&self, id: &str, force: bool) -> Result<(), VesselError> {
        let options = RemoveContainerOptionsBuilder::default()
            .force(force)
            .build();
        self.docker
            .remove_container(id, Some(options))
            .await
            .map_err(map_api)
    }

    fn stats_stream(&self, id: &str) -> BoxStream<'static, StatsItem> {
        let options = StatsOptionsBuilder::default()
            .stream(true)
            .one_shot(false)
            .build();
        let stream = self.docker.stats(id, Some(options));
        stream
            .map(|res| match res {
                Ok(stats) => Ok(stats_to_model(stats)),
                Err(e) => Err(map_api(e)),
            })
            .boxed()
    }

    fn logs_stream(
        &self,
        id: &str,
        follow: bool,
        tail: Option<u64>,
    ) -> BoxStream<'static, LogItem> {
        let mut builder = LogsOptionsBuilder::default()
            .follow(follow)
            .stdout(true)
            .stderr(true)
            .timestamps(false);
        if let Some(t) = tail {
            builder = builder.tail(&t.to_string());
        }
        let options = builder.build();
        let stream = self.docker.logs(id, Some(options));
        stream
            .map(|res| match res {
                Ok(out) => {
                    let (stream, text) = match out {
                        bollard::container::LogOutput::StdOut { message } => {
                            ("stdout", String::from_utf8_lossy(&message).into_owned())
                        }
                        bollard::container::LogOutput::StdErr { message } => {
                            ("stderr", String::from_utf8_lossy(&message).into_owned())
                        }
                        bollard::container::LogOutput::Console { message } => {
                            ("console", String::from_utf8_lossy(&message).into_owned())
                        }
                        bollard::container::LogOutput::StdIn { message } => {
                            ("stdin", String::from_utf8_lossy(&message).into_owned())
                        }
                    };
                    Ok(LogLine {
                        stream: stream.into(),
                        text,
                    })
                }
                Err(e) => Err(map_api(e)),
            })
            .boxed()
    }

    async fn list_pods(&self) -> Result<Vec<PodSummary>, VesselError> {
        let libpod = self
            .libpod
            .as_ref()
            .ok_or_else(|| VesselError::Unsupported("Pod listing requires Podman".into()))?;
        let items: Vec<LibpodPodListItem> = libpod.get_json("/libpod/pods/json").await?;
        Ok(items
            .into_iter()
            .map(|p| {
                let mut containers = Vec::new();
                let mut infra = None;
                if let Some(cs) = p.containers {
                    for c in cs {
                        if c.name.contains("infra") || c.name.ends_with("-infra") {
                            infra = Some(c.id.clone());
                        }
                        if !c.id.is_empty() {
                            containers.push(c.id);
                        } else if !c.name.is_empty() {
                            containers.push(c.name);
                        }
                    }
                }
                let created = chrono::DateTime::parse_from_rfc3339(&p.created)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                PodSummary {
                    id: p.id,
                    name: p.name,
                    status: parse_pod_status(&p.status),
                    infra_container_id: infra,
                    containers,
                    created,
                    labels: p.labels.unwrap_or_default(),
                }
            })
            .collect())
    }

    async fn create_pod(&self, req: CreatePodRequest) -> Result<CreatePodResponse, VesselError> {
        let libpod = self
            .libpod
            .as_ref()
            .ok_or_else(|| VesselError::Unsupported("Pod create requires Podman".into()))?;
        if req.name.trim().is_empty() {
            return Err(VesselError::Api("pod name is required".into()));
        }
        let body = serde_json::json!({
            "name": req.name,
            "labels": req.labels,
        });
        let resp: LibpodIdResponse = libpod.post_json("/libpod/pods/create", &body).await?;
        Ok(CreatePodResponse {
            id: resp.id,
            name: req.name,
        })
    }

    async fn start_pod(&self, id: &str) -> Result<(), VesselError> {
        let libpod = self
            .libpod
            .as_ref()
            .ok_or_else(|| VesselError::Unsupported("Pod start requires Podman".into()))?;
        let path = format!("/libpod/pods/{}/start", urlencoding_path(id));
        libpod.post_empty(&path).await
    }

    async fn stop_pod(&self, id: &str) -> Result<(), VesselError> {
        let libpod = self
            .libpod
            .as_ref()
            .ok_or_else(|| VesselError::Unsupported("Pod stop requires Podman".into()))?;
        let path = format!("/libpod/pods/{}/stop", urlencoding_path(id));
        libpod.post_empty(&path).await
    }

    async fn remove_pod(&self, id: &str, force: bool) -> Result<(), VesselError> {
        let libpod = self
            .libpod
            .as_ref()
            .ok_or_else(|| VesselError::Unsupported("Pod remove requires Podman".into()))?;
        let path = if force {
            format!("/libpod/pods/{}?force=true", urlencoding_path(id))
        } else {
            format!("/libpod/pods/{}", urlencoding_path(id))
        };
        libpod.delete(&path).await
    }

    fn events_stream(&self) -> BoxStream<'static, EventItem> {
        // Lightweight: empty stream for phase 1; mutations emit via daemon EventBus.
        futures::stream::empty().boxed()
    }
}

fn urlencoding_path(id: &str) -> String {
    id.split('/')
        .map(urlencoding_encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_zero_when_missing() {
        let stats = ContainerStatsResponse {
            ..Default::default()
        };
        assert_eq!(calculate_cpu_percent(&stats), 0.0);
    }
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;

use machina_core::libvirt::network;
use machina_core::{CreateNetworkRequest, LibvirtManager, NetworkInfo};

use crate::auth::{require_write, RequestActor};
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::{ok_json, AppError, Xml};

async fn list_networks(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<Vec<NetworkInfo>>, AppError> {
    let rows = spawn_libvirt_actor(manager, Some(&actor), conn_q, network::list_networks).await?;
    Ok(Json(rows))
}

async fn create_network(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<CreateNetworkRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "networks:write")?;
    let name = req.name.clone();
    let req2 = req.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        network::create_network(
            conn,
            &req2.name,
            &req2.subnet,
            &req2.dhcp_start,
            &req2.dhcp_end,
        )
    })
    .await?;
    Ok(ok_json("created", &name))
}

async fn delete_network_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "networks:write")?;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        network::delete_network(conn, &name2)
    })
    .await?;
    Ok(ok_json("deleted", &name))
}

async fn start_network(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "networks:write")?;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        network::start_network(conn, &name2)
    })
    .await?;
    Ok(ok_json("started", &name))
}

async fn stop_network(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "networks:write")?;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        network::stop_network(conn, &name2)
    })
    .await?;
    Ok(ok_json("stopped", &name))
}

async fn get_network_xml(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Xml, AppError> {
    let name2 = name.clone();
    let xml = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        network::get_network_xml(conn, &name2)
    })
    .await?;
    Ok(Xml(xml))
}

#[derive(Deserialize)]
struct UpdateNetworkXmlBody {
    xml: String,
}

async fn update_network_xml(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(body): Json<UpdateNetworkXmlBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "networks:write")?;
    let xml = body.xml;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        network::update_network_xml(conn, &name2, &xml)
    })
    .await?;
    Ok(ok_json("updated", &name))
}

async fn set_network_autostart(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((name, enabled)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "networks:write")?;
    let autostart = enabled == "true" || enabled == "1";
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        network::set_network_autostart(conn, &name2, autostart)
    })
    .await?;
    let label = if autostart { "enabled" } else { "disabled" };
    Ok(ok_json(label, &name))
}

pub fn network_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/networks", get(list_networks))
        .route("/networks", post(create_network))
        .route("/networks/{name}", delete(delete_network_handler))
        .route("/networks/{name}/start", post(start_network))
        .route("/networks/{name}/stop", post(stop_network))
        .route(
            "/networks/{name}/xml",
            get(get_network_xml).put(update_network_xml),
        )
        .route(
            "/networks/{name}/autostart/{enabled}",
            post(set_network_autostart),
        )
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Unified health summary for KubeVirt, automation, and Kubernetes tooling.

use axum::{routing::get, Json, Router};
use machina_core::{libvirt::automation, LibvirtManager, MachinaConfig};

use crate::automation_worker;
use crate::error::AppError;
use crate::k8s_kubeconfig;

async fn integrations_status() -> Result<Json<serde_json::Value>, AppError> {
    let cfg = MachinaConfig::load();

    let kubevirt = {
        let kv = &cfg.kubevirt;
        serde_json::json!({
            "exec_enabled": kv.exec_enabled,
            "default_namespace": kv.default_namespace,
            "default_storage_class": kv.default_storage_class,
            "routes": {
                "qcow2_bundle": "/api/v1/kubevirt/qcow2-bundle",
                "vm_bundle": "/api/v1/vms/{name}/kubevirt-bundle",
                "k8s_list": "/api/v1/k8s/kubevirt/virtualmachines",
            },
        })
    };

    let automation_cfg = {
        let rules = automation::load_alert_rules();
        let enabled_rules = rules.iter().filter(|r| r.enabled).count();
        let alerts = automation::load_alerts();
        let unacked = alerts.iter().filter(|a| !a.acknowledged).count();
        serde_json::json!({
            "worker_interval_secs": 60,
            "last_tick_unix": automation_worker::automation_last_tick_unix(),
            "alert_rules_total": rules.len(),
            "alert_rules_enabled": enabled_rules,
            "alerts_unacknowledged": unacked,
            "routes": {
                "alert_rules": "/api/v1/alert-rules",
                "alerts": "/api/v1/alerts",
                "schedules": "/api/v1/schedules",
            },
        })
    };

    let k8s = {
        let choice = k8s_kubeconfig::kubectl_kubeconfig_choice().await;
        serde_json::json!({
            "kubeconfig_auto_selected": choice.auto_selected_path,
            "kubectl_args_prefix": choice.prefix,
            "inventory_history_enabled": cfg.k8s_inventory_history.enabled,
            "routes": {
                "overview": "/api/v1/k8s/overview",
                "cluster_inventory": "/api/v1/k8s/cluster-inventory",
                "metrics": "/api/v1/k8s/metrics",
            },
        })
    };

    let run_as_user = {
        let r = &cfg.auth.run_as_user;
        let mode = serde_json::to_value(&r.mode).unwrap_or(serde_json::json!("disabled"));
        serde_json::json!({
            "enabled": r.wants_impersonation(),
            "mode": mode,
            "impersonation_active": r.impersonation_active(),
            "status_url": "/api/v1/auth/run-as-user",
        })
    };

    Ok(Json(serde_json::json!({
        "kubevirt": kubevirt,
        "automation": automation_cfg,
        "k8s": k8s,
        "run_as_user": run_as_user,
    })))
}

pub fn integrations_routes() -> Router<LibvirtManager> {
    Router::new().route("/integrations/status", get(integrations_status))
}

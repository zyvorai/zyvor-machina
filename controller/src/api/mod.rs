// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

mod ai;
mod alarms;
pub mod alerts;
pub mod apikeys;
mod applications;
mod atlas;
mod audit;
mod autopilot;
mod backup_targets;
mod backups;
mod baremetal;
mod blueprints;
mod chaos;
pub mod cert;
pub(crate) mod cloud;
mod cloud_init;
mod cluster;
mod content;
pub mod cpu_compat;
mod developer;
pub(crate) mod dist;
pub(crate) mod enrollment;
mod fleet_pki;
pub(crate) mod join_events;
mod enterprise_security;
mod error;
mod events;
mod fence;
mod flavors;
mod fleet;
mod fleet_automation;
mod guestkit;
mod ha;
mod health;
mod health_check;
mod host_cockpit;
mod host_os;
mod hosts;
mod ec2;
mod elastic_ips;
mod keypairs;
pub(crate) mod metric_stats;
mod kubevirt;
mod launchpad;
mod load_balancers;
mod k8s_clusters;
mod maintenance;
mod migration_waves;
mod marketplace;
mod mcp;
mod tags;
mod metrics;
mod migration_jobs;
mod network_canvas;
mod network_segments;
pub(crate) mod networking;
mod networks;
pub mod notification_channels;
mod notifications;
mod observability;
mod observability_middleware;
mod oidc;
mod operations;
mod placement;
mod preempt;
mod policy;
mod projects;
mod proxmox;
mod recommendations;
mod reports;
pub mod scheduled_jobs;
mod snapshots;
mod soc;
mod sprites;
mod sse;
pub mod stacks;
mod storage;
mod storage_tiers;
mod support;
mod update_check;
mod task_failures;
mod tasks;
mod templates;
mod topology;
mod transiva;
mod upgrade;
mod users;
mod vm_libvirt;
pub(crate) mod vm_network_policies;
mod vm_row;
mod vm_schedules;
mod time_travel;
mod vm_sleep;
pub(crate) mod vms;
mod vmware;
pub(crate) mod volumes;
pub mod watchdog;
mod webhooks;
mod zeus_firewall;
mod zeus_security;

use axum::middleware;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;

use crate::auth::auth_middleware;
use crate::console;
use crate::consolehub;
use crate::rate_limit::{rate_limit_middleware, RateLimiter};
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let rate_limiter = RateLimiter::from_env(state.config.jwt_secret.clone());
    let protected = Router::new()
        .merge(cloud::routes())
        .route(
            "/api/v1/enrollment/tokens",
            get(enrollment::list_enrollment_tokens).post(enrollment::create_enrollment_token),
        )
        .route(
            "/api/v1/hosts",
            get(hosts::list_hosts).post(hosts::create_host),
        )
        .route("/api/v1/hosts/sync-all", post(hosts::sync_all_hosts))
        .route(
            "/api/v1/hosts/{id}",
            get(hosts::get_host)
                .patch(hosts::patch_host)
                .delete(hosts::delete_host),
        )
        .route("/api/v1/hosts/{id}/detail", get(hosts::get_host_detail))
        .route("/api/v1/hosts/{id}/gpus", get(hosts::get_host_gpus))
        .route(
            "/api/v1/hosts/{id}/validate",
            get(hosts::validate_host).post(hosts::enqueue_validate_host),
        )
        .route("/api/v1/hosts/{id}/sync", post(hosts::sync_host))
        .route("/api/v1/hosts/{id}/lldp", get(hosts::host_lldp))
        .route(
            "/api/v1/hosts/{id}/linux/observability",
            get(host_os::host_linux_observability),
        )
        .route(
            "/api/v1/hosts/{id}/linux/network-diag",
            get(host_os::host_network_diagnostics),
        )
        .route(
            "/api/v1/hosts/{id}/linux/audit",
            get(host_os::host_linux_audit),
        )
        .route(
            "/api/v1/hosts/{id}/linux/filesystems",
            get(host_os::host_linux_filesystems),
        )
        .route(
            "/api/v1/hosts/{id}/linux/processes",
            get(host_os::host_linux_processes),
        )
        .route(
            "/api/v1/hosts/{id}/linux/package-upgrade",
            post(host_os::host_linux_package_upgrade),
        )
        .route(
            "/api/v1/hosts/{id}/linux/reboot",
            post(host_os::host_linux_reboot),
        )
        .route(
            "/api/v1/hosts/{id}/cockpit",
            get(host_cockpit::host_cockpit_inventory),
        )
        .route(
            "/api/v1/hosts/{id}/cockpit/actions",
            post(host_cockpit::host_cockpit_action),
        )
        .route("/api/v1/hosts/{id}/diagnose", post(host_os::diagnose_host))
        .route(
            "/api/v1/hosts/{id}/maintenance",
            post(hosts::host_maintenance),
        )
        .route("/api/v1/hosts/{id}/cordon", post(hosts::cordon_host))
        .route("/api/v1/vms", get(vms::list_vms).post(vms::create_vm))
        .route("/api/v1/sprites", get(sprites::list_fleet_sprites))
        .route("/api/v1/vms/prune-missing", post(vms::prune_missing_vms))
        .route(
            "/api/v1/vms/{id}/prune-inventory",
            post(vms::prune_vm_inventory_record),
        )
        .route(
            "/api/v1/enrollment/tokens/{token}/events",
            get(join_events::join_progress),
        )
        .route(
            "/api/v1/enrollment/tokens/{token}",
            delete(enrollment::revoke_enrollment_token),
        )
        .route(
            "/api/v1/hosts/{id}/fence",
            post(maintenance::fence_host_manual),
        )
        .route("/api/v1/vms/{id}", get(vms::get_vm).patch(vms::patch_vm))
        .route(
            "/api/v1/vms/{id}/labels",
            get(vm_network_policies::get_labels).put(vm_network_policies::put_labels),
        )
        .route(
            "/api/v1/vms/{id}/quarantine",
            post(vm_network_policies::quarantine).delete(vm_network_policies::quarantine_release),
        )
        .route(
            "/api/v1/vm-network-policies/quarantines",
            get(vm_network_policies::quarantines),
        )
        .route(
            "/api/v1/vm-network-policies/jit",
            get(vm_network_policies::jit_list).post(vm_network_policies::jit_request),
        )
        .route(
            "/api/v1/vm-network-policies/threat-feeds",
            get(vm_network_policies::threat_feeds),
        )
        .route(
            "/api/v1/vm-network-policies/threat-feeds/{name}",
            put(vm_network_policies::threat_feed_set)
                .delete(vm_network_policies::threat_feed_remove)
                .layer(axum::extract::DefaultBodyLimit::max(
                    machina_bpf::netpol::threat::MAX_FEED_BYTES,
                )),
        )
        .route(
            "/api/v1/vm-network-policies/threat-feeds/{name}/refresh",
            post(vm_network_policies::threat_feed_refresh),
        )
        .route(
            "/api/v1/vm-network-policies",
            get(vm_network_policies::list).post(vm_network_policies::apply),
        )
        .route(
            "/api/v1/vm-network-policies/validate",
            post(vm_network_policies::validate),
        )
        .route(
            "/api/v1/vm-network-policies/trace",
            post(vm_network_policies::trace),
        )
        .route(
            "/api/v1/vm-network-policies/endpoints",
            get(vm_network_policies::endpoints),
        )
        .route(
            "/api/v1/vm-network-policies/selectors",
            get(vm_network_policies::selectors),
        )
        .route(
            "/api/v1/vm-network-policies/status",
            get(vm_network_policies::status),
        )
        .route(
            "/api/v1/vm-network-policies/fqdn-cache",
            get(vm_network_policies::fqdn_cache),
        )
        .route(
            "/api/v1/vm-network-policies/auth",
            get(vm_network_policies::auth_table),
        )
        .route(
            "/api/v1/vm-network-policies/sync",
            post(vm_network_policies::sync_now),
        )
        .route(
            "/api/v1/vm-network-policies/learn",
            post(vm_network_policies::learn),
        )
        .route(
            "/api/v1/vm-network-policies/replay",
            post(vm_network_policies::replay),
        )
        .route(
            "/api/v1/vm-network-policies/projects",
            get(vm_network_policies::projects),
        )
        .route(
            "/api/v1/vm-network-policies/projects/{project}",
            put(vm_network_policies::project_set).delete(vm_network_policies::project_remove),
        )
        .route(
            "/api/v1/vm-network-policies/projects/{project}/preview",
            post(vm_network_policies::project_preview),
        )
        .route(
            "/api/v1/vm-network-policies/evidence",
            get(vm_network_policies::evidence),
        )
        .route(
            "/api/v1/vm-network-policies/evidence/archive",
            get(vm_network_policies::evidence_archive),
        )
        .route(
            "/api/v1/vm-network-policies/evidence/archive/{name}",
            get(vm_network_policies::evidence_archived),
        )
        .route(
            "/api/v1/vm-network-policies/egress-ips",
            get(vm_network_policies::egress_ips),
        )
        .route(
            "/api/v1/vm-network-policies/overlay",
            get(vm_network_policies::overlay).put(vm_network_policies::overlay_set),
        )
        .route(
            "/api/v1/vm-network-policies/draft",
            get(vm_network_policies::draft_pending).post(vm_network_policies::draft),
        )
        .route(
            "/api/v1/vm-network-policies/draft/propose",
            post(vm_network_policies::draft_propose),
        )
        .route(
            "/api/v1/flows/edges",
            get(vm_network_policies::flow_edges).delete(vm_network_policies::flow_edges_reset),
        )
        .route(
            "/api/v1/flows/alerts",
            get(vm_network_policies::flow_alerts),
        )
        .route(
            "/api/v1/vm-network-policies/{name}",
            get(vm_network_policies::get_one).delete(vm_network_policies::delete_one),
        )
        .route(
            "/api/v1/vm-network-policies/{name}/enabled",
            put(vm_network_policies::set_enabled),
        )
        .route("/api/v1/flows", get(vm_network_policies::flows))
        .route(
            "/api/v1/flows/stream",
            get(vm_network_policies::flow_stream),
        )
        .route("/api/v1/vms/{id}/disks", get(vms::list_vm_disks))
        .route("/api/v1/vms/{id}/disks/attach", post(vms::attach_vm_disk))
        .route(
            "/api/v1/vms/{id}/disks/detach/{target}",
            post(vms::detach_vm_disk),
        )
        .route(
            "/api/v1/vms/{id}/disks/resize/{target}",
            post(vms::resize_vm_disk),
        )
        .route("/api/v1/vms/{id}/nics", get(vms::list_vm_nics))
        .route("/api/v1/vms/{id}/nics/attach", post(vms::attach_vm_nic))
        .route(
            "/api/v1/vms/{id}/nics/detach/{mac}",
            post(vms::detach_vm_nic),
        )
        .route("/api/v1/vms/{id}/autostart", post(vms::set_vm_autostart))
        .route("/api/v1/vms/{id}/vcpus", post(vms::set_vm_vcpus))
        .route("/api/v1/vms/{id}/memory", post(vms::set_vm_memory))
        .route("/api/v1/vms/{id}/change-type", post(vms::change_vm_type))
        .route(
            "/api/v1/vms/{id}/libvirt-details",
            get(vms::get_vm_libvirt_details),
        )
        .route(
            "/api/v1/vms/{id}/hardware-summary",
            get(vms::get_vm_hardware_summary),
        )
        .route(
            "/api/v1/vms/{id}/hardware-compat",
            get(vms::get_vm_hardware_compat),
        )
        .route("/api/v1/vms/{id}/domain-caps", get(vms::get_vm_domain_caps))
        .route(
            "/api/v1/vms/{id}/pending-config",
            get(vms::get_vm_pending_config),
        )
        .route(
            "/api/v1/vms/pending-config/batch",
            post(vms::batch_vm_parity_summary),
        )
        .route("/api/v1/vms/guest-ips/batch", post(vms::batch_vm_guest_ips))
        .route("/api/v1/vms/{id}/viewer.vv", get(vms::get_vm_viewer_vv))
        .route("/api/v1/vms/{id}/qemu-logs", get(vms::get_vm_qemu_logs))
        .route("/api/v1/vms/{id}/rename", post(vms::rename_platform_vm))
        .route("/api/v1/vms/{id}/nmi", post(vms::inject_vm_nmi))
        .route(
            "/api/v1/vms/{id}/graphics/spice-to-vnc",
            post(vms::convert_vm_spice_to_vnc),
        )
        .route("/api/v1/vms/{id}/graphics/add", post(vms::add_vm_graphics))
        .route(
            "/api/v1/vms/{id}/graphics/remove",
            post(vms::remove_vm_graphics),
        )
        .route("/api/v1/vms/batch/power", post(vms::batch_vm_power))
        .route("/api/v1/vms/batch/snapshots", post(vms::batch_vm_snapshot))
        .route("/api/v1/vms/batch/delete", post(vms::batch_vm_delete))
        .route(
            "/api/v1/vms/{id}/libvirt",
            get(vm_libvirt::query_vm_libvirt).post(vm_libvirt::invoke_vm_libvirt),
        )
        .route(
            "/api/v1/hosts/{id}/libvirt",
            get(vm_libvirt::query_host_libvirt).post(vm_libvirt::invoke_host_libvirt),
        )
        .route("/api/v1/vms/{id}/metrics", get(vms::get_vm_metrics))
        .route("/api/v1/vms/{id}/adopt", post(vms::adopt_vm))
        .route(
            "/api/v1/vms/{id}/schedules",
            get(vm_schedules::list_vm_schedules).post(vm_schedules::create_vm_schedule),
        )
        .route(
            "/api/v1/vms/{id}/schedules/{schedule_id}",
            delete(vm_schedules::delete_vm_schedule),
        )
        .route(
            "/api/v1/vms/{id}/health-check",
            post(health_check::vm_health_check),
        )
        .route(
            "/api/v1/vms/{id}/guest/health",
            get(host_os::vm_guest_health),
        )
        .route(
            "/api/v1/vms/{id}/guest/observability",
            get(host_os::vm_guest_observability),
        )
        .route(
            "/api/v1/vms/{id}/guest/services",
            get(host_os::vm_guest_services),
        )
        .route(
            "/api/v1/vms/{id}/guest/services/{unit}/{action}",
            post(host_os::vm_guest_service_action),
        )
        .route(
            "/api/v1/vms/{id}/guest/network",
            get(host_os::vm_guest_network_get).post(host_os::vm_guest_network_apply),
        )
        .route(
            "/api/v1/vms/{id}/guest/sync-time",
            post(host_os::vm_guest_sync_time),
        )
        .route(
            "/api/v1/vms/{id}/guest/fstrim",
            post(host_os::vm_guest_fstrim),
        )
        .route(
            "/api/v1/vms/{id}/guest/fs-freeze-status",
            get(host_os::vm_guest_fs_freeze_status),
        )
        .route(
            "/api/v1/vms/{id}/guest/ai-insights",
            post(host_os::vm_guest_ai_insights),
        )
        .route("/api/v1/vms/{id}/diagnose", post(host_os::diagnose_vm))
        .route("/api/v1/vms/{id}/doctor", get(ai::vm_doctor))
        .route(
            "/api/v1/ai/settings",
            get(ai::get_settings).patch(ai::patch_settings),
        )
        .route("/api/v1/ai/spotlight", post(ai::spotlight))
        .route("/api/v1/ai/jarvis/landing", get(ai::jarvis_landing))
        .route("/api/v1/ai/copilot/chat", post(ai::copilot_chat))
        .route("/api/v1/ai/copilot/stream", post(ai::copilot_stream))
        .route("/api/v1/ai/fleet/guest-query", post(ai::fleet_guest_query))
        .route(
            "/api/v1/ai/migration/readiness-report",
            post(ai::migration_readiness_report),
        )
        .route("/api/v1/ai/explain", post(ai::explain))
        .route("/api/v1/ai/runbook", post(ai::runbook))
        .route(
            "/api/v1/ai/blueprints/generate",
            post(ai::generate_blueprint),
        )
        .route("/api/v1/ai/cost", get(ai::cost_guardian))
        .route("/api/v1/ai/cost/export.csv", get(ai::cost_export_csv))
        .route("/api/v1/ai/capacity", get(ai::capacity_planner))
        .route(
            "/api/v1/ai/capacity/export.csv",
            get(ai::capacity_export_csv),
        )
        .route("/api/v1/ai/fleet/summary", get(ai::fleet_summary))
        .route("/api/v1/ai/fleet/local", get(ai::fleet_local))
        .route("/api/v1/ai/twin/graph", get(ai::twin_graph))
        .route("/api/v1/ai/twin/impact", post(ai::twin_impact))
        .route("/api/v1/ai/twin/simulate", post(ai::twin_simulate))
        .route("/api/v1/ai/graph", get(ai::infra_graph))
        .route("/api/v1/ai/graph/path", post(ai::infra_graph_path))
        .route("/api/v1/ai/graph/query", post(ai::infra_graph_query))
        .route(
            "/api/v1/ai/graph/object/{kind}/{id}",
            get(ai::infra_graph_object),
        )
        .route("/api/v1/ai/graph/at/{timestamp}", get(ai::infra_graph_at))
        .route("/api/v1/ai/timeline/replay", get(ai::timeline_replay))
        .route(
            "/api/v1/ai/incidents/analyze",
            get(ai::analyze_incident).post(ai::analyze_incident_post),
        )
        .route("/api/v1/ai/interference/noisy-neighbors", get(ai::noisy_neighbors))
        .route(
            "/api/v1/ai/incidents/blackbox",
            get(ai::analyze_blackbox_incident),
        )
        .route("/api/v1/ai/incidents/active", get(ai::incidents_active))
        .route("/api/v1/ai/incidents/{id}/room", get(ai::incident_room))
        .route("/api/v1/ai/incidents/{id}/ack", post(ai::incident_ack))
        .route("/api/v1/ai/troubleshoot", post(ai::troubleshoot_vm))
        .route("/api/v1/ai/predictions", get(ai::predictions_unified))
        .route("/api/v1/ai/rightsizing/report", get(ai::rightsizing_report))
        .route("/api/v1/ai/nl-ops", post(ai::nl_ops))
        .route(
            "/api/v1/ai/memory/changes-before",
            get(ai::memory_changes_before),
        )
        .route("/api/v1/ai/vm-builder", post(ai::vm_builder))
        .route(
            "/api/v1/ai/intent/environment",
            post(ai::intent_environment),
        )
        .route(
            "/api/v1/ai/intent/environment/execute",
            post(ai::intent_environment_execute),
        )
        .route("/api/v1/ai/sre/forecast", get(ai::sre_forecast))
        .route("/api/v1/ai/sre/remediate", get(ai::sre_remediate))
        .route(
            "/api/v1/ai/compliance/remediate",
            get(ai::compliance_remediate),
        )
        .route("/api/v1/ai/zyra/summary", get(ai::zyra_summary))
        .route(
            "/api/v1/ai/fleet/power/optimize",
            get(ai::fleet_power_optimize),
        )
        .route("/api/v1/ai/fleet/heatmap", get(ai::fleet_heatmap))
        .route(
            "/api/v1/ai/fleet/rebalance/propose",
            get(ai::fleet_rebalance_propose),
        )
        .route(
            "/api/v1/ai/fleet/rebalance/execute",
            post(ai::fleet_rebalance_execute),
        )
        .route("/api/v1/ai/cost/attribution", get(ai::cost_attribution))
        .route(
            "/api/v1/ai/compliance/frameworks",
            get(ai::compliance_frameworks),
        )
        .route("/api/v1/ai/security/graph", get(ai::security_graph))
        .route(
            "/api/v1/ai/security/attack-path",
            post(ai::security_attack_path),
        )
        .route("/api/v1/ai/knowledge/search", post(ai::knowledge_search))
        .route("/api/v1/ai/services/graph", get(ai::service_graph))
        .route(
            "/api/v1/ai/memory/incidents",
            get(ai::infrastructure_memory),
        )
        .route("/api/v1/ai/mission/stack", post(ai::mission_stack))
        .route(
            "/api/v1/ai/mission/stack/execute",
            post(ai::mission_stack_execute),
        )
        .route(
            "/api/v1/ai/cost/attribution/export.csv",
            get(ai::cost_attribution_export_csv),
        )
        .route(
            "/api/v1/ai/fleet/gpu-placement",
            get(ai::fleet_gpu_placement),
        )
        .route(
            "/api/v1/ai/knowledge/diagnose",
            post(ai::knowledge_diagnose),
        )
        .route("/api/v1/ai/services/impact", post(ai::service_impact))
        .route("/api/v1/ai/memory/similar", get(ai::memory_similar))
        .route("/api/v1/ai/remediate/hub", get(ai::remediate_hub))
        .route("/api/v1/ai/knowledge/runbook", post(ai::knowledge_runbook))
        .route("/api/v1/ai/cost/budget", get(ai::cost_budget))
        .route(
            "/api/v1/ai/mission/stack/status",
            get(ai::mission_stack_status),
        )
        .route(
            "/api/v1/ai/providers",
            get(ai::list_ai_providers).post(ai::create_ai_provider),
        )
        .route(
            "/api/v1/ai/providers/{id}",
            patch(ai::patch_ai_provider).delete(ai::delete_ai_provider),
        )
        .route(
            "/api/v1/ai/providers/{id}/models",
            get(ai::list_ai_provider_models),
        )
        .route("/api/v1/ai/providers/{id}/test", post(ai::test_ai_provider))
        .route("/api/v1/ai/routing/rules", get(ai::list_routing_rules))
        .route(
            "/api/v1/ai/routing/rules/{task_class}",
            patch(ai::patch_routing_rule),
        )
        .route("/api/v1/ai/agents", get(ai::list_zyra_agents))
        .route("/api/v1/ai/zyra/chat", post(ai::zyra_chat))
        .route(
            "/api/v1/ai/prompts",
            get(ai::list_ai_prompts).post(ai::create_ai_prompt),
        )
        .route(
            "/api/v1/ai/prompts/{id}",
            patch(ai::patch_ai_prompt).delete(ai::delete_ai_prompt),
        )
        .route(
            "/api/v1/ai/memory/settings",
            get(ai::get_memory_settings).patch(ai::patch_memory_settings),
        )
        .route("/api/v1/ai/memory", delete(ai::purge_memory))
        .route("/api/v1/ai/actions/hub", get(ai::zyra_approval_hub))
        .route("/api/v1/ai/actions", post(ai::create_zyra_action))
        .route(
            "/api/v1/ai/actions/{id}/execute",
            post(ai::execute_zyra_action),
        )
        .route(
            "/api/v1/ai/actions/{id}/reject",
            post(ai::reject_zyra_action),
        )
        .route("/api/v1/ai/actions/history", get(ai::zyra_action_history))
        .route("/api/v1/mcp", post(mcp::mcp_post))
        .route("/api/v1/tags", get(tags::list_tags))
        .route(
            "/api/v1/tags/{resource_type}/{id}",
            get(tags::get_tags).put(tags::put_tags).delete(tags::delete_tags),
        )
        .route("/api/v1/ids/{ec2_id}", get(tags::resolve_ec2_id))
        .route("/api/v1/ai/trust", get(ai::zyra_trust_list))
        .route("/api/v1/ai/trust/{action_type}", put(ai::zyra_trust_set))
        .route("/api/v1/ai/agent/run", post(ai::run_zyra_agent))
        .route("/api/v1/ai/agent/stream", post(ai::run_zyra_agent_stream))
        .route(
            "/api/v1/ai/actions/{id}/verify",
            post(ai::verify_zyra_action),
        )
        .route("/api/v1/ai/actions/{id}/undo", post(ai::undo_zyra_action))
        .route(
            "/api/v1/ai/marketplace/agents",
            get(ai::list_agent_marketplace),
        )
        .route(
            "/api/v1/ai/marketplace/agents/{slug}/install",
            post(ai::install_agent_marketplace),
        )
        .route(
            "/api/v1/ai/marketplace/agents/{slug}/uninstall",
            post(ai::uninstall_agent_marketplace),
        )
        .route(
            "/api/v1/ai/enterprise/zyra",
            get(ai::zyra_enterprise_overview).patch(ai::patch_zyra_enterprise_overview),
        )
        .route("/api/v1/ai/zyra/plan", post(ai::zyra_autonomous_plan))
        .route("/api/v1/ai/zyra/execute", post(ai::zyra_autonomous_execute))
        .route(
            "/api/v1/baremetal/servers",
            get(baremetal::list_servers).post(baremetal::register_server),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/power",
            post(baremetal::server_power),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/lifecycle",
            post(baremetal::server_lifecycle),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/commission",
            post(baremetal::server_commission),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/inventory",
            get(baremetal::server_inventory),
        )
        .route("/api/v1/baremetal/boot/dnsmasq.conf", get(baremetal::boot_dnsmasq))
        .route("/api/v1/baremetal/allocate", post(baremetal::allocate))
        .route("/api/v1/baremetal/discover", post(baremetal::discover))
        .route("/api/v1/baremetal/import", post(baremetal::import_servers))
        .route(
            "/api/v1/baremetal/servers/{id}/bmc-info",
            get(baremetal::get_bmc_info),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/bmc-info/refresh",
            post(baremetal::refresh_bmc_info),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/bmc",
            axum::routing::put(baremetal::set_bmc),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/sol-token",
            post(baremetal::sol_token),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/virtual-media",
            post(baremetal::server_virtual_media),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/power-state",
            get(baremetal::server_power_state),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/boot-device",
            post(baremetal::server_boot_device),
        )
        .route(
            "/api/v1/baremetal/profiles",
            get(baremetal::list_profiles).put(baremetal::put_profile),
        )
        .route(
            "/api/v1/baremetal/profiles/{name}",
            axum::routing::delete(baremetal::delete_profile),
        )
        .route("/api/v1/baremetal/provision", post(baremetal::provision))
        .route(
            "/api/v1/baremetal/images",
            get(baremetal::list_images).post(baremetal::import_image),
        )
        .route(
            "/api/v1/baremetal/images/{name}",
            axum::routing::delete(baremetal::delete_image),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/deploy",
            post(baremetal::server_deploy),
        )
        .route("/api/v1/baremetal/zones", get(baremetal::zones))
        .route(
            "/api/v1/baremetal/servers/{id}/release",
            post(baremetal::release),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/zone",
            post(baremetal::set_zone),
        )
        .route(
            "/api/v1/baremetal/servers/{id}/provision",
            get(baremetal::server_provision),
        )
        .route(
            "/api/v1/baremetal/capacity/plan",
            post(baremetal::capacity_plan),
        )
        .route("/api/v1/ai/security", get(ai::security_sentinel))
        .route("/api/v1/ai/policy/export", get(ai::policy_export))
        .route("/api/v1/ai/performance-autopilot", get(ai::performance_autopilot))
        .route("/api/v1/ai/autopilot/propose", get(ai::autopilot_propose))
        .route("/api/v1/ai/autopilot/execute", post(ai::autopilot_execute))
        .route("/api/v1/ai/autopilot/run", post(ai::autopilot_run))
        .route("/api/v1/ai/autopilot/history", get(ai::autopilot_history))
        .route("/api/v1/ai/compliance", get(ai::compliance_report))
        .route(
            "/api/v1/ai/compliance/export",
            get(ai::compliance_export_html),
        )
        .route(
            "/api/v1/ai/compliance/export.pdf",
            get(ai::compliance_export_pdf),
        )
        .route("/api/v1/ai/terminal/suggest", post(ai::terminal_suggest))
        .route("/api/v1/ai/network/explain", post(ai::network_explain))
        .route("/api/v1/migrations/advisor", get(ai::migration_advisor))
        .route("/api/v1/migrations/adaptive/decision", post(vms::adaptive_migration_decision))
        .route("/api/v1/guestkit/status", get(guestkit::guestkit_status))
        .route("/api/v1/guestkit/doctor", post(guestkit::guestkit_doctor))
        .route(
            "/api/v1/guestkit/migrate-plan",
            post(guestkit::guestkit_migrate_plan),
        )
        .route(
            "/api/v1/guestkit/vms/{id}/doctor",
            get(guestkit::guestkit_vm_doctor),
        )
        .route(
            "/api/v1/guestkit/vms/{id}/migrate-plan",
            get(guestkit::guestkit_vm_migrate_plan),
        )
        .route("/api/v1/guestkit/jobs", post(guestkit::guestkit_submit_job))
        .route(
            "/api/v1/guestkit/jobs/{id}",
            get(guestkit::guestkit_job_status),
        )
        .route(
            "/api/v1/launchpad/catalog",
            get(launchpad::launchpad_catalog),
        )
        // Atlas storage control plane
        .route("/api/v1/atlas/status", get(atlas::atlas_status))
        .route("/api/v1/atlas/backends", get(atlas::atlas_backends))
        .route("/api/v1/atlas/clusters", get(atlas::atlas_clusters))
        .route("/api/v1/atlas/pools", get(atlas::atlas_pools))
        .route("/api/v1/atlas/policies", get(atlas::atlas_policies))
        .route("/api/v1/atlas/forecast", get(atlas::atlas_forecast))
        .route("/api/v1/atlas/alerts", get(atlas::atlas_alerts))
        .route(
            "/api/v1/atlas/metrics/summary",
            get(atlas::atlas_metrics_summary),
        )
        .route(
            "/api/v1/atlas/volumes",
            get(atlas::atlas_volumes).post(atlas::atlas_create_volume),
        )
        .route(
            "/api/v1/atlas/volumes/{id}",
            get(atlas::atlas_volume).delete(atlas::atlas_delete_volume),
        )
        .route(
            "/api/v1/atlas/volumes/{id}/expand",
            post(atlas::atlas_expand_volume),
        )
        .route(
            "/api/v1/atlas/volumes/{id}/snapshots",
            post(atlas::atlas_snapshot_volume),
        )
        .route("/api/v1/atlas/snapshots", get(atlas::atlas_snapshots))
        .route(
            "/api/v1/atlas/snapshots/{id}/clone",
            post(atlas::atlas_clone_snapshot),
        )
        .route(
            "/api/v1/atlas/snapshots/{id}/restore",
            post(atlas::atlas_restore_snapshot),
        )
        .route(
            "/api/v1/atlas/snapshots/{id}",
            delete(atlas::atlas_delete_snapshot),
        )
        .route(
            "/api/v1/atlas/buckets",
            get(atlas::atlas_buckets).post(atlas::atlas_create_bucket),
        )
        .route(
            "/api/v1/atlas/backups",
            get(atlas::atlas_backups).post(atlas::atlas_backup_volume),
        )
        .route(
            "/api/v1/atlas/backups/{id}",
            delete(atlas::atlas_delete_backup),
        )
        .route(
            "/api/v1/atlas/restore-jobs",
            post(atlas::atlas_restore_backup),
        )
        .route("/api/v1/atlas/jobs", get(atlas::atlas_jobs))
        .route("/api/v1/atlas/jobs/{id}", get(atlas::atlas_job))
        // VM ↔ Atlas volume orchestration
        .route(
            "/api/v1/atlas/vms/{id}/volumes",
            get(atlas::atlas_vm_volumes).post(atlas::atlas_provision_vm_volume),
        )
        .route(
            "/api/v1/atlas/vms/{id}/snapshot",
            post(atlas::atlas_snapshot_vm),
        )
        .route(
            "/api/v1/atlas/vms/{id}/backup",
            post(atlas::atlas_backup_vm),
        )
        .route("/api/v1/zeus-firewall/status", get(zeus_firewall::status))
        .route(
            "/api/v1/zeus-firewall/overview",
            get(zeus_firewall::overview),
        )
        .route(
            "/api/v1/zeus-firewall/profiles",
            get(zeus_firewall::list_profiles),
        )
        .route(
            "/api/v1/zeus-firewall/policies",
            get(zeus_firewall::list_policies).post(zeus_firewall::create_policy),
        )
        .route(
            "/api/v1/zeus-firewall/temporary-rules",
            post(zeus_firewall::create_temporary_rule),
        )
        .route(
            "/api/v1/zeus-firewall/simulate",
            post(zeus_firewall::simulate),
        )
        .route(
            "/api/v1/zeus-firewall/approvals",
            get(zeus_firewall::list_approvals).post(zeus_firewall::request_risky_change),
        )
        .route(
            "/api/v1/zeus-firewall/approvals/{id}/approve",
            post(zeus_firewall::approve_change),
        )
        .route(
            "/api/v1/zeus-firewall/approvals/{id}/reject",
            post(zeus_firewall::reject_change),
        )
        .route(
            "/api/v1/zeus-firewall/policies/gitops/export",
            get(zeus_firewall::export_gitops),
        )
        .route(
            "/api/v1/zeus-firewall/policies/gitops/sync",
            post(zeus_firewall::sync_gitops),
        )
        .route(
            "/api/v1/zeus-firewall/k8s/status",
            get(zeus_firewall::k8s_status),
        )
        .route(
            "/api/v1/zeus-firewall/k8s/plan",
            post(zeus_firewall::k8s_plan),
        )
        .route(
            "/api/v1/zeus-firewall/k8s/apply",
            post(zeus_firewall::k8s_apply),
        )
        .route(
            "/api/v1/zeus-firewall/baremetal/overview",
            get(zeus_firewall::baremetal_overview),
        )
        .route(
            "/api/v1/zeus-firewall/baremetal/{id}/scan",
            post(zeus_firewall::baremetal_scan),
        )
        .route(
            "/api/v1/zeus-firewall/baremetal/{id}/temporary",
            post(zeus_firewall::baremetal_temporary),
        )
        .route(
            "/api/v1/zeus-firewall/finops/exposure",
            get(zeus_firewall::finops_exposure),
        )
        .route(
            "/api/v1/zeus-firewall/finops/exposure/export.csv",
            get(zeus_firewall::finops_exposure_export_csv),
        )
        .route(
            "/api/v1/zeus-firewall/cloud/overview",
            get(zeus_firewall::cloud_overview),
        )
        .route(
            "/api/v1/zeus-firewall/vms/{id}/guest-ports",
            get(zeus_firewall::vm_guest_ports),
        )
        .route(
            "/api/v1/zeus-firewall/connectivity",
            post(zeus_firewall::connectivity_matrix),
        )
        .route(
            "/api/v1/zeus-firewall/compliance/{kind}/export.pdf",
            get(zeus_firewall::compliance_export_pdf),
        )
        .route(
            "/api/v1/zeus-firewall/anomalies",
            get(zeus_firewall::anomalies),
        )
        .route(
            "/api/v1/zeus-firewall/compliance/{kind}",
            get(zeus_firewall::compliance_report),
        )
        .route(
            "/api/v1/zeus-firewall/siem/export",
            get(zeus_firewall::siem_export),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}",
            get(zeus_firewall::get_target),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/ports",
            get(zeus_firewall::get_ports),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/services",
            get(zeus_firewall::get_services),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/score",
            get(zeus_firewall::get_score),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/plan",
            post(zeus_firewall::plan_target),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/apply",
            post(zeus_firewall::apply_target),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/timeline",
            get(zeus_firewall::get_timeline),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/drift",
            get(zeus_firewall::detect_drift),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/activity",
            get(zeus_firewall::get_activity),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/lockdown",
            post(zeus_firewall::lockdown),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/profile",
            post(zeus_firewall::apply_profile),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/checkpoints",
            get(zeus_firewall::list_checkpoints),
        )
        .route(
            "/api/v1/zeus-firewall/targets/{id}/rollback",
            post(zeus_firewall::rollback),
        )
        .route(
            "/api/v1/ai/firewall/explain",
            post(zeus_firewall::ai_explain),
        )
        .route(
            "/api/v1/ai/firewall/secure-plan",
            post(zeus_firewall::ai_secure_plan),
        )
        .route(
            "/api/v1/zeus-firewall/multisite/overview",
            get(zeus_firewall::multisite_overview),
        )
        .route(
            "/api/v1/zeus-firewall/multisite/export",
            get(zeus_firewall::multisite_export),
        )
        .route(
            "/api/v1/zeus-firewall/multisite/drift",
            get(zeus_firewall::multisite_drift),
        )
        .route(
            "/api/v1/zeus-firewall/multisite/connectivity",
            get(zeus_firewall::multisite_connectivity),
        )
        .route(
            "/api/v1/zeus-firewall/multisite/sync",
            post(zeus_firewall::multisite_sync),
        )
        .route(
            "/api/v1/zeus-firewall/multisite/timeline",
            get(zeus_firewall::multisite_timeline),
        )
        .route(
            "/api/v1/zeus-firewall/multisite/dr-templates",
            get(zeus_firewall::multisite_dr_templates),
        )
        .route(
            "/api/v1/zeus-firewall/operator/plan",
            get(zeus_firewall::operator_plan),
        )
        .route(
            "/api/v1/zeus-firewall/operator/execute",
            post(zeus_firewall::operator_execute),
        )
        .route(
            "/api/v1/zeus-firewall/operator/execute-batch",
            post(zeus_firewall::operator_execute_batch),
        )
        .route(
            "/api/v1/zeus-firewall/operator/thresholds",
            get(zeus_firewall::operator_thresholds),
        )
        .route("/api/v1/zeus-security/status", get(zeus_security::status))
        .route(
            "/api/v1/zeus-security/fleet/threat",
            get(zeus_security::fleet_threat),
        )
        .route("/api/v1/zeus-security/sensors", get(zeus_security::sensors))
        .route(
            "/api/v1/zeus-security/asset-inventory",
            get(zeus_security::asset_inventory),
        )
        .route(
            "/api/v1/zeus-security/fleet/timeline",
            get(zeus_security::fleet_timeline),
        )
        .route(
            "/api/v1/zeus-security/correlations",
            get(zeus_security::correlations),
        )
        .route("/api/v1/zeus-security/l7", get(zeus_security::fleet_l7))
        .route(
            "/api/v1/zeus-security/accounting",
            get(zeus_security::fleet_accounting),
        )
        .route(
            "/api/v1/zeus-security/native-dataplane",
            get(zeus_security::fleet_native_dataplane),
        )
        .route(
            "/api/v1/zeus-security/tls/fingerprints",
            get(zeus_security::fleet_tls_fingerprints),
        )
        .route(
            "/api/v1/zeus-security/icmp-errors",
            get(zeus_security::fleet_icmp_errors),
        )
        .route(
            "/api/v1/zeus-security/rtnl-events",
            get(zeus_security::fleet_rtnl_events),
        )
        .route(
            "/api/v1/zeus-security/vm-intel",
            get(zeus_security::fleet_vm_intel),
        )
        .route(
            "/api/v1/zeus-security/guard-events",
            get(zeus_security::fleet_guard_events),
        )
        .route(
            "/api/v1/zeus-security/fabric/health",
            get(zeus_security::fabric_health),
        )
        .route(
            "/api/v1/zeus-security/hunt/queries",
            get(zeus_security::hunt_queries),
        )
        .route(
            "/api/v1/zeus-security/hunt/run/{query_id}",
            post(zeus_security::run_hunt_query),
        )
        .route(
            "/api/v1/zeus-security/alerts/sync",
            post(zeus_security::sync_alerts),
        )
        .route(
            "/api/v1/zeus-security/graph",
            get(zeus_security::security_graph),
        )
        .route("/api/v1/zeus-security/search", post(zeus_security::search))
        .route(
            "/api/v1/zeus-security/hosts/{id}/summary",
            get(zeus_security::host_summary),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/processes",
            get(zeus_security::host_processes),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/connections",
            get(zeus_security::host_connections),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/dns",
            get(zeus_security::host_dns),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/files",
            get(zeus_security::host_files),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/ports",
            get(zeus_security::host_ports),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/containers",
            get(zeus_security::host_containers),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/timeline",
            get(zeus_security::host_timeline),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/process-graph",
            get(zeus_security::host_process_graph),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/bpf",
            post(zeus_security::host_bpf_call),
        )
        .route(
            "/api/v1/zeus-security/enforcement/status",
            get(zeus_security::enforcement_status),
        )
        .route(
            "/api/v1/zeus-security/enforcement/attach",
            post(zeus_security::attach_enforcement),
        )
        .route(
            "/api/v1/zeus-security/enforcement/sync",
            post(zeus_security::sync_enforcement),
        )
        .route(
            "/api/v1/zeus-security/enforcement/detach",
            post(zeus_security::detach_enforcement),
        )
        .route(
            "/api/v1/zeus-security/enforcement/policies",
            get(zeus_security::enforcement_policies).post(zeus_security::create_enforcement_policy),
        )
        .route(
            "/api/v1/zeus-security/enforcement/policies/{id}/document",
            get(zeus_security::enforcement_policy_document),
        )
        .route(
            "/api/v1/zeus-security/enforcement/policies/{id}/apply",
            post(zeus_security::apply_enforcement_policy),
        )
        .route(
            "/api/v1/zeus-security/enforcement/policies/{id}",
            patch(zeus_security::patch_enforcement_policy)
                .delete(zeus_security::delete_enforcement_policy),
        )
        .route(
            "/api/v1/zeus-security/fleet/sensors",
            get(zeus_security::fleet_sensors),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/enforcement",
            get(zeus_security::host_enforcement),
        )
        .route(
            "/api/v1/zeus-security/hosts/{id}/fabric-status",
            get(zeus_security::host_fabric_status),
        )
        .route(
            "/api/v1/ai/security/explain-event",
            post(zeus_security::explain_event),
        )
        .route(
            "/api/v1/ai/security/attack-reconstruct",
            post(zeus_security::attack_reconstruct),
        )
        .route(
            "/api/v1/ai/security/nl-search",
            post(zeus_security::nl_search),
        )
        .route(
            "/api/v1/ai/security/hunt-summary",
            post(zeus_security::hunt_summary),
        )
        .route("/api/v1/soc/overview", get(soc::overview))
        .route("/api/v1/soc/events", get(soc::list_events))
        .route("/api/v1/soc/alerts", get(soc::list_alerts))
        .route(
            "/api/v1/soc/alerts/{id}",
            get(soc::get_alert)
                .patch(soc::patch_alert)
                .delete(soc::delete_alert),
        )
        .route(
            "/api/v1/soc/rules",
            get(soc::list_rules).post(soc::create_rule),
        )
        .route(
            "/api/v1/soc/rules/{id}",
            patch(soc::patch_rule).delete(soc::delete_rule),
        )
        .route("/api/v1/soc/rules/{id}/test", post(soc::test_rule))
        .route("/api/v1/soc/asm/summary", get(soc::asm_summary))
        .route("/api/v1/soc/integrations", get(soc::list_integrations))
        .route(
            "/api/v1/soc/integrations/{integration_type}",
            patch(soc::patch_integration),
        )
        .route(
            "/api/v1/soc/integrations/{integration_type}/test",
            post(soc::test_integration),
        )
        .route(
            "/api/v1/soc/integrations/splunk",
            get(soc::get_splunk_integration).put(soc::put_splunk_integration),
        )
        .route(
            "/api/v1/soc/integrations/splunk/test",
            post(soc::test_splunk_integration),
        )
        .route(
            "/api/v1/soc/forward/replay",
            post(soc::forward_replay_handler),
        )
        .route("/api/v1/soc/ingest/run", post(soc::run_ingest_cycle))
        .route(
            "/api/v1/soc/playbooks",
            get(soc::list_playbooks).post(soc::create_playbook),
        )
        .route(
            "/api/v1/soc/playbooks/{id}",
            get(soc::get_playbook)
                .patch(soc::patch_playbook)
                .delete(soc::delete_playbook),
        )
        .route("/api/v1/soc/playbook-runs", get(soc::list_playbook_runs))
        .route(
            "/api/v1/soc/settings",
            get(soc::get_soc_settings).patch(soc::patch_soc_settings),
        )
        .route(
            "/api/v1/hosts/{id}/health-check",
            post(health_check::host_health_check),
        )
        .route(
            "/api/v1/recommendations",
            get(recommendations::list_recommendations),
        )
        .route(
            "/api/v1/applications",
            get(applications::list_applications).post(applications::create_application),
        )
        .route(
            "/api/v1/applications/{id}",
            get(applications::get_application).delete(applications::delete_application),
        )
        .route(
            "/api/v1/applications/{id}/actions",
            post(applications::run_application_action),
        )
        .route("/api/v1/topology", get(topology::cluster_topology))
        .route(
            "/api/v1/network-canvas",
            get(network_canvas::network_canvas),
        )
        .route("/api/v1/vms/{id}/topology", get(topology::vm_topology))
        .route("/api/v1/vms/{id}/spec", get(vms::get_vm_spec))
        .route(
            "/api/v1/vms/{id}/domain-xml",
            get(vms::get_vm_domain_xml).put(vm_libvirt::put_vm_domain_xml),
        )
        .route(
            "/api/v1/vms/{id}/port-forwards",
            get(vms::list_vm_port_forwards).post(vms::create_vm_port_forward),
        )
        .route(
            "/api/v1/vms/{id}/port-forwards/delete",
            post(vms::delete_vm_port_forward),
        )
        .route(
            "/api/v1/vms/{id}/port-forward-templates",
            get(vms::list_vm_port_forward_templates).post(vms::upsert_vm_port_forward_template),
        )
        .route("/api/v1/vms/{id}/start", post(vms::start_vm))
        .route("/api/v1/vms/{id}/stop", post(vms::stop_vm))
        .route("/api/v1/vms/{id}/shutdown", post(vms::shutdown_vm))
        .route("/api/v1/vms/{id}/pause", post(vms::pause_vm))
        .route("/api/v1/vms/{id}/resume", post(vms::resume_vm))
        .route("/api/v1/vms/{id}/sleep", post(vms::sleep_vm))
        .route("/api/v1/vms/{id}/wake", post(vms::wake_vm))
        .route(
            "/api/v1/vms/{id}/sleep-policy",
            get(vms::get_vm_sleep_policy).put(vms::set_vm_sleep_policy),
        )
        .route("/api/v1/sleep/summary", get(vm_sleep::summary))
        .route("/api/v1/sleep/policies", get(vm_sleep::list_project_policies))
        .route(
            "/api/v1/sleep/policies/{project}",
            put(vm_sleep::put_project_policy).delete(vm_sleep::delete_project_policy),
        )
        .route(
            "/api/v1/vms/{id}/restore-points",
            get(time_travel::get_time_travel).post(time_travel::create_restore_point),
        )
        .route(
            "/api/v1/vms/{id}/restore-points/policy",
            put(time_travel::set_policy),
        )
        .route(
            "/api/v1/vms/{id}/restore-points/{point_id}/rewind",
            post(time_travel::rewind),
        )
        .route("/api/v1/vms/{id}/fork", post(time_travel::fork))
        .route("/api/v1/vms/{id}/fork/detach", post(time_travel::detach))
        .route("/api/v1/vms/{id}/reboot", post(vms::reboot_vm))
        .route("/api/v1/vms/{id}/reset", post(vms::reset_vm))
        .route("/api/v1/vms/{id}/install", post(vms::install_vm))
        .route("/api/v1/vms/{id}/delete", post(vms::delete_vm))
        .route(
            "/api/v1/vms/{id}/guest-tools/install",
            post(vms::install_guest_tools),
        )
        .route("/api/v1/migrations/adaptive/step", post(vms::adaptive_migration_step))
        .route(
            "/api/v1/vms/{id}/migrate/precheck",
            post(vms::migrate_precheck),
        )
        .route(
            "/api/v1/vms/{id}/migration-oracle",
            post(vms::migration_oracle),
        )
        .route("/api/v1/vms/{id}/migrate", post(vms::migrate_vm))
        .route(
            "/api/v1/vms/{id}/fluxvm/recover",
            post(vms::recover_fluxvm_vm),
        )
        .route(
            "/api/v1/vms/{id}/ha",
            get(ha::get_vm_ha_policy).post(ha::set_vm_ha_policy),
        )
        .route("/api/v1/vms/from-template", post(vms::create_from_template))
        .route("/api/v1/vms/run-instances", post(vms::run_instances))
        .route("/api/v1/vms/from-iso", post(vms::create_from_iso))
        .route(
            "/api/v1/vms/from-virt-install",
            post(vms::create_from_virt_install),
        )
        .route("/api/v1/vms/{id}/clone", post(vms::clone_vm))
        .route(
            "/api/v1/vms/{id}/publish-template",
            post(vms::publish_vm_template),
        )
        .route("/api/v1/vms/{id}/retire", post(vms::retire_vm))
        .route("/api/v1/vms/{id}/disk/export", post(vms::export_vm_disk))
        .route("/api/v1/vms/{id}/export", get(developer::export_vm_iac))
        .route(
            "/api/v1/vms/{id}/export.zip",
            get(developer::export_vm_iac_zip),
        )
        .route("/api/v1/vms/{id}/console", get(console::vm_console))
        .route("/api/v1/vms/{id}/ws-token", post(console::issue_ws_token))
        .merge(consolehub::api_routes())
        .route(
            "/api/v1/vms/{id}/timeline",
            get(snapshots::list_vm_timeline),
        )
        .route(
            "/api/v1/vms/{id}/snapshots",
            get(snapshots::list_vm_snapshots).post(snapshots::create_vm_snapshot),
        )
        .route(
            "/api/v1/vms/{id}/snapshots/{name}",
            delete(snapshots::delete_vm_snapshot),
        )
        .route(
            "/api/v1/vms/{id}/snapshots/{name}/revert",
            post(snapshots::revert_vm_snapshot),
        )
        .route(
            "/api/v1/vms/{id}/snapshots/{name}/clone",
            post(snapshots::clone_vm_snapshot),
        )
        .route(
            "/api/v1/vms/{id}/backups",
            get(backups::list_vm_backups).post(backups::create_vm_backup),
        )
        .route(
            "/api/v1/backup-targets",
            get(backup_targets::list_backup_targets).post(backup_targets::create_backup_target),
        )
        .route(
            "/api/v1/backup-targets/{id}",
            delete(backup_targets::delete_backup_target),
        )
        .route(
            "/api/v1/backups/timeline",
            get(backups::list_backup_timeline),
        )
        .route(
            "/api/v1/backup-schedules",
            get(backups::list_backup_schedules).post(backups::create_backup_schedule),
        )
        .route(
            "/api/v1/backup-schedules/{id}",
            delete(backups::delete_backup_schedule),
        )
        .route(
            "/api/v1/vms/{id}/backups/{backup_id}/verify",
            post(backups::verify_vm_backup),
        )
        .route(
            "/api/v1/vms/{id}/backups/{backup_id}/restore",
            post(backups::restore_vm_backup),
        )
        .route(
            "/api/v1/vms/{id}/migrations",
            get(migration_jobs::list_vm_migration_jobs),
        )
        .route(
            "/api/v1/templates",
            get(templates::list_templates).post(templates::create_template),
        )
        .route(
            "/api/v1/templates/marketplace",
            get(templates::list_marketplace_templates),
        )
        .route(
            "/api/v1/marketplace/plugins",
            get(marketplace::plugins_overview),
        )
        .route(
            "/api/v1/marketplace/plugins/{slug}/install",
            post(marketplace::install_plugin),
        )
        .route(
            "/api/v1/marketplace/plugins/{slug}/uninstall",
            post(marketplace::uninstall_plugin),
        )
        .route(
            "/api/v1/marketplace/plugins",
            post(marketplace::publish_plugin),
        )
        .route(
            "/api/v1/templates/missing-images",
            get(templates::list_missing_template_images),
        )
        .route(
            "/api/v1/templates/prefetch-missing",
            axum::routing::post(templates::prefetch_missing_template_images),
        )
        .route("/api/v1/templates/seed", post(templates::seed_templates))
        .route(
            "/api/v1/templates/sync-git",
            post(templates::sync_git_templates),
        )
        .route(
            "/api/v1/templates/sync-git/webhook",
            post(templates::sync_git_templates_webhook),
        )
        .route(
            "/api/v1/templates/{name}/{version}/visibility",
            put(templates::set_template_visibility),
        )
        .route(
            "/api/v1/templates/{name}/{version}/shares",
            get(templates::list_template_shares),
        )
        .route(
            "/api/v1/templates/{name}/{version}/shares/{project}",
            put(templates::share_template).delete(templates::unshare_template),
        )
        .route(
            "/api/v1/templates/{name}/{version}/approval",
            axum::routing::patch(templates::approve_template),
        )
        .route(
            "/api/v1/templates/{name}/{version}",
            get(templates::get_template).delete(templates::delete_template),
        )
        .route(
            "/api/v1/templates/{name}/{version}/readiness",
            get(templates::get_template_readiness),
        )
        .route(
            "/api/v1/storage/pools",
            get(storage::list_storage_pools).post(storage::create_storage_pool),
        )
        .route(
            "/api/v1/storage/pools/discover",
            post(storage::discover_storage_pools),
        )
        .route(
            "/api/v1/storage/pools/{id}",
            get(storage::get_storage_pool)
                .patch(storage::patch_storage_pool)
                .delete(storage::delete_storage_pool),
        )
        .route(
            "/api/v1/storage/pools/{id}/activate",
            post(storage::activate_storage_pool),
        )
        .route(
            "/api/v1/storage/pools/{id}/deactivate",
            post(storage::deactivate_storage_pool),
        )
        .route(
            "/api/v1/storage/pools/{id}/refresh",
            post(storage::refresh_storage_pool),
        )
        .route(
            "/api/v1/storage/pools/live",
            get(storage::live_storage_pools),
        )
        .route(
            "/api/v1/storage/pools/{id}/volumes",
            get(storage::list_storage_pool_volumes).post(storage::create_storage_pool_volume),
        )
        .route(
            "/api/v1/storage/pools/{id}/volumes/{vol_name}",
            delete(storage::delete_storage_pool_volume),
        )
        .route(
            "/api/v1/storage/tiers/overview",
            get(storage_tiers::tiers_overview),
        )
        .route(
            "/api/v1/storage/pools/{pool_id}/tier/{tier_id}",
            post(storage_tiers::bind_pool_tier),
        )
        .route(
            "/api/v1/storage/backup-sla",
            get(storage_tiers::backup_sla_overview),
        )
        .route(
            "/api/v1/storage/pools/{id}/backup-sla",
            post(storage_tiers::upsert_backup_sla),
        )
        .route(
            "/api/v1/storage/pools/{id}/snapshot-policy",
            get(storage_tiers::pool_snapshot_policy),
        )
        .route(
            "/api/v1/networks",
            get(networks::list_networks).post(networks::create_network),
        )
        .route(
            "/api/v1/networks/discover",
            post(networks::discover_networks),
        )
        .route(
            "/api/v1/networks/{id}",
            get(networks::get_network)
                .patch(networks::patch_network)
                .delete(networks::delete_network),
        )
        .route(
            "/api/v1/networks/{id}/activate",
            post(networks::activate_network),
        )
        .route(
            "/api/v1/networks/{id}/deactivate",
            post(networks::deactivate_network),
        )
        .route("/api/v1/networks/live", get(networks::live_networks))
        .route(
            "/api/v1/network/segments/overview",
            get(network_segments::segments_overview),
        )
        .route(
            "/api/v1/network/segments",
            post(network_segments::create_segment),
        )
        .route(
            "/api/v1/network/segments/gitops/export",
            get(network_segments::gitops_export),
        )
        .route(
            "/api/v1/network/ipam/pools",
            get(network_segments::ipam_pools),
        )
        .route(
            "/api/v1/network/segments/{id}/connectivity",
            post(network_segments::segment_connectivity),
        )
        .route(
            "/api/v1/network/segments/{id}/ipam/allocate",
            post(network_segments::ipam_allocate),
        )
        .route(
            "/api/v1/network/segments/{id}/emergency-unlock",
            post(network_segments::emergency_unlock),
        )
        .route(
            "/api/v1/network/segments/{id}/bind/{network_id}",
            post(network_segments::bind_network_to_segment),
        )
        .route("/api/v1/failure-domains", get(placement::failure_domains))
        .route(
            "/api/v1/k8s-clusters",
            get(k8s_clusters::list).post(k8s_clusters::create),
        )
        .route(
            "/api/v1/k8s-clusters/{id}",
            get(k8s_clusters::get).delete(k8s_clusters::delete),
        )
        .route("/api/v1/k8s-clusters/{id}/nodes", post(k8s_clusters::add_node))
        .route(
            "/api/v1/k8s-clusters/{id}/nodes/{node_id}",
            delete(k8s_clusters::remove_node),
        )
        .route("/api/v1/k8s-clusters/{id}/upgrade", post(k8s_clusters::upgrade))
        .route("/api/v1/k8s-clusters/{id}/kubeconfig", get(k8s_clusters::kubeconfig))
        .route(
            "/api/v1/placement/recommendations",
            get(placement::list_recommendations),
        )
        .route(
            "/api/v1/placement/refresh",
            post(placement::refresh_recommendations),
        )
        .route(
            "/api/v1/cluster",
            get(cluster::get_cluster).patch(cluster::patch_cluster),
        )
        .route("/api/v1/cluster/leadership", get(cluster::get_leadership))
        .route(
            "/api/v1/cluster/settings",
            get(cluster::get_settings).patch(cluster::patch_settings),
        )
        .route(
            "/api/v1/enterprise/security/overview",
            get(enterprise_security::overview),
        )
        .route(
            "/api/v1/enterprise/air-gap/bundles",
            get(enterprise_security::list_air_gap_bundles)
                .post(enterprise_security::create_air_gap_bundle),
        )
        .route(
            "/api/v1/enterprise/air-gap/bundles/{id}",
            get(enterprise_security::get_air_gap_bundle)
                .delete(enterprise_security::delete_air_gap_bundle),
        )
        .route(
            "/api/v1/enterprise/fips/matrix",
            get(enterprise_security::fips_matrix),
        )
        .route(
            "/api/v1/enterprise/tenants/overview",
            get(enterprise_security::tenant_isolation_overview),
        )
        .route(
            "/api/v1/enterprise/tenants/policies/{project}",
            post(enterprise_security::upsert_tenant_policy),
        )
        .route("/api/v1/ha/status", get(ha::get_ha_status))
        .route(
            "/api/v1/migrations",
            get(migration_jobs::list_migration_jobs),
        )
        .route(
            "/api/v1/migration-waves",
            get(migration_waves::list).post(migration_waves::create),
        )
        .route(
            "/api/v1/migration-waves/{id}",
            get(migration_waves::get).delete(migration_waves::delete),
        )
        .route(
            "/api/v1/migration-waves/{id}/advance",
            post(migration_waves::advance),
        )
        .route(
            "/api/v1/migration-waves/{id}/cancel",
            post(migration_waves::cancel),
        )
        .route(
            "/api/v1/migration-waves/{id}/items/{item_id}/cutover",
            post(migration_waves::cutover),
        )
        .route("/api/v1/transiva/status", get(transiva::status))
        .route("/api/v1/transiva/providers", get(transiva::providers))
        .route("/api/v1/transiva/migrations/preview", post(transiva::preview))
        .route("/api/v1/transiva/migrations/submit", post(transiva::submit))
        .route("/api/v1/transiva/migrations/batch", post(transiva::batch))
        .route("/api/v1/fence/events", get(fence::list_fence_events))
        .route("/api/v1/audit", get(audit::list_audit_logs))
        .route("/api/v1/tasks/failures", get(task_failures::summary))
        .route(
            "/api/v1/tasks/failures/acknowledge",
            post(task_failures::acknowledge),
        )
        .route("/api/v1/tasks", get(tasks::list_tasks))
        .route("/api/v1/tasks/{id}", get(tasks::get_task))
        .route("/api/v1/tasks/{id}/cancel", post(tasks::cancel_task))
        .route("/api/v1/tasks/{id}/retry", post(tasks::retry_task))
        .route("/api/v1/events", get(events::list_events))
        .route("/api/v1/events/stream", get(sse::stream_events))
        .route(
            "/api/v1/content/images",
            get(content::list_content_images).post(content::create_content_image),
        )
        .route(
            "/api/v1/content/images/{id}",
            delete(content::delete_content_image),
        )
        .route(
            "/api/v1/content/images/{id}/approve",
            post(content::approve_content_image),
        )
        .route(
            "/api/v1/content/images/{id}/reject",
            post(content::reject_content_image),
        )
        .route(
            "/api/v1/users",
            get(users::list_users).post(users::create_user),
        )
        .route(
            "/api/v1/users/prune-invalid",
            post(users::prune_invalid_users),
        )
        .route("/api/v1/users/me", get(users::me))
        .route(
            "/api/v1/users/{id}",
            patch(users::patch_user).delete(users::delete_user),
        )
        .route(
            "/api/v1/api-keys",
            get(apikeys::list_api_keys).post(apikeys::create_api_key),
        )
        .route("/api/v1/api-keys/{id}", delete(apikeys::delete_api_key))
        .route(
            "/api/v1/api-keys/{id}/rotate",
            post(apikeys::rotate_api_key),
        )
        .route(
            "/api/v1/alert-rules",
            get(alerts::list_alert_rules).post(alerts::create_alert_rule),
        )
        .route(
            "/api/v1/alert-rules/{id}",
            patch(alerts::patch_alert_rule).delete(alerts::delete_alert_rule),
        )
        .route(
            "/api/v1/notification-channels",
            get(notification_channels::list_channels).post(notification_channels::create_channel),
        )
        .route(
            "/api/v1/notification-channels/{id}",
            delete(notification_channels::delete_channel),
        )
        .route(
            "/api/v1/notification-channels/{id}/test",
            post(notification_channels::test_channel),
        )
        .route("/api/v1/cert-status", get(cert::get_cert_status))
        .route(
            "/api/v1/scheduled-jobs",
            get(scheduled_jobs::list_scheduled_jobs).post(scheduled_jobs::create_scheduled_job),
        )
        .route(
            "/api/v1/scheduled-jobs/{id}",
            patch(scheduled_jobs::patch_scheduled_job).delete(scheduled_jobs::delete_scheduled_job),
        )
        .route(
            "/api/v1/vms/{id}/watchdog",
            get(watchdog::get_vm_watchdog).post(watchdog::set_vm_watchdog),
        )
        .route(
            "/api/v1/webhooks",
            get(webhooks::list_webhooks).post(webhooks::create_webhook),
        )
        .route("/api/v1/webhooks/{id}", delete(webhooks::delete_webhook))
        .route(
            "/api/v1/webhooks/{id}/toggle",
            post(webhooks::toggle_webhook),
        )
        .route(
            "/api/v1/webhook-deliveries",
            get(webhooks::list_webhook_deliveries),
        )
        .route(
            "/api/v1/webhook-deliveries/purge",
            post(webhooks::purge_webhook_deliveries),
        )
        .route(
            "/api/v1/webhook-deliveries/{id}/retry",
            post(webhooks::retry_webhook_delivery),
        )
        .route("/api/v1/projects", get(projects::list_projects))
        .route(
            "/api/v1/project-registry",
            get(projects::list_project_registry).post(projects::create_project),
        )
        .route(
            "/api/v1/project-registry/{id}/members",
            get(projects::list_project_members).post(projects::add_project_member),
        )
        .route(
            "/api/v1/project-registry/{id}/members/{user_id}",
            delete(projects::remove_project_member),
        )
        .route(
            "/api/v1/volumes",
            get(volumes::list_volumes).post(volumes::create_volume),
        )
        .route(
            "/api/v1/volumes/{id}",
            get(volumes::get_volume).delete(volumes::delete_volume),
        )
        .route("/api/v1/volumes/{id}/attach", post(volumes::attach_volume))
        .route("/api/v1/volumes/{id}/detach", post(volumes::detach_volume))
        .route("/api/v1/volumes/{id}/extend", post(volumes::extend_volume))
        .route(
            "/api/v1/volumes/{id}/iotune",
            put(volumes::set_volume_iotune),
        )
        .route(
            "/api/v1/volumes/{id}/delete-on-termination",
            put(volumes::set_volume_delete_on_termination),
        )
        .route(
            "/api/v1/volumes/{id}/snapshots",
            get(volumes::list_volume_snapshots).post(volumes::create_volume_snapshot),
        )
        .route(
            "/api/v1/volume-snapshots",
            get(volumes::list_all_volume_snapshots),
        )
        .route(
            "/api/v1/volume-snapshots/{id}/create-volume",
            post(volumes::create_volume_from_snapshot),
        )
        .route(
            "/api/v1/volume-snapshots/{id}",
            delete(volumes::delete_volume_snapshot),
        )
        .route(
            "/api/v1/security-groups",
            get(networking::list_security_groups).post(networking::create_security_group),
        )
        .route(
            "/api/v1/security-groups/{id}",
            get(networking::get_security_group).delete(networking::delete_security_group),
        )
        .route(
            "/api/v1/security-groups/{id}/enforce-preview",
            get(networking::preview_security_group_enforcement),
        )
        .route(
            "/api/v1/security-groups/{id}/mode",
            put(networking::set_security_group_mode),
        )
        .route(
            "/api/v1/vms/{id}/security-groups",
            get(networking::list_instance_security_groups),
        )
        .route(
            "/api/v1/vms/{id}/security-groups/{sg_id}",
            put(networking::attach_instance_security_group)
                .delete(networking::detach_instance_security_group),
        )
        .route(
            "/api/v1/security-groups/{id}/rules",
            get(networking::list_security_group_rules).post(networking::create_security_group_rule),
        )
        .route(
            "/api/v1/security-group-rules/{id}",
            delete(networking::delete_security_group_rule),
        )
        .route(
            "/api/v1/ports",
            get(networking::list_ports).post(networking::create_port),
        )
        .route(
            "/api/v1/ports/{id}",
            get(networking::get_port).delete(networking::delete_port),
        )
        .route(
            "/api/v1/stacks",
            get(stacks::list_stacks).post(stacks::create_stack),
        )
        .route("/api/v1/stacks/draft", post(stacks::draft_stack))
        .route(
            "/api/v1/chaos/experiments",
            get(chaos::list_experiments).post(chaos::create_experiment),
        )
        .route(
            "/api/v1/chaos/experiments/{id}",
            get(chaos::get_experiment)
                .put(chaos::update_experiment)
                .delete(chaos::delete_experiment),
        )
        .route("/api/v1/chaos/experiments/{id}/run", post(chaos::run_experiment))
        .route("/api/v1/chaos/runs", get(chaos::list_runs))
        .route("/api/v1/chaos/runs/{id}", get(chaos::get_run))
        .route("/api/v1/chaos/runs/{id}/abort", post(chaos::abort_run))
        .route("/api/v1/chaos/faults", get(chaos::faults))
        .route("/api/v1/preemption", get(preempt::overview))
        .route("/api/v1/preemption/settings", put(preempt::update_settings))
        .route("/api/v1/vms/{id}/preemptible", put(preempt::set_vm))
        .route("/api/v1/rightsizing", get(autopilot::rightsizing))
        .route("/api/v1/rightsizing/propose", post(autopilot::propose_resize))
        .route("/api/v1/drs/consolidation", get(autopilot::consolidation))
        .route(
            "/api/v1/drs/consolidation/propose",
            post(autopilot::propose_consolidation),
        )
        .route("/api/v1/stacks/plan", post(stacks::plan_stack))
        .route("/api/v1/stacks/propose", post(stacks::propose_stack))
        .route(
            "/api/v1/stacks/{id}",
            get(stacks::get_stack)
                .put(stacks::update_stack)
                .delete(stacks::delete_stack),
        )
        .route("/api/v1/stacks/{id}/drift", get(stacks::get_drift))
        .route("/api/v1/stacks/{id}/converge", post(stacks::converge_stack))
        .route("/api/v1/stacks/{id}/auto-heal", put(stacks::set_auto_heal))
        .route(
            "/api/v1/flavors",
            get(flavors::list_flavors).post(flavors::create_flavor),
        )
        .route(
            "/api/v1/flavors/{id}",
            get(flavors::get_flavor).delete(flavors::delete_flavor),
        )
        .route(
            "/api/v1/keypairs",
            get(keypairs::list_keypairs).post(keypairs::create_keypair),
        )
        .route(
            "/api/v1/keypairs/{id}",
            get(keypairs::get_keypair).delete(keypairs::delete_keypair),
        )
        .route(
            "/api/v1/load-balancers",
            get(load_balancers::list_load_balancers).post(load_balancers::create_load_balancer),
        )
        .route(
            "/api/v1/load-balancers/{id}",
            get(load_balancers::get_load_balancer).delete(load_balancers::delete_load_balancer),
        )
        .route(
            "/api/v1/load-balancers/{id}/health-check",
            put(load_balancers::set_health_check),
        )
        .route(
            "/api/v1/load-balancers/{id}/members",
            get(load_balancers::list_lb_members).post(load_balancers::add_lb_member),
        )
        .route(
            "/api/v1/load-balancers/{id}/members/{member_id}",
            axum::routing::patch(load_balancers::patch_lb_member)
                .delete(load_balancers::delete_lb_member),
        )
        .route("/api/v1/policy/rules", get(policy::list_policy_rules))
        .route(
            "/api/v1/policy/quotas",
            get(policy::list_project_quotas).post(policy::upsert_project_quota),
        )
        .route("/api/v1/support/bundle", get(support::support_bundle))
        .route("/api/v1/support/bundle.zip", get(support::support_bundle_zip))
        .route("/api/v1/system/update-check", get(update_check::update_check))
        .route("/api/v1/upgrade/matrix", get(upgrade::upgrade_matrix))
        .route(
            "/api/v1/hosts/{id}/upgrade",
            post(upgrade::upgrade_host_agent),
        )
        .route("/api/v1/reports/capacity", get(reports::capacity_report))
        .route("/api/v1/reports/finops", get(reports::finops_report))
        .route("/api/v1/operations/overview", get(operations::overview))
        .route(
            "/api/v1/operations/runbooks",
            get(operations::list_runbooks),
        )
        .route(
            "/api/v1/operations/runbooks/{incident}/execute",
            post(operations::execute_runbook),
        )
        .route(
            "/api/v1/operations/executions",
            get(operations::list_executions),
        )
        .route(
            "/api/v1/operations/showback",
            get(operations::showback_overview),
        )
        .route("/api/v1/fleet/desktop", get(fleet::desktop_overview))
        .route("/api/v1/fleet/linux-health", get(fleet::linux_health))
        .route("/api/v1/fleet/activity", get(fleet::activity_overview))
        .route("/api/v1/fleet/backups", get(fleet::backup_overview))
        .route("/api/v1/fleet/finder", get(fleet::finder_overview))
        .route("/api/v1/fleet/network", get(fleet::network_overview))
        .route("/api/v1/fleet/storage", get(fleet::storage_overview))
        .route("/api/v1/fleet/console", get(fleet::console_overview))
        .route("/api/v1/fleet/updates", get(fleet::updates_overview))
        .route("/api/v1/fleet/keychain", get(fleet::keychain_overview))
        .route("/api/v1/fleet/users", get(fleet::users_overview))
        .route("/api/v1/fleet/shortcuts", get(fleet::shortcuts_overview))
        .route("/api/v1/fleet/spaces", get(fleet::spaces_overview))
        .route("/api/v1/fleet/general", get(fleet::general_overview))
        .route("/api/v1/fleet/mission", get(fleet::mission_overview))
        .route("/api/v1/fleet/gpu", get(fleet::gpu_overview))
        .route(
            "/api/v1/fleet/maintenance-mission",
            get(fleet::maintenance_mission_overview),
        )
        .route("/api/v1/fleet/dna", get(fleet::dna_overview))
        .route(
            "/api/v1/hosts/{id}/linux/updates",
            get(host_os::host_linux_package_updates),
        )
        .route("/api/v1/ai/fleet/diagnose", post(fleet::fleet_diagnose))
        .route("/api/v1/developer/overview", get(developer::overview))
        .route(
            "/api/v1/developer/terraform/schema",
            get(developer::terraform_schema),
        )
        .route(
            "/api/v1/fleet/snapshot-schedules",
            get(fleet_automation::list_fleet_snapshot_schedules)
                .post(fleet_automation::create_fleet_snapshot_schedule),
        )
        .route(
            "/api/v1/fleet/snapshot-schedules/{id}",
            delete(fleet_automation::delete_fleet_snapshot_schedule),
        )
        .route("/api/v1/kubevirt/sync", post(kubevirt::sync_inventory))
        .route("/api/v1/proxmox/sync", post(proxmox::sync_inventory))
        .route("/api/v1/vmware/sync", post(vmware::sync_inventory))
        .route(
            "/api/v1/observability/overview",
            get(observability::overview),
        )
        .route(
            "/api/v1/observability/traces",
            get(observability::list_traces),
        )
        .route(
            "/api/v1/blueprints",
            get(blueprints::list_blueprints).post(blueprints::create_blueprint),
        )
        .route(
            "/api/v1/blueprints/{id}/run",
            post(blueprints::run_blueprint),
        )
        .route(
            "/api/v1/blueprints/{id}",
            get(blueprints::get_blueprint).delete(blueprints::delete_blueprint),
        )
        .route(
            "/api/v1/metrics/prometheus",
            get(metrics::prometheus_metrics),
        )
        .route(
            "/api/v1/maintenance/schedules",
            get(maintenance::list_schedules).post(maintenance::create_schedule),
        )
        .route(
            "/api/v1/maintenance/schedules/{id}",
            get(maintenance::get_schedule).delete(maintenance::delete_schedule),
        )
        .route(
            "/api/v1/notifications",
            get(notifications::list_notifications),
        )
        .route(
            "/api/v1/notifications/{id}/deliver",
            post(notifications::mark_notification_delivered),
        )
        .route(
            "/api/v1/auth/oidc",
            get(oidc::get_oidc_settings).patch(oidc::patch_oidc_settings),
        )
        .route(
            "/api/v1/cpu-compat",
            get(cpu_compat::get_cpu_compat_matrix).patch(cpu_compat::patch_cpu_compat_matrix),
        )
        .route("/api/v1/elastic-ip-pools", get(elastic_ips::list_pools).post(elastic_ips::create_pool))
        .route("/api/v1/elastic-ip-pools/{id}", delete(elastic_ips::delete_pool))
        .route("/api/v1/elastic-ips", get(elastic_ips::list).post(elastic_ips::allocate))
        .route("/api/v1/elastic-ips/{id}", delete(elastic_ips::release))
        .route("/api/v1/elastic-ips/{id}/associate", post(elastic_ips::associate))
        .route("/api/v1/elastic-ips/{id}/disassociate", post(elastic_ips::disassociate))
        .route("/api/v1/metrics/statistics", get(metric_stats::statistics))
        .route("/api/v1/alarms", get(alarms::list_alarms).post(alarms::create_alarm))
        .route("/api/v1/alarms/{id}", get(alarms::get_alarm).delete(alarms::delete_alarm))
        .route("/api/v1/alarms/{id}/enabled", put(alarms::set_alarm_enabled))
        .route(
            "/api/v1/ec2/access-keys",
            get(ec2::list_access_keys).post(ec2::create_access_key),
        )
        .route(
            "/api/v1/ec2/access-keys/{id}",
            delete(ec2::revoke_access_key),
        )
        .route(
            "/api/v1/cloud-init/validate",
            post(cloud_init::validate_cloud_init),
        )
        // Project-scoped access (opt-in: MACHINA_PROJECT_RBAC=audit|enforce). Innermost, so it runs after auth.
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            crate::project_rbac::middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            observability_middleware::trace_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // OIDC login/redirect/callback are unauthenticated by nature (that's the point of the
    // flow) and each round-trips to the external IdP (discovery + token exchange, optionally
    // userinfo) — expensive work a caller can trigger without any credential. They belong in
    // the same rate-limited-public bucket as password login and host join.
    let rate_limited_public = Router::new()
        .route("/api/v1/hosts/join", post(hosts::join_host))
        .route(
            "/api/v1/baremetal/commissioning/{id}/report",
            post(baremetal::commissioning_report),
        )
        .route(
            "/api/v1/baremetal/boot/deploy.sh",
            get(baremetal::boot_deploy_script),
        )
        .route(
            "/api/v1/baremetal/boot/plan/{id}/{token}",
            get(baremetal::deploy_plan),
        )
        .route(
            "/api/v1/baremetal/deploy/{id}/complete",
            post(baremetal::deploy_complete),
        )
        .route("/api/v1/baremetal/boot/ipxe", get(baremetal::boot_ipxe))
        .route(
            "/api/v1/baremetal/boot/install/{id}/{token}/{name}",
            get(baremetal::install_document),
        )
        .route(
            "/api/v1/baremetal/boot/commission.sh",
            get(baremetal::boot_commission_script),
        )
        .route("/api/v1/auth/login", post(crate::auth::login))
        .route("/ec2", post(ec2::query))
        .route("/ec2/", post(ec2::query))
        // The same handler: the service in the request's SigV4 scope picks the action table.
        .route("/monitoring", post(ec2::query))
        .route("/monitoring/", post(ec2::query))
        .route("/autoscaling", post(ec2::query))
        .route("/autoscaling/", post(ec2::query))
        .route("/elbv2", post(ec2::query))
        .route("/elbv2/", post(ec2::query))
        .route("/api/v1/auth/oidc/login", get(oidc::oidc_login))
        .route("/api/v1/auth/oidc/redirect", get(oidc::oidc_login_redirect))
        .route("/api/v1/auth/oidc/callback", get(oidc::oidc_callback))
        .route_layer(middleware::from_fn_with_state(
            rate_limiter,
            rate_limit_middleware,
        ));

    Router::new()
        .route("/api/v1/health", get(health::health))
        .route("/api/v1/health/ready", get(health::ready))
        .route("/api/v1/openapi.json", get(health::openapi))
        .route("/install.sh", get(enrollment::install_script))
        .route("/api/v1/pki/ca", get(fleet_pki::ca))
        .route("/api/v1/pki/ca.pem", get(fleet_pki::ca_pem))
        .route("/dist/images/{name}", get(dist::serve_image))
        .route("/dist/{name}", get(dist::serve))
        .merge(rate_limited_public)
        .merge(console::ws_routes())
        .merge(protected)
        .with_state(state)
}

/// What a joining node may reach over the controller's network (TLS) listener, and nothing else.
pub fn enrollment_router(state: AppState) -> Router {
    let rate_limiter = RateLimiter::from_env(state.config.jwt_secret.clone());
    let joins = Router::new()
        .route("/api/v1/hosts/join", post(hosts::join_host))
        .route(
            "/api/v1/baremetal/commissioning/{id}/report",
            post(baremetal::commissioning_report),
        )
        .route(
            "/api/v1/baremetal/boot/plan/{id}/{token}",
            get(baremetal::deploy_plan),
        )
        .route(
            "/api/v1/baremetal/deploy/{id}/complete",
            post(baremetal::deploy_complete),
        )
        .route(
            "/api/v1/baremetal/boot/deploy.sh",
            get(baremetal::boot_deploy_script),
        )
        .route(
            "/api/v1/baremetal/boot/commission.sh",
            get(baremetal::boot_commission_script),
        )
        .route_layer(middleware::from_fn_with_state(
            rate_limiter,
            rate_limit_middleware,
        ));
    Router::new()
        .route("/api/v1/health", get(health::health))
        .route("/api/v1/pki/ca", get(fleet_pki::ca))
        .route("/api/v1/pki/ca.pem", get(fleet_pki::ca_pem))
        .route("/dist/images/{name}", get(dist::serve_image))
        .route("/dist/{name}", get(dist::serve))
        .route("/install.sh", get(enrollment::install_script))
        .merge(joins)
        .with_state(state)
}

pub use error::ApiError;

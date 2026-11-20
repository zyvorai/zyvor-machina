// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use machina_core::bpf_probe;
use machina_core::host_linux_obs;
use machina_core::libvirt::automation;
use machina_core::libvirt::extras::get_host_stats;
use machina_core::libvirt::node;
use machina_core::obs_counters;
use machina_core::{
    LibvirtManager, MachinaConfig, VmBlockDeviceMetrics, VmInfo, VmMetrics, VmNetDeviceMetrics,
};

use crate::daemon_stats::DaemonStats;
use crate::http_metrics::HttpMetrics;

static LAST_VM_CPU: Mutex<Option<HashMap<String, (Instant, u64)>>> = Mutex::new(None);

fn add_gauge(output: &mut String, name: &str, help: &str, value: impl Display) {
    output.push_str(&format!(
        "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}\n"
    ));
}

fn escape_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn add_labeled(output: &mut String, name: &str, labels: &str, value: impl Display) {
    output.push_str(&format!("{name}{{{labels}}} {value}\n"));
}

fn vm_prom_label(m: &VmMetrics) -> String {
    match &m.libvirt_connection {
        Some(c) => format!("{c}/{}", m.name),
        None => m.name.clone(),
    }
}

fn vm_info_label(vm: &VmInfo) -> String {
    match &vm.libvirt_connection {
        Some(c) => format!("{c}/{}", vm.name),
        None => vm.name.clone(),
    }
}

fn vm_state_code(state: &str) -> u32 {
    match state.to_ascii_lowercase().as_str() {
        "running" => 1,
        "paused" => 2,
        "shutting down" => 3,
        "shutoff" => 0,
        "crashed" => 4,
        "blocked" => 5,
        "suspended" => 6,
        _ => 99,
    }
}

fn vm_cpu_util_pct(m: &VmMetrics) -> f64 {
    let mut guard = LAST_VM_CPU.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    let pct = if let Some((t0, cpu0)) = map.get(&m.name) {
        let dt = now.duration_since(*t0).as_secs_f64();
        if dt > 0.1 {
            let dcpu = m.cpu_time_ns.saturating_sub(*cpu0) as f64 / 1_000_000_000.0;
            (dcpu / dt / m.vcpus.max(1) as f64 * 100.0).min(100.0)
        } else {
            0.0
        }
    } else {
        0.0
    };
    // Prune entries for VMs not seen in 10 min so the map stays bounded to live VMs
    // rather than accumulating a slot for every distinct VM name ever scraped.
    map.retain(|_, (t, _)| now.saturating_duration_since(*t) < std::time::Duration::from_secs(600));
    map.insert(m.name.clone(), (now, m.cpu_time_ns));
    pct
}

fn add_vm_metric(
    output: &mut String,
    name: &str,
    help: &str,
    metric_type: &str,
    vm_metrics: &[VmMetrics],
    extract: impl Fn(&VmMetrics) -> String,
) {
    output.push_str(&format!(
        "# HELP {name} {help}\n# TYPE {name} {metric_type}\n"
    ));
    for m in vm_metrics {
        let label = escape_label(&vm_prom_label(m));
        add_labeled(output, name, &format!("vm=\"{label}\""), extract(m));
    }
}

fn add_vm_disk_metrics(output: &mut String, vm_metrics: &[VmMetrics]) {
    output.push_str(
        "# HELP machina_vm_disk_read_bytes_total Per-disk read bytes\n\
         # TYPE machina_vm_disk_read_bytes_total counter\n",
    );
    output.push_str(
        "# HELP machina_vm_disk_write_bytes_total Per-disk write bytes\n\
         # TYPE machina_vm_disk_write_bytes_total counter\n",
    );
    output.push_str(
        "# HELP machina_vm_disk_read_ops_total Per-disk read operations\n\
         # TYPE machina_vm_disk_read_ops_total counter\n",
    );
    output.push_str(
        "# HELP machina_vm_disk_write_ops_total Per-disk write operations\n\
         # TYPE machina_vm_disk_write_ops_total counter\n",
    );
    for m in vm_metrics {
        let vm = escape_label(&vm_prom_label(m));
        for d in &m.disks {
            emit_disk_metric(
                output,
                "machina_vm_disk_read_bytes_total",
                &vm,
                d,
                d.rd_bytes,
            );
            emit_disk_metric(
                output,
                "machina_vm_disk_write_bytes_total",
                &vm,
                d,
                d.wr_bytes,
            );
            emit_disk_metric(output, "machina_vm_disk_read_ops_total", &vm, d, d.rd_ops);
            emit_disk_metric(output, "machina_vm_disk_write_ops_total", &vm, d, d.wr_ops);
        }
    }
}

fn emit_disk_metric(
    output: &mut String,
    name: &str,
    vm: &str,
    d: &VmBlockDeviceMetrics,
    value: u64,
) {
    let device = escape_label(&d.device);
    add_labeled(
        output,
        name,
        &format!("vm=\"{vm}\",device=\"{device}\""),
        value,
    );
}

fn add_vm_net_metrics(output: &mut String, vm_metrics: &[VmMetrics]) {
    output.push_str(
        "# HELP machina_vm_net_rx_bytes_total Per-interface RX bytes\n\
         # TYPE machina_vm_net_rx_bytes_total counter\n",
    );
    output.push_str(
        "# HELP machina_vm_net_tx_bytes_total Per-interface TX bytes\n\
         # TYPE machina_vm_net_tx_bytes_total counter\n",
    );
    output.push_str(
        "# HELP machina_vm_net_rx_packets_total Per-interface RX packets\n\
         # TYPE machina_vm_net_rx_packets_total counter\n",
    );
    output.push_str(
        "# HELP machina_vm_net_tx_packets_total Per-interface TX packets\n\
         # TYPE machina_vm_net_tx_packets_total counter\n",
    );
    for m in vm_metrics {
        let vm = escape_label(&vm_prom_label(m));
        for n in &m.nets {
            emit_net_metric(output, "machina_vm_net_rx_bytes_total", &vm, n, n.rx_bytes);
            emit_net_metric(output, "machina_vm_net_tx_bytes_total", &vm, n, n.tx_bytes);
            emit_net_metric(
                output,
                "machina_vm_net_rx_packets_total",
                &vm,
                n,
                n.rx_packets,
            );
            emit_net_metric(
                output,
                "machina_vm_net_tx_packets_total",
                &vm,
                n,
                n.tx_packets,
            );
        }
    }
}

fn emit_net_metric(output: &mut String, name: &str, vm: &str, n: &VmNetDeviceMetrics, value: u64) {
    let device = escape_label(&n.device);
    add_labeled(
        output,
        name,
        &format!("vm=\"{vm}\",device=\"{device}\""),
        value,
    );
}

fn add_vm_vcpu_metrics(output: &mut String, vm_metrics: &[VmMetrics]) {
    let mut any = false;
    for m in vm_metrics {
        if !m.vcpus_detail.is_empty() {
            any = true;
            break;
        }
    }
    if !any {
        return;
    }
    output.push_str(
        "# HELP machina_vm_vcpu_cpu_time_seconds_total Per-vCPU CPU time\n\
         # TYPE machina_vm_vcpu_cpu_time_seconds_total counter\n",
    );
    for m in vm_metrics {
        let vm = escape_label(&vm_prom_label(m));
        for v in &m.vcpus_detail {
            let labels = format!("vm=\"{vm}\",vcpu=\"{}\"", v.vcpu);
            add_labeled(
                output,
                "machina_vm_vcpu_cpu_time_seconds_total",
                &labels,
                format!("{:.3}", v.cpu_time_ns as f64 / 1_000_000_000.0),
            );
        }
    }
}

fn add_vm_disk_ops_metrics(output: &mut String, vm_metrics: &[VmMetrics]) {
    output.push_str(
        "# HELP machina_vm_disk_read_ops_total VM aggregate disk read ops\n\
         # TYPE machina_vm_disk_read_ops_total counter\n",
    );
    output.push_str(
        "# HELP machina_vm_disk_write_ops_total VM aggregate disk write ops\n\
         # TYPE machina_vm_disk_write_ops_total counter\n",
    );
    for m in vm_metrics {
        if !m.running {
            continue;
        }
        let label = escape_label(&vm_prom_label(m));
        add_labeled(
            output,
            "machina_vm_disk_read_ops_total",
            &format!("vm=\"{label}\""),
            m.disk_rd_ops,
        );
        add_labeled(
            output,
            "machina_vm_disk_write_ops_total",
            &format!("vm=\"{label}\""),
            m.disk_wr_ops,
        );
    }
}

fn add_vm_cgroup_metrics(output: &mut String, vm_metrics: &[VmMetrics]) {
    let mut any = false;
    for m in vm_metrics {
        if let Some(cg) = &m.cgroup {
            if !cg.available {
                continue;
            }
            any = true;
            break;
        }
    }
    if !any {
        return;
    }
    output.push_str(
        "# HELP machina_vm_cgroup_memory_current_bytes VM cgroup memory.current\n\
         # TYPE machina_vm_cgroup_memory_current_bytes gauge\n",
    );
    output.push_str(
        "# HELP machina_vm_cgroup_memory_max_bytes VM cgroup memory.max\n\
         # TYPE machina_vm_cgroup_memory_max_bytes gauge\n",
    );
    output.push_str(
        "# HELP machina_vm_cgroup_cpu_usage_seconds_total VM cgroup CPU usage seconds\n\
         # TYPE machina_vm_cgroup_cpu_usage_seconds_total gauge\n",
    );
    for m in vm_metrics {
        let Some(cg) = m.cgroup.as_ref() else {
            continue;
        };
        if !cg.available {
            continue;
        }
        let vm = escape_label(&vm_prom_label(m));
        if let Some(b) = cg.memory_current_bytes {
            add_labeled(
                output,
                "machina_vm_cgroup_memory_current_bytes",
                &format!("vm=\"{vm}\""),
                b,
            );
        }
        if let Some(b) = cg.memory_max_bytes {
            add_labeled(
                output,
                "machina_vm_cgroup_memory_max_bytes",
                &format!("vm=\"{vm}\""),
                b,
            );
        }
        if let Some(u) = cg.cpu_usage_usec {
            add_labeled(
                output,
                "machina_vm_cgroup_cpu_usage_seconds_total",
                &format!("vm=\"{vm}\""),
                format!("{:.3}", u as f64 / 1_000_000.0),
            );
        }
    }
}

pub(crate) async fn collect_prometheus_exposition(
    manager: LibvirtManager,
    stats: Arc<DaemonStats>,
    http_metrics: Arc<HttpMetrics>,
) -> String {
    let scrape_start = Instant::now();
    let mut output = String::new();

    let m = manager.clone();
    let data = tokio::task::spawn_blocking(move || {
        let node_info = m.with_conn(node::get_node_info).ok();
        let vm_metrics = m.merge_all_metrics().ok();
        let vms = m.list_all_vms().ok();
        let host_stats = Some(get_host_stats());
        let cgroup = host_linux_obs::read_cgroup_v2_self();
        (node_info, vm_metrics, vms, host_stats, cgroup)
    })
    .await
    .unwrap_or((
        None,
        None,
        None,
        None,
        host_linux_obs::CgroupV2Stats::default(),
    ));

    if let Some(info) = data.0 {
        add_gauge(
            &mut output,
            "machina_node_memory_mb",
            "Total host memory in MB",
            info.memory_mb,
        );
        add_gauge(
            &mut output,
            "machina_node_cpus",
            "Total host CPU cores",
            info.cpu_cores,
        );
        add_gauge(
            &mut output,
            "machina_vms_active",
            "Number of active VMs",
            info.active_vms,
        );
        add_gauge(
            &mut output,
            "machina_vms_defined",
            "Number of defined VMs",
            info.defined_vms,
        );
    }

    if let Some(h) = data.3 {
        add_gauge(
            &mut output,
            "machina_host_cpu_percent",
            "Host CPU utilization percent",
            format!("{:.2}", h.cpu_percent),
        );
        add_gauge(
            &mut output,
            "machina_host_memory_percent",
            "Host memory utilization percent",
            format!("{:.2}", h.memory_percent),
        );
        add_gauge(
            &mut output,
            "machina_host_disk_percent",
            "Root filesystem utilization percent",
            format!("{:.2}", h.disk_percent),
        );
        add_gauge(
            &mut output,
            "machina_host_load_1",
            "1-minute load average",
            format!("{:.2}", h.load_1),
        );
        add_gauge(
            &mut output,
            "machina_host_load_5",
            "5-minute load average",
            format!("{:.2}", h.load_5),
        );
        add_gauge(
            &mut output,
            "machina_host_uptime_seconds",
            "Host uptime seconds",
            h.uptime_secs,
        );
        add_gauge(
            &mut output,
            "machina_host_processes",
            "Process count",
            h.processes,
        );
    }

    if let Some(vms) = &data.2 {
        output.push_str("# HELP machina_vm_state VM state code (0=shutoff 1=running 2=paused)\n# TYPE machina_vm_state gauge\n");
        for vm in vms {
            let label = escape_label(&vm_info_label(vm));
            add_labeled(
                &mut output,
                "machina_vm_state",
                &format!("vm=\"{label}\""),
                vm_state_code(&vm.state),
            );
        }
    }

    if let Some(vm_metrics) = data.1 {
        add_vm_metric(
            &mut output,
            "machina_vm_cpu_time_seconds_total",
            "CPU time in seconds",
            "counter",
            &vm_metrics,
            |m| format!("{:.3}", m.cpu_time_ns as f64 / 1_000_000_000.0),
        );
        add_vm_metric(
            &mut output,
            "machina_vm_cpu_percent",
            "Derived VM CPU utilization percent",
            "gauge",
            &vm_metrics,
            |m| format!("{:.2}", vm_cpu_util_pct(m)),
        );
        add_vm_metric(
            &mut output,
            "machina_vm_memory_used_mb",
            "Memory used in MB",
            "gauge",
            &vm_metrics,
            |m| m.memory_used_mb.to_string(),
        );
        add_vm_metric(
            &mut output,
            "machina_vm_memory_total_mb",
            "Memory total in MB",
            "gauge",
            &vm_metrics,
            |m| m.memory_total_mb.to_string(),
        );
        add_vm_metric(
            &mut output,
            "machina_vm_memory_percent",
            "Memory utilization percent",
            "gauge",
            &vm_metrics,
            |m| format!("{:.2}", m.memory_pct),
        );
        add_vm_metric(
            &mut output,
            "machina_vm_disk_read_bytes_total",
            "Disk read bytes",
            "counter",
            &vm_metrics,
            |m| m.disk_rd_bytes.to_string(),
        );
        add_vm_metric(
            &mut output,
            "machina_vm_disk_write_bytes_total",
            "Disk write bytes",
            "counter",
            &vm_metrics,
            |m| m.disk_wr_bytes.to_string(),
        );
        add_vm_metric(
            &mut output,
            "machina_vm_net_rx_bytes_total",
            "Network RX bytes",
            "counter",
            &vm_metrics,
            |m| m.net_rx_bytes.to_string(),
        );
        add_vm_metric(
            &mut output,
            "machina_vm_net_tx_bytes_total",
            "Network TX bytes",
            "counter",
            &vm_metrics,
            |m| m.net_tx_bytes.to_string(),
        );
        add_vm_disk_metrics(&mut output, &vm_metrics);
        add_vm_disk_ops_metrics(&mut output, &vm_metrics);
        add_vm_net_metrics(&mut output, &vm_metrics);
        add_vm_vcpu_metrics(&mut output, &vm_metrics);
        add_vm_cgroup_metrics(&mut output, &vm_metrics);
    }

    add_gauge(
        &mut output,
        "machina_daemon_uptime_seconds",
        "Daemon process uptime",
        stats.uptime_seconds(),
    );
    add_gauge(
        &mut output,
        "machina_daemon_active_sessions",
        "Active browser cookie sessions",
        stats.active_sessions(),
    );
    add_gauge(
        &mut output,
        "machina_daemon_auth_failures_total",
        "Cumulative failed login attempts",
        stats.auth_failures(),
    );
    add_gauge(
        &mut output,
        "machina_daemon_prometheus_scrapes_total",
        "Prometheus scrape requests served",
        stats.prometheus_scrapes(),
    );
    let scrape_ms = scrape_start.elapsed().as_secs_f64() * 1000.0;
    obs_counters::record_prometheus_scrape(scrape_ms as u64);
    add_gauge(
        &mut output,
        "machina_prometheus_last_scrape_duration_ms",
        "Duration of the last Prometheus scrape in milliseconds",
        scrape_ms as u64,
    );
    add_gauge(
        &mut output,
        "machina_libvirt_reconnects_total",
        "Libvirt reconnect attempts",
        stats.libvirt_reconnects(),
    );
    add_gauge(
        &mut output,
        "machina_api_token_auth_ok_total",
        "Successful API token authentications",
        obs_counters::API_TOKEN_AUTH_OK.load(std::sync::atomic::Ordering::Relaxed),
    );
    add_gauge(
        &mut output,
        "machina_api_token_auth_fail_total",
        "Rejected API token authentications",
        obs_counters::API_TOKEN_AUTH_FAIL.load(std::sync::atomic::Ordering::Relaxed),
    );

    let alerts = automation::load_alerts();
    let unacked = alerts.iter().filter(|a| !a.acknowledged).count();
    let rules = automation::load_alert_rules();
    let rules_enabled = rules.iter().filter(|r| r.enabled).count();
    add_gauge(
        &mut output,
        "machina_alerts_unacknowledged",
        "Unacknowledged Machina alerts",
        unacked,
    );
    add_gauge(
        &mut output,
        "machina_alert_rules_enabled",
        "Enabled alert rules",
        rules_enabled,
    );
    if let Some(ts) = crate::automation_worker::automation_last_tick_unix() {
        add_gauge(
            &mut output,
            "machina_automation_last_tick_unix",
            "Unix timestamp of last automation worker tick",
            ts.max(0) as u64,
        );
    }
    let cfg = MachinaConfig::load();
    let run_as = &cfg.auth.run_as_user;
    add_gauge(
        &mut output,
        "machina_run_as_user_active",
        "1 when OIDC run-as-user impersonation is active",
        if run_as.impersonation_active() { 1 } else { 0 },
    );
    add_gauge(
        &mut output,
        "machina_k8s_metrics_available",
        "1 when the last kubectl top probe succeeded",
        if crate::k8s_metrics_cache::k8s_metrics_available() {
            1
        } else {
            0
        },
    );
    if let Some(ts) = crate::k8s_metrics_cache::k8s_last_probe_unix() {
        add_gauge(
            &mut output,
            "machina_k8s_last_probe_unix",
            "Unix timestamp of last kubectl top probe",
            ts.max(0) as u64,
        );
    }

    let bpf = bpf_probe::probe_bpf_summary();
    add_gauge(
        &mut output,
        "machina_bpfd_up",
        "machina-bpfd control socket present (native eBPF datapath)",
        u32::from(bpf.bpfd_present),
    );
    if !bpf.bpftool_path.is_empty() {
        add_gauge(
            &mut output,
            "machina_bpf_programs",
            "BPF programs on host (bpftool)",
            bpf.program_count,
        );
        add_gauge(
            &mut output,
            "machina_bpf_maps",
            "BPF maps on host (bpftool)",
            bpf.map_count,
        );
        add_gauge(
            &mut output,
            "machina_bpf_cgroup_programs",
            "cgroup BPF programs on host",
            bpf.cgroup_program_count,
        );
    }

    let cgroup = data.4;
    if cgroup.available {
        if let Some(b) = cgroup.memory_current_bytes {
            add_gauge(
                &mut output,
                "machina_cgroup_memory_current_bytes",
                "Daemon cgroup memory.current",
                b,
            );
        }
        if let Some(b) = cgroup.memory_max_bytes {
            add_gauge(
                &mut output,
                "machina_cgroup_memory_max_bytes",
                "Daemon cgroup memory.max",
                b,
            );
        }
        if let Some(u) = cgroup.cpu_usage_usec {
            add_gauge(
                &mut output,
                "machina_cgroup_cpu_usage_seconds_total",
                "Daemon cgroup CPU usage seconds",
                format!("{:.3}", u as f64 / 1_000_000.0),
            );
        }
    }

    let thermal = host_linux_obs::read_hwmon_temps();
    if !thermal.is_empty() {
        output.push_str(
            "# HELP machina_host_hwmon_temp_celsius Hardware temperature from /sys/class/hwmon\n\
             # TYPE machina_host_hwmon_temp_celsius gauge\n",
        );
        for t in thermal {
            let labels = format!("sensor=\"{}\"", escape_label(&t.sensor));
            add_labeled(
                &mut output,
                "machina_host_hwmon_temp_celsius",
                &labels,
                format!("{:.2}", t.temp_celsius),
            );
        }
    }

    output.push_str(&http_metrics.render_prometheus());
    output.push_str(&stats.render_auth_prometheus());
    output
}

async fn prometheus_metrics(
    State(manager): State<LibvirtManager>,
    Extension(stats): Extension<Arc<DaemonStats>>,
    Extension(http_metrics): Extension<Arc<HttpMetrics>>,
) -> impl IntoResponse {
    stats.inc_prometheus_scrape();
    let output = collect_prometheus_exposition(manager, stats, http_metrics).await;
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        output,
    )
}

pub fn prometheus_routes() -> Router<LibvirtManager> {
    Router::new().route("/prometheus", get(prometheus_metrics))
}

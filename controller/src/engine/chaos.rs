// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Chaos game days. A run is: a baseline, each fault step in turn, then a
//! recovery window, with the experiment's probes sampled every two seconds
//! throughout. When probe success over the abort window drops below the
//! threshold the run aborts and every fault is removed at once. Network
//! faults carry a bpfd lease (step length plus a grace period), so they end
//! even if the controller dies mid-run; disk limits and killed VMs are put
//! back by the run itself or, after a crash, by the normal reconcile loop.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use machina_bpf::api::{Request, VmChaosFault};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::engine::bpf;
use crate::state::AppState;

pub const MAX_STEPS: usize = 20;
pub const MAX_TARGETS: usize = 20;
pub const MAX_PROBES: usize = 10;
pub const MAX_TOTAL_SECS: u64 = 3600;
const LEASE_GRACE: u64 = 30;
const PROBE_EVERY: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const KEEP_SAMPLES: usize = 2000;

fn d15() -> u64 {
    15
}
fn d180() -> u64 {
    180
}
fn d200() -> u16 {
    200
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentSpec {
    pub targets: Vec<Uuid>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub probes: Vec<Probe>,
    #[serde(default)]
    pub abort: AbortRule,
    #[serde(default = "d15")]
    pub baseline_secs: u64,
    #[serde(default = "d15")]
    pub recovery_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// Added latency towards the targets.
    Latency {
        delay_ms: u32,
        #[serde(default)]
        jitter_ms: u32,
        secs: u64,
    },
    /// Random packet loss towards the targets.
    Loss { loss_pct: f64, secs: u64 },
    /// Cut the targets off from CIDRs and/or other VMs, both ways.
    Partition {
        #[serde(default)]
        cidrs: Vec<String>,
        #[serde(default)]
        peers: Vec<Uuid>,
        secs: u64,
    },
    /// IOPS limits on the targets' first disk (0 = unlimited).
    Disk {
        #[serde(default)]
        read_iops: u64,
        #[serde(default)]
        write_iops: u64,
        secs: u64,
    },
    /// Power the targets off hard and time how long self-healing takes.
    Kill {
        #[serde(default = "d180")]
        recover_secs: u64,
    },
    /// What losing a host would do (placement simulation; nothing is touched).
    HostFailure { host_id: Uuid },
}

impl Step {
    fn kind(&self) -> &'static str {
        match self {
            Step::Latency { .. } => "latency",
            Step::Loss { .. } => "loss",
            Step::Partition { .. } => "partition",
            Step::Disk { .. } => "disk",
            Step::Kill { .. } => "kill",
            Step::HostFailure { .. } => "host_failure",
        }
    }

    /// Longest the step can take.
    fn secs(&self) -> u64 {
        match self {
            Step::Latency { secs, .. }
            | Step::Loss { secs, .. }
            | Step::Partition { secs, .. }
            | Step::Disk { secs, .. } => *secs,
            Step::Kill { recover_secs } => *recover_secs,
            Step::HostFailure { .. } => 0,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Step::Latency {
                delay_ms,
                jitter_ms,
                secs,
            } if *jitter_ms > 0 => format!("Latency {delay_ms}±{jitter_ms} ms for {secs}s"),
            Step::Latency { delay_ms, secs, .. } => format!("Latency {delay_ms} ms for {secs}s"),
            Step::Loss { loss_pct, secs } => format!("Loss {loss_pct}% for {secs}s"),
            Step::Partition {
                cidrs, peers, secs, ..
            } => format!(
                "Partition from {} for {secs}s",
                [
                    (!cidrs.is_empty()).then(|| cidrs.join(", ")),
                    (!peers.is_empty()).then(|| format!("{} VMs", peers.len())),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" and ")
            ),
            Step::Disk {
                read_iops,
                write_iops,
                secs,
            } => format!("Disk {read_iops}/{write_iops} IOPS (read/write) for {secs}s"),
            Step::Kill { recover_secs } => format!("Kill, recover within {recover_secs}s"),
            Step::HostFailure { .. } => "Host failure (simulated)".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Probe {
    /// TCP connect to `host:port`.
    Tcp { target: String },
    Http {
        url: String,
        #[serde(default = "d200")]
        expect_status: u16,
    },
    /// The VM's recorded state is `running`.
    VmRunning { vm: Uuid },
}

impl Probe {
    fn label(&self) -> String {
        match self {
            Probe::Tcp { target } => format!("tcp {target}"),
            Probe::Http { url, .. } => format!("http {url}"),
            Probe::VmRunning { vm } => format!("vm {vm} running"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AbortRule {
    #[serde(default = "d50")]
    pub min_success_pct: f64,
    #[serde(default = "d20")]
    pub window_secs: u64,
}

fn d50() -> f64 {
    50.0
}
fn d20() -> u64 {
    20
}

impl Default for AbortRule {
    fn default() -> Self {
        AbortRule {
            min_success_pct: d50(),
            window_secs: d20(),
        }
    }
}

fn host_port_ok(s: &str) -> bool {
    let Some((h, p)) = s.rsplit_once(':') else {
        return false;
    };
    let h = h.trim_start_matches('[').trim_end_matches(']');
    !h.is_empty()
        && p.parse::<u16>().is_ok_and(|p| p > 0)
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
}

pub fn total_secs(s: &ExperimentSpec) -> u64 {
    s.baseline_secs + s.recovery_secs + s.steps.iter().map(Step::secs).sum::<u64>()
}

pub fn validate(s: &ExperimentSpec) -> Result<(), String> {
    if s.targets.is_empty() || s.targets.len() > MAX_TARGETS {
        return Err(format!("1..={MAX_TARGETS} target VMs"));
    }
    if s.steps.is_empty() || s.steps.len() > MAX_STEPS {
        return Err(format!("1..={MAX_STEPS} steps"));
    }
    if s.probes.len() > MAX_PROBES {
        return Err(format!("at most {MAX_PROBES} probes"));
    }
    if !(0.0..=100.0).contains(&s.abort.min_success_pct) || s.abort.min_success_pct.is_nan() {
        return Err("abort threshold must be 0..=100 percent".into());
    }
    if !(4..=600).contains(&s.abort.window_secs) {
        return Err("abort window must be 4..=600 seconds".into());
    }
    if s.baseline_secs > 600 || s.recovery_secs > 600 {
        return Err("baseline and recovery at most 600 seconds".into());
    }
    if total_secs(s) > MAX_TOTAL_SECS {
        return Err(format!(
            "an experiment runs at most {MAX_TOTAL_SECS} seconds"
        ));
    }
    for (i, st) in s.steps.iter().enumerate() {
        let at = format!("step {}", i + 1);
        let fault = match st {
            Step::Latency {
                delay_ms,
                jitter_ms,
                secs,
            } => Some(VmChaosFault {
                delay_ms: *delay_ms,
                jitter_ms: *jitter_ms,
                secs: *secs,
                ..probe_fault()
            }),
            Step::Loss { loss_pct, secs } => Some(VmChaosFault {
                loss_pct: *loss_pct,
                secs: *secs,
                ..probe_fault()
            }),
            Step::Partition { cidrs, peers, secs } => {
                if cidrs.is_empty() && peers.is_empty() {
                    return Err(format!("{at}: partition from what? cidrs or peers"));
                }
                Some(VmChaosFault {
                    partition: if cidrs.is_empty() {
                        vec!["10.0.0.1".into()]
                    } else {
                        cidrs.clone()
                    },
                    secs: *secs,
                    ..probe_fault()
                })
            }
            Step::Disk {
                read_iops,
                write_iops,
                secs,
            } => {
                if *read_iops == 0 && *write_iops == 0 {
                    return Err(format!("{at}: set a read or write IOPS limit"));
                }
                if *secs == 0 || *secs > MAX_TOTAL_SECS {
                    return Err(format!("{at}: duration 1..={MAX_TOTAL_SECS} seconds"));
                }
                None
            }
            Step::Kill { recover_secs } => {
                if !(10..=1800).contains(recover_secs) {
                    return Err(format!("{at}: recover within 10..=1800 seconds"));
                }
                None
            }
            Step::HostFailure { .. } => None,
        };
        if let Some(f) = fault {
            machina_bpf::netpol::chaos::validate(&f).map_err(|e| format!("{at}: {e}"))?;
        }
    }
    for (i, p) in s.probes.iter().enumerate() {
        let at = format!("probe {}", i + 1);
        match p {
            Probe::Tcp { target } if !host_port_ok(target) => {
                return Err(format!("{at}: target must be host:port"));
            }
            Probe::Http { url, expect_status } => {
                let ok = (url.starts_with("http://") || url.starts_with("https://"))
                    && url.len() <= 2048
                    && reqwest::Url::parse(url).is_ok();
                if !ok {
                    return Err(format!("{at}: an http(s) URL"));
                }
                if !(100..=599).contains(expect_status) {
                    return Err(format!("{at}: expected status 100..=599"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn probe_fault() -> VmChaosFault {
    VmChaosFault {
        id: "validate".into(),
        vm: "validate".into(),
        ..Default::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Sample {
    /// Milliseconds since the run started.
    pub t: u64,
    pub probe: usize,
    pub phase: usize,
    pub ok: bool,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PhaseReport {
    pub name: String,
    pub kind: String,
    pub started_s: f64,
    pub ended_s: f64,
    pub samples: usize,
    pub ok: usize,
    pub success_pct: Option<f64>,
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovered_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Report {
    pub experiment: String,
    pub targets: Vec<String>,
    pub probes: Vec<String>,
    pub phases: Vec<PhaseReport>,
    pub verdict: String,
    pub findings: Vec<String>,
    pub duration_s: f64,
}

#[derive(Default)]
pub struct Live {
    abort: AtomicBool,
    done: AtomicBool,
    reason: Mutex<Option<String>>,
    phase: Mutex<usize>,
    samples: Mutex<Vec<Sample>>,
    phases: Mutex<Vec<PhaseReport>>,
}

static LIVE: LazyLock<Mutex<HashMap<Uuid, Arc<Live>>>> = LazyLock::new(Default::default);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ask a live run to stop; false if it isn't running here.
pub fn abort(run: Uuid, reason: &str) -> bool {
    let Some(l) = lock(&LIVE).get(&run).cloned() else {
        return false;
    };
    lock(&l.reason).get_or_insert_with(|| reason.to_string());
    l.abort.store(true, Ordering::SeqCst);
    true
}

/// Progress of a live run: the current phase, finished phases so far and the
/// latest samples.
pub fn live(run: Uuid) -> Option<Value> {
    let l = lock(&LIVE).get(&run).cloned()?;
    let samples = lock(&l.samples);
    let tail = samples.len().saturating_sub(300);
    Some(json!({
        "phase": *lock(&l.phase),
        "phases": &*lock(&l.phases),
        "samples": &samples[tail..],
    }))
}

#[derive(Clone)]
struct Target {
    id: Uuid,
    name: String,
    host_id: Uuid,
    addresses: Vec<String>,
}

async fn load_vm(state: &AppState, id: Uuid) -> anyhow::Result<Target> {
    type Row = (String, Option<Uuid>, Option<String>, Option<String>);
    let (name, host, ip, ips): Row =
        crate::db::query_as("SELECT name, host_id, guest_ip, guest_ips FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("VM {id} not found"))?;
    let host_id = host.ok_or_else(|| anyhow::anyhow!("{name} has no host"))?;
    let mut addresses: Vec<String> = ip.into_iter().filter(|a| !a.is_empty()).collect();
    let more: Vec<String> = ips
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    for a in more {
        if !a.is_empty() && !addresses.contains(&a) {
            addresses.push(a);
        }
    }
    Ok(Target {
        id,
        name,
        host_id,
        addresses,
    })
}

async fn observed(state: &AppState, id: Uuid) -> String {
    crate::db::query_scalar("SELECT observed_state FROM vms WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

async fn host_ref(state: &AppState, host: Uuid) -> anyhow::Result<bpf::HostRef> {
    bpf::host(&state.pool, &host.to_string())
        .await
        .filter(|h| h.state == "online")
        .ok_or_else(|| anyhow::anyhow!("host {host} is not online"))
}

async fn agent(state: &AppState, host: Uuid) -> anyhow::Result<crate::agent_client::AgentClient> {
    let h = host_ref(state, host).await?;
    crate::agent_client::connect(&h.addr).await
}

/// `dev` of the first `<disk device='disk'>` target in a domain XML.
pub fn first_disk_target(xml: &str) -> Option<String> {
    let mut rest = xml;
    while let Some(i) = rest.find("<disk ") {
        let block = &rest[i..];
        let end = block.find("</disk>").unwrap_or(block.len());
        let disk = &block[..end];
        let head = &disk[..disk.find('>').unwrap_or(disk.len())];
        if head.contains("device='disk'") || head.contains("device=\"disk\"") {
            if let Some(t) = disk.find("<target ") {
                let tag = &disk[t..];
                for q in ["dev='", "dev=\""] {
                    if let Some(s) = tag.find(q) {
                        let v = &tag[s + q.len()..];
                        if let Some(e) = v.find(['\'', '"']) {
                            return Some(v[..e].to_string());
                        }
                    }
                }
            }
        }
        rest = &block[end.min(block.len())..];
        if end == block.len() {
            break;
        }
    }
    None
}

async fn disk_limit(state: &AppState, t: &Target, read: u64, write: u64) -> anyhow::Result<()> {
    let mut c = agent(state, t.host_id).await?;
    let xml = crate::agent_client::get_domain_xml(&mut c, &t.name).await?;
    let dev = first_disk_target(&xml).ok_or_else(|| anyhow::anyhow!("{}: no disk", t.name))?;
    crate::agent_client::vm_libvirt_invoke(
        &mut c,
        &t.name,
        "disk.iotune",
        &json!({ "target": dev, "read_iops": read, "write_iops": write }),
    )
    .await?;
    Ok(())
}

fn fault_id(run: Uuid, step: usize, vm: Uuid) -> String {
    format!("chaos:{}:{step}:{}", run.simple(), vm.simple())
}

fn run_prefix(run: Uuid) -> String {
    format!("chaos:{}:", run.simple())
}

async fn stop_faults(state: &AppState, run: Uuid) {
    let req = Request::VmChaosStop {
        id: String::new(),
        prefix: run_prefix(run),
    };
    for (h, r) in bpf::fan_out(&state.pool, &req).await {
        if let Err(e) = r {
            tracing::warn!(host = %h.hostname, "chaos: stop faults: {e:#}");
        }
    }
}

async fn probe_once(client: &reqwest::Client, state: &AppState, p: &Probe) -> (bool, u64) {
    let t0 = Instant::now();
    let ok = match p {
        Probe::Tcp { target } => matches!(
            tokio::time::timeout(PROBE_TIMEOUT, tokio::net::TcpStream::connect(target)).await,
            Ok(Ok(_))
        ),
        Probe::Http { url, expect_status } => client
            .get(url)
            .send()
            .await
            .is_ok_and(|r| r.status().as_u16() == *expect_status),
        Probe::VmRunning { vm } => observed(state, *vm).await == "running",
    };
    (ok, t0.elapsed().as_millis() as u64)
}

fn spawn_probes(state: AppState, probes: Vec<Probe>, live: Arc<Live>, started: Instant) {
    if probes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        let mut tick = tokio::time::interval(PROBE_EVERY);
        while !live.done.load(Ordering::SeqCst) {
            tick.tick().await;
            let phase = *lock(&live.phase);
            let results = futures_util::future::join_all(
                probes.iter().map(|p| probe_once(&client, &state, p)),
            )
            .await;
            let t = started.elapsed().as_millis() as u64;
            let mut s = lock(&live.samples);
            for (probe, (ok, ms)) in results.into_iter().enumerate() {
                s.push(Sample {
                    t,
                    probe,
                    phase,
                    ok,
                    ms,
                });
            }
            let excess = s.len().saturating_sub(KEEP_SAMPLES);
            if excess > 0 {
                s.drain(..excess);
            }
        }
    });
}

/// Success rate of samples newer than `window`.
pub fn window_success(samples: &[Sample], now_ms: u64, window_secs: u64) -> Option<(f64, usize)> {
    let from = now_ms.saturating_sub(window_secs * 1000);
    let recent: Vec<&Sample> = samples.iter().filter(|s| s.t >= from).collect();
    if recent.len() < 3 {
        return None;
    }
    let ok = recent.iter().filter(|s| s.ok).count();
    Some((ok as f64 * 100.0 / recent.len() as f64, recent.len()))
}

/// Wait `secs`, or until `until` says so; Err(reason) on abort.
async fn hold<F, Fut>(
    live: &Live,
    rule: &AbortRule,
    started: Instant,
    secs: u64,
    mut until: F,
) -> Result<(), String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if live.abort.load(Ordering::SeqCst) {
            return Err(lock(&live.reason)
                .clone()
                .unwrap_or_else(|| "aborted".into()));
        }
        let now = started.elapsed().as_millis() as u64;
        if let Some((pct, n)) = window_success(&lock(&live.samples), now, rule.window_secs) {
            if pct < rule.min_success_pct {
                let why = format!(
                    "probe success {pct:.0}% over the last {}s ({n} samples) fell below {}%",
                    rule.window_secs, rule.min_success_pct
                );
                lock(&live.reason).get_or_insert(why.clone());
                live.abort.store(true, Ordering::SeqCst);
                return Err(why);
            }
        }
        if Instant::now() >= end || until().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted.get(i).copied()
}

/// Probe statistics of each phase from the samples.
pub fn summarize(phases: &mut [PhaseReport], samples: &[Sample]) {
    for (i, ph) in phases.iter_mut().enumerate() {
        let mine: Vec<&Sample> = samples.iter().filter(|s| s.phase == i).collect();
        ph.samples = mine.len();
        ph.ok = mine.iter().filter(|s| s.ok).count();
        ph.success_pct = (!mine.is_empty()).then(|| ph.ok as f64 * 100.0 / mine.len() as f64);
        let mut ms: Vec<u64> = mine.iter().filter(|s| s.ok).map(|s| s.ms).collect();
        ms.sort_unstable();
        ph.p50_ms = percentile(&ms, 0.5);
        ph.p95_ms = percentile(&ms, 0.95);
    }
}

/// Plain-language findings comparing each phase with the baseline.
pub fn findings(phases: &[PhaseReport]) -> Vec<String> {
    let mut out = Vec::new();
    let base = phases.first();
    for ph in phases.iter().skip(1) {
        if let Some(e) = &ph.error {
            out.push(format!("{}: {e}", ph.name));
            continue;
        }
        if ph.kind == "kill" {
            match ph.recovered_s {
                Some(s) => out.push(format!("{}: back in {s:.0}s", ph.name)),
                None => out.push(format!("{}: did not recover in time", ph.name)),
            }
        }
        if let (Some(pct), Some(b)) = (ph.success_pct, base.and_then(|b| b.success_pct)) {
            if pct + 0.5 < b {
                out.push(format!(
                    "{}: probe success fell from {b:.0}% to {pct:.0}%",
                    ph.name
                ));
            }
        }
        if let (Some(p), Some(b)) = (ph.p95_ms, base.and_then(|b| b.p95_ms)) {
            if p > b * 2 + 50 {
                out.push(format!(
                    "{}: p95 latency rose from {b} ms to {p} ms",
                    ph.name
                ));
            }
        }
    }
    if out.is_empty() && phases.iter().any(|p| p.samples > 0) {
        out.push("Probes held steady through every step.".into());
    }
    out
}

/// Record a run and start it in the background.
pub async fn start(
    state: &AppState,
    experiment: Uuid,
    name: &str,
    spec: ExperimentSpec,
    by: &str,
) -> anyhow::Result<Uuid> {
    validate(&spec).map_err(|e| anyhow::anyhow!(e))?;
    let run = Uuid::new_v4();
    crate::db::query("INSERT INTO chaos_runs (id, experiment_id, started_by) VALUES (?, ?, ?)")
        .bind(run)
        .bind(experiment)
        .bind(by)
        .execute(&state.pool)
        .await?;
    let live = Arc::new(Live::default());
    lock(&LIVE).insert(run, live.clone());
    state.emit_event("chaos.run", format!("{by} started game day {name}"));
    let (st, name) = (state.clone(), name.to_string());
    tokio::spawn(async move {
        let report = execute(&st, run, &name, &spec, &live).await;
        live.done.store(true, Ordering::SeqCst);
        let aborted = lock(&live.reason).clone();
        let status = match (&aborted, report.verdict.as_str()) {
            (Some(_), _) => "aborted",
            (None, "passed") => "passed",
            _ => "failed",
        };
        let _ = crate::db::query("UPDATE chaos_runs SET status = ?, finished_at = CURRENT_TIMESTAMP, abort_reason = ?, report_json = ? WHERE id = ?")
            .bind(status)
            .bind(aborted.unwrap_or_default())
            .bind(serde_json::to_string(&report).unwrap_or_default())
            .bind(run)
            .execute(&st.pool)
            .await;
        st.emit_event("chaos.run", format!("game day {name}: {status}"));
        lock(&LIVE).remove(&run);
    });
    Ok(run)
}

async fn execute(
    state: &AppState,
    run: Uuid,
    name: &str,
    spec: &ExperimentSpec,
    live: &Arc<Live>,
) -> Report {
    let started = Instant::now();
    let secs = |i: Instant| i.duration_since(started).as_secs_f64();
    let mut report = Report {
        experiment: name.into(),
        probes: spec.probes.iter().map(Probe::label).collect(),
        ..Default::default()
    };
    let mut targets = Vec::new();
    for id in &spec.targets {
        match load_vm(state, *id).await {
            Ok(t) => targets.push(t),
            Err(e) => {
                report.verdict = "failed".into();
                report.findings = vec![format!("{e:#}")];
                return report;
            }
        }
    }
    report.targets = targets.iter().map(|t| t.name.clone()).collect();
    spawn_probes(state.clone(), spec.probes.clone(), live.clone(), started);

    let begin_phase = |live: &Live, name: String, kind: &str| {
        let mut ph = lock(&live.phases);
        ph.push(PhaseReport {
            name,
            kind: kind.into(),
            started_s: secs(Instant::now()),
            ..Default::default()
        });
        *lock(&live.phase) = ph.len() - 1;
    };
    let end_phase = |live: &Live, f: &dyn Fn(&mut PhaseReport)| {
        if let Some(p) = lock(&live.phases).last_mut() {
            p.ended_s = secs(Instant::now());
            f(p);
        }
    };

    begin_phase(live, "Baseline".into(), "baseline");
    let mut outcome = hold(live, &spec.abort, started, spec.baseline_secs, || async {
        false
    })
    .await;
    end_phase(live, &|_| {});
    let mut disk_limited: Vec<Target> = Vec::new();
    let mut killed: Vec<Target> = Vec::new();

    for (i, step) in spec.steps.iter().enumerate() {
        if outcome.is_err() {
            break;
        }
        begin_phase(live, format!("{}. {}", i + 1, step.label()), step.kind());
        let res = run_step(
            state,
            run,
            i,
            step,
            &targets,
            spec,
            live,
            started,
            &mut disk_limited,
            &mut killed,
        )
        .await;
        match res {
            Ok(note) => end_phase(live, &|p| {
                if let StepNote::Recovered(s) = &note {
                    p.recovered_s = *s;
                }
                if let StepNote::Notes(n) = &note {
                    p.notes = n.clone();
                }
            }),
            Err(StepError::Aborted(why)) => {
                end_phase(live, &|p| p.notes.push("aborted here".into()));
                outcome = Err(why);
            }
            Err(StepError::Failed(e)) => {
                end_phase(live, &|p| p.error = Some(e.clone()));
            }
        }
    }

    stop_faults(state, run).await;
    for t in disk_limited.drain(..) {
        if let Err(e) = disk_limit(state, &t, 0, 0).await {
            report
                .findings
                .push(format!("{}: disk limit not removed: {e:#}", t.name));
        }
    }
    if outcome.is_err() {
        for t in &killed {
            if observed(state, t.id).await != "running" {
                if let Ok(mut c) = agent(state, t.host_id).await {
                    let _ = crate::agent_client::vm_power(&mut c, &t.name, "start", None).await;
                }
            }
        }
    }
    if outcome.is_ok() {
        begin_phase(live, "Recovery".into(), "recovery");
        outcome = hold(live, &spec.abort, started, spec.recovery_secs, || async {
            false
        })
        .await;
        end_phase(live, &|_| {});
    }

    let mut phases = lock(&live.phases).clone();
    summarize(&mut phases, &lock(&live.samples));
    let mut f = findings(&phases);
    f.append(&mut report.findings);
    let failed = phases
        .iter()
        .any(|p| p.error.is_some() || (p.kind == "kill" && p.recovered_s.is_none()));
    report.verdict = match (&outcome, failed) {
        (Err(_), _) => "aborted".into(),
        (Ok(()), true) => "failed".into(),
        (Ok(()), false) => "passed".into(),
    };
    if let Err(why) = &outcome {
        f.insert(0, format!("Aborted: {why}. Every fault was removed."));
    }
    report.findings = f;
    report.phases = phases;
    report.duration_s = secs(Instant::now());
    report
}

enum StepNote {
    None,
    Recovered(Option<f64>),
    Notes(Vec<String>),
}

enum StepError {
    Aborted(String),
    Failed(String),
}

#[allow(clippy::too_many_arguments)]
async fn run_step(
    state: &AppState,
    run: Uuid,
    i: usize,
    step: &Step,
    targets: &[Target],
    spec: &ExperimentSpec,
    live: &Live,
    started: Instant,
    disk_limited: &mut Vec<Target>,
    killed: &mut Vec<Target>,
) -> Result<StepNote, StepError> {
    let failed = |e: anyhow::Error| StepError::Failed(format!("{e:#}"));
    match step {
        Step::Latency { secs, .. } | Step::Loss { secs, .. } | Step::Partition { secs, .. } => {
            let (delay_ms, jitter_ms, loss_pct, mut partition) = match step {
                Step::Latency {
                    delay_ms,
                    jitter_ms,
                    ..
                } => (*delay_ms, *jitter_ms, 0.0, Vec::new()),
                Step::Loss { loss_pct, .. } => (0, 0, *loss_pct, Vec::new()),
                Step::Partition { cidrs, .. } => (0, 0, 0.0, cidrs.clone()),
                _ => unreachable!(),
            };
            if let Step::Partition { peers, .. } = step {
                for p in peers {
                    let t = load_vm(state, *p).await.map_err(failed)?;
                    if t.addresses.is_empty() {
                        return Err(StepError::Failed(format!(
                            "{} has no known address",
                            t.name
                        )));
                    }
                    partition.extend(t.addresses);
                }
            }
            for t in targets {
                let host = host_ref(state, t.host_id).await.map_err(failed)?;
                let fault = VmChaosFault {
                    id: fault_id(run, i, t.id),
                    vm: t.name.clone(),
                    tap: None,
                    delay_ms,
                    jitter_ms,
                    loss_pct,
                    partition: partition.clone(),
                    secs: secs + LEASE_GRACE,
                };
                bpf::call(&host, &Request::VmChaosStart { fault })
                    .await
                    .map_err(|e| StepError::Failed(format!("{}: {e:#}", t.name)))?;
            }
            let r = hold(live, &spec.abort, started, *secs, || async { false }).await;
            for t in targets {
                if let Ok(host) = host_ref(state, t.host_id).await {
                    let _ = bpf::call(
                        &host,
                        &Request::VmChaosStop {
                            id: fault_id(run, i, t.id),
                            prefix: String::new(),
                        },
                    )
                    .await;
                }
            }
            r.map(|_| StepNote::None).map_err(StepError::Aborted)
        }
        Step::Disk {
            read_iops,
            write_iops,
            secs,
        } => {
            for t in targets {
                disk_limit(state, t, *read_iops, *write_iops)
                    .await
                    .map_err(|e| StepError::Failed(format!("{}: {e:#}", t.name)))?;
                disk_limited.push(t.clone());
            }
            let r = hold(live, &spec.abort, started, *secs, || async { false }).await;
            for t in disk_limited.drain(..) {
                let _ = disk_limit(state, &t, 0, 0).await;
            }
            r.map(|_| StepNote::None).map_err(StepError::Aborted)
        }
        Step::Kill { recover_secs } => {
            for t in targets {
                let desired: Option<String> =
                    crate::db::query_scalar("SELECT desired_state FROM vms WHERE id = ?")
                        .bind(t.id)
                        .fetch_optional(&state.pool)
                        .await
                        .map_err(|e| failed(e.into()))?;
                if desired.as_deref() != Some("running") {
                    return Err(StepError::Failed(format!(
                        "{} is not meant to stay running, so nothing would bring it back",
                        t.name
                    )));
                }
            }
            let t0 = Instant::now();
            for t in targets {
                let mut c = agent(state, t.host_id).await.map_err(failed)?;
                crate::agent_client::vm_power(&mut c, &t.name, "stop", None)
                    .await
                    .map_err(|e| StepError::Failed(format!("{}: {e:#}", t.name)))?;
                killed.push(t.clone());
            }
            let ids: Vec<Uuid> = targets.iter().map(|t| t.id).collect();
            let seen_down = Arc::new(Mutex::new(std::collections::HashSet::<Uuid>::new()));
            let back = Arc::new(Mutex::new(None::<f64>));
            let r = hold(live, &spec.abort, started, *recover_secs, || {
                let (ids, seen_down, back) = (ids.clone(), seen_down.clone(), back.clone());
                async move {
                    let mut all = true;
                    for id in &ids {
                        let s = observed(state, *id).await;
                        let mut down = lock(&seen_down);
                        if s != "running" {
                            down.insert(*id);
                            all = false;
                        } else if !down.contains(id) {
                            all = false;
                        }
                    }
                    if all {
                        *lock(&back) = Some(t0.elapsed().as_secs_f64());
                    }
                    all
                }
            })
            .await;
            if r.is_ok() {
                killed.clear();
            }
            let recovered = *lock(&back);
            r.map(|_| StepNote::Recovered(recovered))
                .map_err(StepError::Aborted)
        }
        Step::HostFailure { host_id } => {
            let req = crate::engine::ai::digital_twin::ImpactRequest {
                action: "shutdown".into(),
                target_kind: "host".into(),
                target_id: host_id.to_string(),
            };
            let a = crate::engine::ai::digital_twin::analyze_impact(&state.pool, &req)
                .await
                .map_err(failed)?;
            let mut notes = vec![format!("[{}] {}", a.severity, a.summary)];
            notes.extend(a.recommendations.iter().take(5).cloned());
            Ok(StepNote::Notes(notes))
        }
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        recover(&state).await;
    });
}

/// A controller that restarted mid-run can't finish it: mark it and lift
/// its network faults now rather than at lease end.
pub async fn recover(state: &AppState) {
    let runs: Vec<Uuid> = crate::db::query_scalar("SELECT id FROM chaos_runs WHERE status = 'running'")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    for run in runs {
        if lock(&LIVE).contains_key(&run) {
            continue;
        }
        tracing::warn!(%run, "chaos: run interrupted by a controller restart");
        stop_faults(state, run).await;
        let _ = crate::db::query("UPDATE chaos_runs SET status = 'interrupted', finished_at = CURRENT_TIMESTAMP, abort_reason = 'the controller restarted during the run' WHERE id = ?")
            .bind(run)
            .execute(&state.pool)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ExperimentSpec {
        serde_json::from_value(json!({
            "targets": [Uuid::nil()],
            "steps": [
                {"kind": "latency", "delay_ms": 200, "jitter_ms": 20, "secs": 30},
                {"kind": "loss", "loss_pct": 10, "secs": 30},
                {"kind": "partition", "cidrs": ["10.0.0.0/8"], "secs": 20},
                {"kind": "disk", "read_iops": 50, "secs": 20},
                {"kind": "kill"},
                {"kind": "host_failure", "host_id": Uuid::nil()}
            ],
            "probes": [{"kind": "tcp", "target": "10.0.0.5:22"}, {"kind": "http", "url": "http://10.0.0.5/"}]
        }))
        .unwrap()
    }

    #[test]
    fn parses_and_validates() {
        let s = spec();
        validate(&s).unwrap();
        assert_eq!(s.abort.min_success_pct, 50.0);
        assert_eq!(total_secs(&s), 15 + 15 + 30 + 30 + 20 + 20 + 180);
        assert_eq!(s.steps[0].label(), "Latency 200±20 ms for 30s");
        let bad = [
            json!({"targets": [], "steps": [{"kind": "kill"}]}),
            json!({"targets": [Uuid::nil()], "steps": []}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "latency", "delay_ms": 20000, "secs": 5}]}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "loss", "loss_pct": 150, "secs": 5}]}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "partition", "secs": 5}]}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "disk", "secs": 5}]}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "kill", "recover_secs": 5}]}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "latency", "delay_ms": 10, "secs": 4000}]}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "kill"}], "probes": [{"kind": "tcp", "target": "nope"}]}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "kill"}], "probes": [{"kind": "http", "url": "ftp://x"}]}),
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "kill"}], "abort": {"window_secs": 1}}),
        ];
        for (i, b) in bad.iter().enumerate() {
            let s: Result<ExperimentSpec, _> = serde_json::from_value(b.clone());
            assert!(
                s.map(|s| validate(&s)).is_ok_and(|r| r.is_err()),
                "case {i} accepted"
            );
        }
        assert!(serde_json::from_value::<ExperimentSpec>(
            json!({"targets": [Uuid::nil()], "steps": [{"kind": "kill"}], "surprise": 1})
        )
        .is_err());
    }

    #[test]
    fn window_and_summary() {
        let mk = |t, phase, ok, ms| Sample {
            t,
            probe: 0,
            phase,
            ok,
            ms,
        };
        let s = vec![
            mk(1000, 0, true, 3),
            mk(3000, 0, true, 5),
            mk(5000, 1, false, 2000),
            mk(7000, 1, false, 2000),
            mk(9000, 1, true, 400),
        ];
        let (pct, n) = window_success(&s, 9000, 5).unwrap();
        assert_eq!(n, 3);
        assert!((pct - 33.3).abs() < 0.5);
        assert!(window_success(&s, 9000, 1).is_none());
        let mut phases = vec![
            PhaseReport {
                name: "Baseline".into(),
                kind: "baseline".into(),
                ..Default::default()
            },
            PhaseReport {
                name: "1. Loss 50% for 10s".into(),
                kind: "loss".into(),
                ..Default::default()
            },
            PhaseReport {
                name: "2. Kill".into(),
                kind: "kill".into(),
                ..Default::default()
            },
        ];
        summarize(&mut phases, &s);
        assert_eq!((phases[0].samples, phases[0].ok), (2, 2));
        assert_eq!(phases[1].p95_ms, Some(400));
        let f = findings(&phases);
        assert!(
            f.iter().any(|l| l.contains("fell from 100% to 33%")),
            "{f:?}"
        );
        assert!(
            f.iter()
                .any(|l| l.contains("p95 latency rose from 5 ms to 400 ms")),
            "{f:?}"
        );
        assert!(f.iter().any(|l| l.contains("did not recover")), "{f:?}");
    }

    #[test]
    fn finds_root_disk() {
        let xml = "<domain><devices><disk type='file' device='cdrom'><target dev='sda' bus='sata'/></disk>\
                   <disk type='file' device='disk'><driver/><target dev='vda' bus='virtio'/></disk></devices></domain>";
        assert_eq!(first_disk_target(xml).as_deref(), Some("vda"));
        assert_eq!(first_disk_target("<domain/>"), None);
    }
}

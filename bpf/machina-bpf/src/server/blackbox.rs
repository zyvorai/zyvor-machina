// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Per-VM in-memory flight recorder for bpfd telemetry.
//!
//! This is observe-only. It consumes records that bpfd already emits and can
//! never alter policy, enforcement leases, VM state or traffic.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, OnceLock};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::api::{
    BlackBoxEvent, BlackBoxIncident, BlackBoxIncidentStatus, BlackBoxSnapshot, BlackBoxVmStatus,
};

pub const PRE_SECS: i64 = 120;
pub const DEFAULT_POST_SECS: u64 = 15;
pub const MAX_POST_SECS: u64 = 300;
pub const MAX_EVENTS_PER_VM: usize = 8192;

#[derive(Debug, Clone)]
struct Incident {
    meta: BlackBoxIncidentStatus,
    events: Vec<BlackBoxEvent>,
}

#[derive(Debug, Default)]
struct VmFlight {
    rolling: VecDeque<(DateTime<Utc>, BlackBoxEvent)>,
    incident: Option<Incident>,
}

#[derive(Debug, Default)]
struct Recorder {
    vms: HashMap<String, VmFlight>,
}

static RECORDER: OnceLock<Mutex<Recorder>> = OnceLock::new();

fn recorder() -> MutexGuard<'static, Recorder> {
    RECORDER
        .get_or_init(|| Mutex::new(Recorder::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Record one already-decoded bpfd event. Serialization failure is ignored:
/// telemetry must never disturb the datapath.
pub(super) fn record<T: Serialize>(topic: &str, event: &T) {
    let Ok(value) = serde_json::to_value(event) else {
        return;
    };
    recorder().record_value(topic, value, Utc::now());
}

pub(super) fn list() -> Vec<BlackBoxVmStatus> {
    recorder().list(Utc::now())
}

pub(super) fn get(vm: &str) -> BlackBoxSnapshot {
    recorder().get(vm, Utc::now())
}

pub(super) fn trigger(vm: &str, post_secs: Option<u64>, reason: String) -> BlackBoxIncident {
    recorder().trigger(vm, Utc::now(), post_secs, reason)
}

pub(super) fn clear(vm: &str) -> bool {
    recorder().clear(vm)
}

impl Recorder {
    fn record_value(&mut self, topic: &str, value: Value, now: DateTime<Utc>) {
        let vms = event_vms(&value);
        if vms.is_empty() {
            return;
        }
        let event = normalize(topic, value, now);
        for vm in vms {
            self.record_for_vm(&vm, event.clone(), now);
        }
    }

    fn record_for_vm(&mut self, vm: &str, event: BlackBoxEvent, now: DateTime<Utc>) {
        let flight = self.vms.entry(vm.to_string()).or_default();
        prune_rolling(&mut flight.rolling, now);
        flight.rolling.push_back((now, event.clone()));
        while flight.rolling.len() > MAX_EVENTS_PER_VM {
            flight.rolling.pop_front();
        }

        if let Some(incident) = flight.incident.as_mut() {
            let freeze_at = parse_rfc3339(&incident.meta.freeze_at).unwrap_or(now);
            if now <= freeze_at {
                incident.events.push(event.clone());
                if incident.events.len() > MAX_EVENTS_PER_VM {
                    let drain = incident.events.len() - MAX_EVENTS_PER_VM;
                    incident.events.drain(0..drain);
                }
                incident.meta.events = incident.events.len();
            } else {
                incident.meta.frozen = true;
            }
        }

        // High-confidence signals get an automatic post-trigger capture.
        // Do not replace an active incident; a later high signal can start a
        // fresh incident once the previous one is frozen.
        if should_auto_trigger(&event)
            && flight.incident.as_ref().is_none_or(|i| i.meta.frozen)
        {
            Self::trigger_inner(
                vm,
                flight,
                now,
                Some(DEFAULT_POST_SECS),
                format!("automatic {}:{}", event.topic, event.kind),
                true,
            );
        }
    }

    fn trigger(
        &mut self,
        vm: &str,
        now: DateTime<Utc>,
        post_secs: Option<u64>,
        reason: String,
    ) -> BlackBoxIncident {
        let flight = self.vms.entry(vm.to_string()).or_default();
        prune_rolling(&mut flight.rolling, now);
        Self::trigger_inner(vm, flight, now, post_secs, reason, false)
    }

    fn trigger_inner(
        vm: &str,
        flight: &mut VmFlight,
        now: DateTime<Utc>,
        post_secs: Option<u64>,
        reason: String,
        automatic: bool,
    ) -> BlackBoxIncident {
        let post_secs = post_secs
            .unwrap_or(DEFAULT_POST_SECS)
            .min(MAX_POST_SECS);
        let freeze = now + Duration::seconds(post_secs as i64);
        let events: Vec<BlackBoxEvent> = flight.rolling.iter().map(|(_, e)| e.clone()).collect();
        let meta = BlackBoxIncidentStatus {
            id: format!("{}-{}", safe_vm(vm), now.timestamp_millis()),
            triggered_at: now.to_rfc3339_opts(SecondsFormat::Millis, true),
            reason,
            post_secs,
            freeze_at: freeze.to_rfc3339_opts(SecondsFormat::Millis, true),
            frozen: post_secs == 0,
            automatic,
            events: events.len(),
        };
        let public = BlackBoxIncident {
            vm: vm.to_string(),
            status: meta.clone(),
            events: events.clone(),
        };
        flight.incident = Some(Incident { meta, events });
        public
    }

    fn get(&mut self, vm: &str, now: DateTime<Utc>) -> BlackBoxSnapshot {
        let Some(flight) = self.vms.get_mut(vm) else {
            return BlackBoxSnapshot {
                status: BlackBoxVmStatus {
                    vm: vm.to_string(),
                    ..Default::default()
                },
                incident: None,
            };
        };
        prune_rolling(&mut flight.rolling, now);
        refresh_frozen(flight, now);
        BlackBoxSnapshot {
            status: status(vm, flight),
            incident: flight.incident.as_ref().map(|i| BlackBoxIncident {
                vm: vm.to_string(),
                status: i.meta.clone(),
                events: i.events.clone(),
            }),
        }
    }

    fn list(&mut self, now: DateTime<Utc>) -> Vec<BlackBoxVmStatus> {
        let mut names: Vec<String> = self.vms.keys().cloned().collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|vm| {
                let flight = self.vms.get_mut(&vm)?;
                prune_rolling(&mut flight.rolling, now);
                refresh_frozen(flight, now);
                Some(status(&vm, flight))
            })
            .collect()
    }

    fn clear(&mut self, vm: &str) -> bool {
        self.vms.remove(vm).is_some()
    }
}

fn status(vm: &str, flight: &VmFlight) -> BlackBoxVmStatus {
    BlackBoxVmStatus {
        vm: vm.to_string(),
        pre_secs: PRE_SECS as u64,
        max_events: MAX_EVENTS_PER_VM,
        rolling_events: flight.rolling.len(),
        incident: flight.incident.as_ref().map(|i| i.meta.clone()),
    }
}

fn refresh_frozen(flight: &mut VmFlight, now: DateTime<Utc>) {
    if let Some(incident) = flight.incident.as_mut() {
        if now > parse_rfc3339(&incident.meta.freeze_at).unwrap_or(now) {
            incident.meta.frozen = true;
        }
        incident.meta.events = incident.events.len();
    }
}

fn prune_rolling(q: &mut VecDeque<(DateTime<Utc>, BlackBoxEvent)>, now: DateTime<Utc>) {
    let cutoff = now - Duration::seconds(PRE_SECS);
    while q.front().is_some_and(|(at, _)| *at < cutoff) {
        q.pop_front();
    }
}

fn event_vms(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["vm", "src_vm", "dst_vm"] {
        if let Some(vm) = value
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            if !out.iter().any(|x| x == vm) {
                out.push(vm.to_string());
            }
        }
    }
    if let Some(wl) = value.get("workload").and_then(Value::as_object) {
        if wl.get("kind").and_then(Value::as_str) == Some("vm") {
            if let Some(vm) = wl
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                if !out.iter().any(|x| x == vm) {
                    out.push(vm.to_string());
                }
            }
        }
    }
    out
}

fn normalize(topic: &str, value: Value, now: DateTime<Utc>) -> BlackBoxEvent {
    let ts = value
        .get("ts")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| now.to_rfc3339_opts(SecondsFormat::Millis, true));
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .or_else(|| value.get("hook").and_then(Value::as_str))
        .or_else(|| value.get("l7_type").and_then(Value::as_str))
        .unwrap_or(topic)
        .to_string();
    let severity = infer_severity(topic, &value);
    let summary = infer_summary(topic, &kind, &value);
    BlackBoxEvent {
        ts,
        topic: topic.to_string(),
        kind,
        severity,
        summary,
        event: value,
    }
}

fn infer_severity(topic: &str, value: &Value) -> String {
    if let Some(s) = value.get("severity").and_then(Value::as_str) {
        return s.to_string();
    }
    if value.get("denied").and_then(Value::as_bool) == Some(true) {
        return "critical".into();
    }
    if let Some(v) = value.get("verdict").and_then(Value::as_str) {
        if matches!(
            v.to_ascii_lowercase().as_str(),
            "drop" | "dropped" | "deny" | "denied"
        ) {
            return "high".into();
        }
    }
    if matches!(topic, "anomaly" | "alert") {
        return "high".into();
    }
    "info".into()
}

fn infer_summary(topic: &str, kind: &str, value: &Value) -> String {
    if let Some(s) = value.get("summary").and_then(Value::as_str) {
        return s.to_string();
    }
    if let Some(s) = value.get("detail").and_then(Value::as_str) {
        return s.to_string();
    }
    if topic == "flow" {
        let src = value.get("src").and_then(Value::as_str).unwrap_or("");
        let dst = value.get("dst").and_then(Value::as_str).unwrap_or("");
        let verdict = value
            .get("verdict")
            .and_then(Value::as_str)
            .unwrap_or("");
        return format!("{verdict} {src} -> {dst}").trim().to_string();
    }
    format!("{topic}:{kind}")
}

fn should_auto_trigger(event: &BlackBoxEvent) -> bool {
    matches!(event.severity.as_str(), "critical" | "high")
        && matches!(event.topic.as_str(), "anomaly" | "alert" | "guard")
}

fn safe_vm(vm: &str) -> String {
    let s: String = vm
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() { "vm".into() } else { s }
}

fn parse_rfc3339(v: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v)
        .ok()
        .map(|x| x.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-07T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn rolling_window_is_120_seconds() {
        let mut r = Recorder::default();
        let t = base();
        r.record_value("net", json!({"vm":"web","kind":"flow_open"}), t);
        r.record_value(
            "net",
            json!({"vm":"web","kind":"flow_open"}),
            t + Duration::seconds(121),
        );
        assert_eq!(r.get("web", t + Duration::seconds(121)).status.rolling_events, 1);
    }

    #[test]
    fn trigger_keeps_pre_and_post_events_then_freezes() {
        let mut r = Recorder::default();
        let t = base();
        r.record_value("dns", json!({"vm":"db","kind":"dns_query"}), t);
        let i = r.trigger("db", t + Duration::seconds(10), Some(5), "manual".into());
        assert_eq!(i.events.len(), 1);
        r.record_value(
            "flow",
            json!({"vm":"db","verdict":"FORWARDED"}),
            t + Duration::seconds(12),
        );
        r.record_value(
            "flow",
            json!({"vm":"db","verdict":"FORWARDED"}),
            t + Duration::seconds(16),
        );
        let s = r.get("db", t + Duration::seconds(16));
        let i = s.incident.unwrap();
        assert!(i.status.frozen);
        assert_eq!(i.events.len(), 2);
    }

    #[test]
    fn anomaly_auto_triggers() {
        let mut r = Recorder::default();
        let t = base();
        r.record_value(
            "anomaly",
            json!({"vm":"api","kind":"port_scan","severity":"high","summary":"scan"}),
            t,
        );
        let i = r.get("api", t).incident.unwrap();
        assert!(i.status.automatic);
        assert_eq!(i.events.len(), 1);
    }

    #[test]
    fn guard_denial_is_critical_and_auto_triggers() {
        let mut r = Recorder::default();
        let t = base();
        r.record_value(
            "guard",
            json!({"vm":"vm1","hook":"mprotect","denied":true,"detail":"W+X"}),
            t,
        );
        let i = r.get("vm1", t).incident.unwrap();
        assert_eq!(i.events[0].severity, "critical");
        assert!(i.status.automatic);
    }

    #[test]
    fn flow_is_attributed_to_src_and_dst_vm() {
        let mut r = Recorder::default();
        let t = base();
        r.record_value(
            "flow",
            json!({"vm":"a","src_vm":"a","dst_vm":"b","verdict":"FORWARDED"}),
            t,
        );
        assert_eq!(r.get("a", t).status.rolling_events, 1);
        assert_eq!(r.get("b", t).status.rolling_events, 1);
    }

    #[test]
    fn workload_vm_is_attributed() {
        let mut r = Recorder::default();
        let t = base();
        r.record_value(
            "process",
            json!({"workload":{"kind":"vm","name":"worker"},"kind":"exec"}),
            t,
        );
        assert_eq!(r.get("worker", t).status.rolling_events, 1);
    }

    #[test]
    fn post_window_is_bounded() {
        let mut r = Recorder::default();
        let i = r.trigger("db", base(), Some(99_999), "manual".into());
        assert_eq!(i.status.post_secs, MAX_POST_SECS);
    }
}

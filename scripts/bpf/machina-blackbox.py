#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Machina VM Black Box: correlate existing eBPF telemetry into one VM timeline.

Observe-only. It never changes bpfd mode, policy, leases or VM state.
"""
from __future__ import annotations

import argparse
import dataclasses
import datetime as dt
import json
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Iterable

DEFAULT_LIMIT = 1000
MAX_LIMIT = 5000


@dataclasses.dataclass(frozen=True)
class TimelineEvent:
    ts: str
    source: str
    kind: str
    severity: str
    summary: str
    details: dict[str, Any]

    def key(self) -> tuple[dt.datetime, str, str]:
        return (parse_ts(self.ts), self.source, self.kind)


def parse_ts(value: str | None) -> dt.datetime:
    if not value:
        return dt.datetime.min.replace(tzinfo=dt.timezone.utc)
    text = str(value).strip()
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    try:
        parsed = dt.datetime.fromisoformat(text)
    except ValueError:
        return dt.datetime.min.replace(tzinfo=dt.timezone.utc)
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=dt.timezone.utc)
    return parsed.astimezone(dt.timezone.utc)


def vm_matches(record: dict[str, Any], vm: str) -> bool:
    if record.get("vm") == vm or record.get("src_vm") == vm or record.get("dst_vm") == vm:
        return True
    wl = record.get("workload")
    return isinstance(wl, dict) and wl.get("kind") == "vm" and wl.get("name") == vm


def _sev_for_verdict(verdict: str) -> str:
    verdict = verdict.lower()
    if verdict in {"drop", "dropped", "deny", "denied"}:
        return "high"
    if verdict in {"audit", "observed"}:
        return "info"
    return "normal"


def normalize(vm: str, datasets: dict[str, Any]) -> list[TimelineEvent]:
    out: list[TimelineEvent] = []

    for r in _records(datasets.get("events")):
        if not vm_matches(r, vm):
            continue
        verdict = str(r.get("verdict", ""))
        out.append(TimelineEvent(
            str(r.get("ts", "")), "net", str(r.get("kind", "network")),
            _sev_for_verdict(verdict),
            f"{verdict or 'event'} {r.get('proto', '')} {r.get('local', '')}:{r.get('local_port', 0)} → {r.get('remote', '')}:{r.get('remote_port', 0)}".strip(), r,
        ))

    for r in _records(datasets.get("vm_flows")):
        if not vm_matches(r, vm):
            continue
        verdict = str(r.get("verdict", ""))
        out.append(TimelineEvent(
            str(r.get("ts", "")), "vm_flow", str(r.get("l7_type") or "flow"),
            _sev_for_verdict(verdict),
            f"{verdict or 'flow'} {r.get('direction', '')} {r.get('src', '')}:{r.get('src_port', 0)} → {r.get('dst', '')}:{r.get('dst_port', 0)}".strip(), r,
        ))

    for r in _records(datasets.get("dns")):
        if not vm_matches(r, vm):
            continue
        rcode = str(r.get("rcode", ""))
        out.append(TimelineEvent(
            str(r.get("ts", "")), "dns", "dns_response" if r.get("is_response") else "dns_query",
            "medium" if rcode and rcode not in {"NOERROR", "0"} else "normal",
            f"DNS {r.get('qname', '')} {r.get('qtype', '')} {rcode}".strip(), r,
        ))

    for r in _records(datasets.get("l7")):
        if not vm_matches(r, vm):
            continue
        proto = str(r.get("protocol", "l7"))
        target = r.get("host") or r.get("server") or ""
        op = r.get("method") or ""
        path = r.get("path") or ""
        out.append(TimelineEvent(str(r.get("ts", "")), "l7", proto, "normal", f"{proto.upper()} {op} {target}{path}".strip(), r))

    for r in _records(datasets.get("processes")):
        if not vm_matches(r, vm):
            continue
        denied = bool(r.get("denied"))
        out.append(TimelineEvent(
            str(r.get("ts", "")), "process", str(r.get("kind", "process")),
            "high" if denied else "info",
            f"{r.get('kind', 'process')} pid={r.get('pid', 0)} {r.get('comm', '')} {r.get('path') or ''}".strip(), r,
        ))

    for r in _records(datasets.get("anomalies")):
        if not vm_matches(r, vm):
            continue
        out.append(TimelineEvent(str(r.get("ts", "")), "anomaly", str(r.get("kind", "anomaly")), str(r.get("severity", "medium")), str(r.get("summary", "anomaly")), r))

    for r in _records(datasets.get("guard")):
        if not vm_matches(r, vm):
            continue
        denied = bool(r.get("denied"))
        out.append(TimelineEvent(str(r.get("ts", "")), "guard", str(r.get("hook", "guard")), "critical" if denied else "medium", f"VMM guard {r.get('hook', '')}: {r.get('detail', '')}".strip(), r))

    # VM-intel is a point-in-time report, so give it the capture timestamp supplied by collect().
    intel = datasets.get("vm_intel")
    if isinstance(intel, dict) and intel.get("name") == vm:
        ts = str(datasets.get("captured_at", ""))
        for name in ("runq", "block", "fault", "reclaim"):
            hist = intel.get(name)
            if isinstance(hist, dict) and int(hist.get("count", 0) or 0):
                p99 = int(hist.get("p99_ns", 0) or 0)
                sev = "high" if p99 >= 100_000_000 else "medium" if p99 >= 10_000_000 else "info"
                out.append(TimelineEvent(ts, "vm_intel", f"{name}_latency", sev, f"{name} p99={p99 / 1_000_000:.3f} ms ({hist.get('count', 0)} samples)", hist))
        if int(intel.get("migrations", 0) or 0):
            out.append(TimelineEvent(ts, "vm_intel", "vcpu_migration", "info", f"vCPU migrations={intel.get('migrations')}", {"migrations": intel.get("migrations")}))

    return sorted(out, key=TimelineEvent.key)


def _records(value: Any) -> list[dict[str, Any]]:
    if isinstance(value, list):
        return [x for x in value if isinstance(x, dict)]
    if isinstance(value, dict):
        for key in ("items", "events", "flows", "data"):
            v = value.get(key)
            if isinstance(v, list):
                return [x for x in v if isinstance(x, dict)]
    return []


def summarize(events: list[TimelineEvent]) -> dict[str, Any]:
    counts: dict[str, int] = {}
    high = []
    for e in events:
        counts[e.source] = counts.get(e.source, 0) + 1
        if e.severity in {"high", "critical"}:
            high.append(e)
    likely = None
    if high:
        # First severe precursor is intentionally conservative: correlation, not causation.
        e = high[0]
        likely = {"ts": e.ts, "source": e.source, "kind": e.kind, "summary": e.summary}
    return {
        "events": len(events),
        "by_source": counts,
        "severe_events": len(high),
        "first_severe_precursor": likely,
    }


def within(events: Iterable[TimelineEvent], since: dt.datetime | None, until: dt.datetime | None) -> list[TimelineEvent]:
    result = []
    for e in events:
        t = parse_ts(e.ts)
        if since and t < since:
            continue
        if until and t > until:
            continue
        result.append(e)
    return result


def fetch_json(base: str, path: str, token: str | None, timeout: float) -> Any:
    url = base.rstrip("/") + path
    req = urllib.request.Request(url, headers={"Accept": "application/json"})
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        raw = json.load(resp)
    # Machina routes return direct JSON today; tolerate wrapped APIs too.
    if isinstance(raw, dict) and raw.get("ok") is True and "data" in raw:
        return raw["data"]
    return raw


def collect(base: str, vm: str, token: str | None, limit: int, timeout: float) -> tuple[dict[str, Any], dict[str, str]]:
    limit = max(1, min(limit, MAX_LIMIT))
    q = urllib.parse.urlencode({"limit": limit})
    sources = {
        "events": f"/api/v1/bpf/events?{q}",
        "vm_flows": f"/api/v1/flows?{urllib.parse.urlencode({'limit': limit, 'vm': vm})}",
        "dns": f"/api/v1/bpf/dns?{q}",
        "l7": f"/api/v1/bpf/l7?{urllib.parse.urlencode({'limit': limit, 'vm': vm})}",
        "processes": f"/api/v1/bpf/processes?{q}",
        "anomalies": f"/api/v1/bpf/anomalies?{q}",
        "guard": f"/api/v1/bpf/guard/events?{q}",
        "vm_intel": f"/api/v1/bpf/vm-intel/vms/{urllib.parse.quote(vm, safe='')}",
    }
    data: dict[str, Any] = {"captured_at": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")}
    errors: dict[str, str] = {}
    for name, path in sources.items():
        try:
            data[name] = fetch_json(base, path, token, timeout)
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, json.JSONDecodeError) as exc:
            errors[name] = str(exc)
    return data, errors


def as_json(vm: str, events: list[TimelineEvent], errors: dict[str, str]) -> str:
    obj = {
        "vm": vm,
        "summary": summarize(events),
        "source_errors": errors,
        "timeline": [dataclasses.asdict(e) for e in events],
    }
    return json.dumps(obj, indent=2, sort_keys=True)


def as_markdown(vm: str, events: list[TimelineEvent], errors: dict[str, str]) -> str:
    s = summarize(events)
    lines = [f"# Machina Black Box — `{vm}`", "", f"Events: **{s['events']}** · severe: **{s['severe_events']}**", ""]
    precursor = s.get("first_severe_precursor")
    if precursor:
        lines += ["## First severe precursor", "", f"`{precursor['ts']}` **{precursor['source']}/{precursor['kind']}** — {precursor['summary']}", ""]
    lines += ["## Timeline", "", "| Time (UTC) | Severity | Source | Event |", "|---|---|---|---|"]
    for e in events:
        summary = e.summary.replace("|", "\\|").replace("\n", " ")
        lines.append(f"| {e.ts} | {e.severity} | {e.source}/{e.kind} | {summary} |")
    if errors:
        lines += ["", "## Sources unavailable", ""]
        for source, err in sorted(errors.items()):
            lines.append(f"- `{source}`: {err}")
    lines.append("")
    lines.append("> Black Box is observe-only. “First severe precursor” is correlation evidence, not an automatic root-cause claim.")
    return "\n".join(lines) + "\n"


def _parse_bound(value: str | None) -> dt.datetime | None:
    if not value:
        return None
    parsed = parse_ts(value)
    if parsed.year == 1:
        raise ValueError(f"invalid timestamp: {value}")
    return parsed


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="Correlate Machina eBPF telemetry into a per-VM flight-recorder timeline")
    p.add_argument("vm", help="libvirt/Machina VM name")
    p.add_argument("--base-url", default="http://127.0.0.1:5092", help="machina-daemon base URL")
    p.add_argument("--token", default=None, help="Bearer token (prefer MACHINA_TOKEN env wrapper in production)")
    p.add_argument("--limit", type=int, default=DEFAULT_LIMIT, help=f"records per source (1..{MAX_LIMIT})")
    p.add_argument("--timeout", type=float, default=3.0)
    p.add_argument("--since", help="RFC3339 lower bound")
    p.add_argument("--until", help="RFC3339 upper bound")
    p.add_argument("--format", choices=("json", "markdown"), default="markdown")
    p.add_argument("--input", help="offline datasets JSON; skips HTTP collection")
    args = p.parse_args(argv)

    try:
        since, until = _parse_bound(args.since), _parse_bound(args.until)
    except ValueError as exc:
        p.error(str(exc))

    if args.input:
        with open(args.input, "r", encoding="utf-8") as f:
            data = json.load(f)
        errors: dict[str, str] = {}
    else:
        data, errors = collect(args.base_url, args.vm, args.token, args.limit, args.timeout)

    events = within(normalize(args.vm, data), since, until)
    text = as_json(args.vm, events, errors) if args.format == "json" else as_markdown(args.vm, events, errors)
    sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

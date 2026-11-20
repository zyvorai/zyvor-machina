#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import importlib.util
import json
import contextlib
import io
import pathlib
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("blackbox", HERE / "machina-blackbox.py")
bb = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
import sys
sys.modules[SPEC.name] = bb
SPEC.loader.exec_module(bb)


class BlackBoxTests(unittest.TestCase):
    def sample(self):
        return {
            "captured_at": "2026-10-07T00:00:10Z",
            "events": [
                {"ts": "2026-10-07T00:00:01Z", "vm": "pay", "kind": "deny", "verdict": "drop", "proto": "tcp", "local": "10.0.0.2", "local_port": 443, "remote": "1.2.3.4", "remote_port": 50000},
                {"ts": "2026-10-07T00:00:02Z", "vm": "other", "kind": "deny", "verdict": "drop"},
            ],
            "dns": [{"ts": "2026-10-07T00:00:03Z", "vm": "pay", "qname": "db.internal", "qtype": "A", "rcode": "NOERROR", "is_response": False}],
            "l7": [{"ts": "2026-10-07T00:00:04Z", "vm": "pay", "protocol": "http", "method": "GET", "host": "api", "path": "/health"}],
            "processes": [{"ts": "2026-10-07T00:00:05Z", "vm": "pay", "kind": "exec", "pid": 9, "comm": "curl", "denied": False}],
            "anomalies": [{"ts": "2026-10-07T00:00:06Z", "vm": "pay", "kind": "beaconing", "severity": "critical", "summary": "periodic egress"}],
            "guard": [{"ts": "2026-10-07T00:00:07Z", "vm": "pay", "hook": "mprotect", "denied": True, "detail": "W+X", "pid": 7}],
            "vm_intel": {"name": "pay", "runq": {"count": 4, "p99_ns": 120000000, "buckets": []}, "block": {"count": 0, "p99_ns": 0, "buckets": []}, "fault": {"count": 0, "p99_ns": 0, "buckets": []}, "reclaim": {"count": 0, "p99_ns": 0, "buckets": []}, "migrations": 2},
        }

    def test_filters_vm_and_orders(self):
        events = bb.normalize("pay", self.sample())
        self.assertEqual(8, len(events))
        self.assertEqual("net", events[0].source)
        self.assertTrue(all(e.ts for e in events))
        self.assertNotIn("other", json.dumps([e.details for e in events]))

    def test_summary_selects_first_severe_precursor(self):
        s = bb.summarize(bb.normalize("pay", self.sample()))
        self.assertGreaterEqual(s["severe_events"], 3)
        self.assertEqual("net", s["first_severe_precursor"]["source"])

    def test_window(self):
        events = bb.normalize("pay", self.sample())
        got = bb.within(events, bb.parse_ts("2026-10-07T00:00:04Z"), bb.parse_ts("2026-10-07T00:00:06Z"))
        self.assertEqual(["l7", "process", "anomaly"], [e.source for e in got])

    def test_workload_attribution(self):
        self.assertTrue(bb.vm_matches({"workload": {"kind": "vm", "name": "pay"}}, "pay"))
        self.assertFalse(bb.vm_matches({"workload": {"kind": "pod", "name": "pay"}}, "pay"))

    def test_offline_cli_json(self):
        with tempfile.TemporaryDirectory() as td:
            p = pathlib.Path(td) / "sample.json"
            p.write_text(json.dumps(self.sample()), encoding="utf-8")
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                self.assertEqual(0, bb.main(["pay", "--input", str(p), "--format", "json"]))
            self.assertEqual("pay", json.loads(buf.getvalue())["vm"])


if __name__ == "__main__":
    unittest.main()

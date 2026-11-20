#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Minimal HTTP receiver for Machina [metrics_history].remote_write_url JSON samples.

Each POST body is one MetricsHistoryPoint (see core/src/metrics_history.rs).
Appends newline-delimited JSON to a local file for debugging or forwarding.

Usage:
  python3 contrib/ingest/machina-metrics-ingest.py --port 9099 --out /tmp/machina-ingest.jsonl
  # config.toml:
  # remote_write_url = "http://127.0.0.1:9099/ingest"
"""

from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

# Guards against a misbehaving/malicious client driving an unbounded rfile.read()
# off of an arbitrary Content-Length header.
MAX_BODY_BYTES = 10 * 1024 * 1024  # 10 MiB


class Handler(BaseHTTPRequestHandler):
    out_path: Path

    def do_POST(self) -> None:  # noqa: N802
        if self.path not in ("/", "/ingest"):
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", "0") or 0)
        except ValueError:
            self.send_error(400, "invalid Content-Length")
            return
        if length < 0 or length > MAX_BODY_BYTES:
            self.send_error(413, "payload too large")
            return
        raw = self.rfile.read(length) if length > 0 else b"{}"
        try:
            payload = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            self.send_error(400, "invalid JSON")
            return
        line = json.dumps(payload, separators=(",", ":"))
        with self.out_path.open("a", encoding="utf-8") as f:
            f.write(line + "\n")
        self.send_response(204)
        self.end_headers()

    def log_message(self, fmt: str, *args: object) -> None:
        print(f"[ingest] {self.address_string()} - {fmt % args}")


def main() -> None:
    p = argparse.ArgumentParser(description="Machina metrics JSON ingest")
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=9099)
    p.add_argument("--out", type=Path, default=Path("/tmp/machina-metrics-ingest.jsonl"))
    args = p.parse_args()
    args.out.parent.mkdir(parents=True, exist_ok=True)

    handler = type("BoundHandler", (Handler,), {"out_path": args.out})
    server = HTTPServer((args.host, args.port), handler)
    print(f"Listening on http://{args.host}:{args.port}/ingest → {args.out}")
    server.serve_forever()


if __name__ == "__main__":
    main()

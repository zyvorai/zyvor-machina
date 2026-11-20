# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer

from machina import APIError, Client


def serve(handler):
    class H(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_GET(self):
            handler(self)

        do_POST = do_GET

    s = HTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=s.serve_forever, daemon=True).start()
    _orig = s.shutdown
    s.shutdown = lambda: (_orig(), s.server_close())  # also release the socket
    return s, f"http://127.0.0.1:{s.server_port}"


def reply(h, status, obj):
    body = json.dumps(obj).encode()
    h.send_response(status)
    h.send_header("Content-Type", "application/json")
    h.send_header("Content-Length", str(len(body)))
    h.end_headers()
    h.wfile.write(body)


class ClientTests(unittest.TestCase):
    def test_bearer_token_and_error_decoding(self):
        seen = {}

        def h(r):
            seen["auth"] = r.headers.get("Authorization")
            reply(r, 404, {"error": "vm not found", "error_code": "not_found", "remediation": "check the id"})

        s, url = serve(h)
        with self.assertRaises(APIError) as cm:
            Client(url, "tok").get_vm("nope")
        s.shutdown()
        self.assertEqual(seen["auth"], "Bearer tok")
        self.assertTrue(cm.exception.not_found)
        self.assertEqual(cm.exception.code, "not_found")
        self.assertEqual(cm.exception.remediation, "check the id")

    def test_create_vm_sends_the_expected_spec(self):
        got = {}

        def h(r):
            got["path"] = r.path
            got["body"] = json.loads(r.rfile.read(int(r.headers["Content-Length"])))
            reply(r, 200, {"task_id": "t1", "status": "pending", "operation": "vm.apply"})

        s, url = serve(h)
        task = Client(url).create_vm("web-1", vcpus=4, memory="8Gi", tags=["prod"])
        s.shutdown()
        self.assertEqual(task.task_id, "t1")
        self.assertEqual(got["path"], "/api/v1/vms")
        self.assertEqual(got["body"]["metadata"]["name"], "web-1")
        self.assertEqual(got["body"]["spec"]["cpu"]["cores"], 4)
        self.assertEqual(got["body"]["spec"]["memory"], "8Gi")
        self.assertEqual(got["body"]["desired_state"], "running")

    def test_validation_errors_do_not_hit_the_network(self):
        c = Client("http://127.0.0.1:1")
        with self.assertRaises(ValueError):
            c.create_vm("")
        with self.assertRaises(ValueError):
            c.power("id", "explode")

    def test_wait_task_succeeds_and_raises_on_failure(self):
        calls = {"n": 0}

        def ok(r):
            calls["n"] += 1
            reply(r, 200, {"id": "t1", "operation": "vm.apply", "status": "succeeded" if calls["n"] >= 2 else "running", "progress": 50})

        s, url = serve(ok)
        t = Client(url).wait_task("t1", every=0.01)
        s.shutdown()
        self.assertEqual((t.status, calls["n"]), ("succeeded", 2))

        s, url = serve(lambda r: reply(r, 200, {"id": "t2", "operation": "vm.apply", "status": "failed", "message": "no capacity"}))
        with self.assertRaises(RuntimeError):
            Client(url).wait_task("t2", every=0.01)
        s.shutdown()

    def test_wait_vm_state_times_out(self):
        s, url = serve(lambda r: reply(r, 200, {"id": "v", "name": "x", "observed_state": "stopped"}))
        with self.assertRaises(TimeoutError):
            Client(url).wait_vm_state("v", "running", every=0.01, timeout=0.05)
        s.shutdown()

    def test_list_vms_parses_machines(self):
        s, url = serve(lambda r: reply(r, 200, [{"id": "1", "name": "a", "vcpus": 2, "memory_mib": 1024, "guest_ip": None, "tags": ["x"], "unknown_field": 1}]))
        vms = Client(url).list_vms()
        s.shutdown()
        self.assertEqual((vms[0].name, vms[0].vcpus, vms[0].tags), ("a", 2, ["x"]))


if __name__ == "__main__":
    unittest.main()

# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""A small typed client for the Machina controller API.

    from machina import Client
    c = Client("https://10.0.0.5:5093", token="<API key>")
    task = c.create_vm("web-1", vcpus=4, memory="8Gi")
    c.wait_task(task.task_id)

Authenticate with an API key (Settings -> API keys); it is sent as a bearer token.
"""

from __future__ import annotations

import json
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional


class APIError(Exception):
    """A non-2xx answer from the controller."""

    def __init__(self, status: int, message: str, code: str = "", remediation: str = ""):
        self.status, self.message, self.code, self.remediation = status, message, code, remediation
        text = f"machina: HTTP {status}: {message}"
        if remediation:
            text += f" ({remediation})"
        super().__init__(text)

    @property
    def not_found(self) -> bool:
        return self.status == 404


@dataclass
class VM:
    id: str
    name: str
    desired_state: str = ""
    observed_state: str = ""
    lifecycle_phase: str = ""
    last_error: str = ""
    vcpus: int = 0
    memory_mib: int = 0
    ha_enabled: bool = False
    project: Optional[str] = None
    tags: List[str] = field(default_factory=list)
    guest_ip: Optional[str] = None
    guest_tools_status: Optional[str] = None
    host_id: Optional[str] = None

    @classmethod
    def from_json(cls, d: Dict[str, Any]) -> "VM":
        known = {k: d.get(k) for k in cls.__dataclass_fields__ if k in d and d.get(k) is not None}
        return cls(**known)  # type: ignore[arg-type]


@dataclass
class Host:
    id: str
    hostname: str
    address: str = ""
    state: str = ""
    vm_count: int = 0
    site: str = ""
    maintenance_mode: bool = False
    schedulable: bool = True

    @classmethod
    def from_json(cls, d: Dict[str, Any]) -> "Host":
        known = {k: d.get(k) for k in cls.__dataclass_fields__ if k in d and d.get(k) is not None}
        return cls(**known)  # type: ignore[arg-type]


@dataclass
class Task:
    task_id: str
    status: str = ""
    operation: str = ""


@dataclass
class TaskStatus:
    id: str
    status: str
    operation: str = ""
    progress: int = 0
    message: Optional[str] = None


class Client:
    def __init__(self, base_url: str, token: str = "", insecure: bool = False, timeout: float = 60.0):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.timeout = timeout
        self._ctx = None
        if insecure:  # lab only: accept a self-signed certificate
            self._ctx = ssl.create_default_context()
            self._ctx.check_hostname = False
            self._ctx.verify_mode = ssl.CERT_NONE

    def request(self, method: str, path: str, query: Optional[Dict[str, str]] = None, body: Any = None) -> Any:
        url = self.base_url + path
        if query:
            url += "?" + urllib.parse.urlencode({k: v for k, v in query.items() if v})
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("Accept", "application/json")
        req.add_header("User-Agent", "machina-python-sdk/0.1")
        if data is not None:
            req.add_header("Content-Type", "application/json")
        if self.token:
            req.add_header("Authorization", "Bearer " + self.token)
        try:
            with urllib.request.urlopen(req, timeout=self.timeout, context=self._ctx) as resp:
                raw = resp.read()
        except urllib.error.HTTPError as e:
            try:
                raw = e.read().decode(errors="replace")
            finally:
                e.close()
            message, code, remediation = raw.strip(), "", ""
            try:
                parsed = json.loads(raw)
                message = parsed.get("error") or message
                code, remediation = parsed.get("error_code", ""), parsed.get("remediation", "")
            except ValueError:
                pass
            raise APIError(e.code, message, code, remediation) from None
        return json.loads(raw) if raw.strip() else None

    # machines ---------------------------------------------------------------
    def list_vms(self, project: str = "") -> List[VM]:
        return [VM.from_json(v) for v in self.request("GET", "/api/v1/vms", {"project": project}) or []]

    def get_vm(self, vm_id: str) -> VM:
        return VM.from_json(self.request("GET", f"/api/v1/vms/{urllib.parse.quote(vm_id)}"))

    def find_vm_by_name(self, name: str) -> Optional[VM]:
        return next((v for v in self.list_vms() if v.name == name), None)

    def create_vm(
        self,
        name: str,
        vcpus: int = 1,
        memory: str = "2Gi",
        disk_size: str = "20Gi",
        storage_class: str = "silver",
        network: str = "default",
        project: str = "",
        host_id: str = "",
        tags: Optional[List[str]] = None,
        desired_state: str = "running",
    ) -> Task:
        if not name:
            raise ValueError("a machine needs a name")
        meta: Dict[str, Any] = {"name": name}
        if project:
            meta["project"] = project
        body: Dict[str, Any] = {
            "api_version": "virt.zyvor.dev/v1",
            "kind": "VirtualMachine",
            "metadata": meta,
            "spec": {
                "cpu": {"sockets": 1, "cores": max(1, vcpus)},
                "memory": memory,
                "storage": [{"name": "root", "size": disk_size, "class": storage_class}],
                "network": [{"network": network, "ip_mode": "dhcp"}],
                "firmware": "bios",
                "graphics": {"type": "vnc", "listen": "127.0.0.1"},
            },
            "tags": list(tags or []),
            "desired_state": desired_state,
        }
        if host_id:
            body["host_id"] = host_id
        d = self.request("POST", "/api/v1/vms", body=body)
        return Task(d["task_id"], d.get("status", ""), d.get("operation", ""))

    def delete_vm(self, vm_id: str) -> Task:
        d = self.request("POST", f"/api/v1/vms/{urllib.parse.quote(vm_id)}/delete", body={})
        return Task(d["task_id"], d.get("status", ""), d.get("operation", ""))

    def power(self, vm_id: str, action: str) -> Task:
        if action not in ("start", "shutdown", "stop", "reboot"):
            raise ValueError(f"unknown power action {action!r}")
        d = self.request("POST", f"/api/v1/vms/{urllib.parse.quote(vm_id)}/{action}", body={})
        return Task(d["task_id"], d.get("status", ""), d.get("operation", ""))

    # hosts and tasks --------------------------------------------------------
    def list_hosts(self) -> List[Host]:
        return [Host.from_json(h) for h in self.request("GET", "/api/v1/hosts") or []]

    def get_task(self, task_id: str) -> TaskStatus:
        d = self.request("GET", f"/api/v1/tasks/{urllib.parse.quote(task_id)}")
        return TaskStatus(d["id"], d["status"], d.get("operation", ""), d.get("progress", 0), d.get("message"))

    def wait_task(self, task_id: str, every: float = 2.0, timeout: float = 600.0) -> TaskStatus:
        """Poll until the task succeeds. Raises RuntimeError if it fails, TimeoutError if it takes too long."""
        end = time.monotonic() + timeout
        while True:
            t = self.get_task(task_id)
            if t.status in ("succeeded", "completed", "success"):
                return t
            if t.status in ("failed", "error", "cancelled"):
                raise RuntimeError(f"machina: task {t.id} ({t.operation}) {t.status}: {t.message or ''}")
            if time.monotonic() > end:
                raise TimeoutError(f"task {task_id} still {t.status} after {timeout:.0f}s")
            time.sleep(every)

    def wait_vm_state(self, vm_id: str, want: str, every: float = 3.0, timeout: float = 300.0) -> VM:
        end = time.monotonic() + timeout
        while True:
            vm = self.get_vm(vm_id)
            if vm.observed_state == want:
                return vm
            if time.monotonic() > end:
                raise TimeoutError(f"{vm.name} is {vm.observed_state}, not {want}, after {timeout:.0f}s")
            time.sleep(every)

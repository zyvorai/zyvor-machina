# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Shared helpers for the compatibility scripts (boto3_compat_vpc.py, boto3_compat_asg.py) and for run-compat.sh.

Each script prints one machine-readable line per API action:

    RESULT<TAB>PASS|FAIL|SKIP<TAB>script<TAB>action<TAB>detail

run-compat.sh turns those lines into its per-action table. A failed step never stops the script (later steps that need its
output are SKIPped) and never turns into a PASS: the exit code is the number of FAIL lines.

`python3 _compat.py discover` prints `IMAGE=<id>`, `ITYPE=<name>` and `AZ=<zone>` as the endpoint reports them
(DescribeImages, DescribeInstanceTypes, DescribeAvailabilityZones); run-compat.sh feeds these to the other scripts and to Terraform.
"""
import os
import sys
from urllib.parse import urlparse

try:
    import boto3
    from botocore.config import Config
    from botocore.exceptions import ClientError
except ImportError:
    sys.exit("boto3 is required: pip install boto3")


def endpoint(var):
    """The endpoint the caller named, and nothing else. Refuses an AWS hostname and an inherited AWS_ENDPOINT_URL."""
    url = os.environ.get(var)
    if not url:
        sys.exit(f"set {var} to the Machina endpoint (https://HOST:5093/ec2, /autoscaling, /elbv2 or /monitoring)")
    host = (urlparse(url).hostname or "").lower()
    if not host or host.endswith("amazonaws.com") or host.endswith("amazonaws.com.cn"):
        sys.exit(f"refusing to run: {url} is not a Machina endpoint")
    for stray in ("AWS_ENDPOINT_URL", "AWS_PROFILE", "AWS_SESSION_TOKEN"):
        os.environ.pop(stray, None)  # nothing inherited may redirect or re-sign a call
    return url


def client(service, var="MACHINA_EC2_ENDPOINT"):
    return boto3.client(
        service,
        endpoint_url=endpoint(var),
        region_name=os.environ.get("AWS_DEFAULT_REGION", "machina"),
        verify=os.environ.get("MACHINA_EC2_VERIFY", "true").lower() != "false",
        config=Config(retries={"max_attempts": 1}),
    )


def inject(cl, operation, extra):
    """Add query parameters boto3 does not know (Machina's ProjectId, HostId) to one operation's request."""
    service = cl.meta.service_model.service_name

    def hook(params, **_):
        if isinstance(params.get("body"), dict):
            params["body"].update(extra)

    cl.meta.events.register(f"before-call.{service}.{operation}", hook)


def code_of(fn):
    try:
        fn()
        return None
    except ClientError as e:
        return e.response["Error"]["Code"]


class Run:
    def __init__(self, script):
        self.script, self.failed, self.cleanups = script, 0, []

    def _emit(self, status, action, detail=""):
        print(f"RESULT\t{status}\t{self.script}\t{action}\t{str(detail).replace(chr(10), ' ')[:300]}", flush=True)
        if status == "FAIL":
            self.failed += 1

    def step(self, action, fn):
        """Run one API action. Returns its result, or None after recording a FAIL."""
        try:
            out = fn()
            self._emit("PASS", action)
            return out if out is not None else True
        except ClientError as e:
            self._emit("FAIL", action, f"{e.response['Error']['Code']}: {e.response['Error'].get('Message', '')}")
        except Exception as e:  # a bug in the call or the answer is a failure too
            self._emit("FAIL", action, f"{type(e).__name__}: {e}")
        return None

    def expect(self, action, ok, detail=""):
        self._emit("PASS" if ok else "FAIL", action, "" if ok else detail)
        return ok

    def refused(self, action, fn, codes):
        got = code_of(fn)
        self._emit("PASS" if got in codes else "FAIL", action, "" if got in codes else f"expected {'/'.join(codes)}, got {got}")

    def skip(self, action, why):
        self._emit("SKIP", action, why)

    def undo(self, action, fn):
        self.cleanups.append((action, fn))

    def cleanup(self):
        """Reverse order, every one attempted, failures reported (a leftover is a FAIL: someone must delete it)."""
        for action, fn in reversed(self.cleanups):
            try:
                fn()
                self._emit("PASS", "cleanup " + action)
            except Exception as e:
                self._emit("FAIL", "cleanup " + action, f"{type(e).__name__}: {e}")
        self.cleanups = []


def discover():
    ec2 = client("ec2")
    images = ec2.describe_images()["Images"]
    types = ec2.describe_instance_types()["InstanceTypes"]
    zones = ec2.describe_availability_zones()["AvailabilityZones"]
    image = os.environ.get("MACHINA_EC2_IMAGE") or (images[0]["ImageId"] if images else "")
    itype = os.environ.get("MACHINA_EC2_INSTANCE_TYPE")
    if not itype and types:
        itype = sorted(types, key=lambda t: (t["VCpuInfo"]["DefaultVCpus"], t["MemoryInfo"]["SizeInMiB"]))[0]["InstanceType"]
    zone = os.environ.get("MACHINA_EC2_AZ") or (zones[0]["ZoneName"] if zones else "")
    return image, itype or "", zone


if __name__ == "__main__":
    if sys.argv[1:] == ["discover"]:
        i, t, z = discover()
        print(f"IMAGE={i}\nITYPE={t}\nAZ={z}")
        sys.exit(0 if i and t and z else 1)
    sys.exit("usage: _compat.py discover")

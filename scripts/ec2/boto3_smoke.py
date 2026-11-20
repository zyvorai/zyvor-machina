#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""boto3 smoke test for Machina's EC2 API foundation: DryRun, ClientToken, filters, pagination, RunInstances
options and DescribeTags.

    pip install boto3
    MACHINA_EC2_ENDPOINT=https://HOST:5093/ec2 \
    AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... \
    MACHINA_EC2_IMAGE=ubuntu-24.04 [MACHINA_EC2_VERIFY=false] ./scripts/ec2/boto3_smoke.py

Create the access key with `POST /api/v1/ec2/access-keys` (admin). The test launches ONE small throwaway instance
(tagged `machina-smoke`) and terminates it at the end; it never touches other instances. Exit code is the number of
failed checks.
"""
import os
import sys
import time
import uuid

try:
    import boto3
    from botocore.config import Config
    from botocore.exceptions import ClientError
except ImportError:
    sys.exit("boto3 is required: pip install boto3")

ENDPOINT = os.environ.get("MACHINA_EC2_ENDPOINT")
IMAGE = os.environ.get("MACHINA_EC2_IMAGE")
if not ENDPOINT or not IMAGE:
    sys.exit("set MACHINA_EC2_ENDPOINT and MACHINA_EC2_IMAGE (an image/template name or ami- id)")
VERIFY = os.environ.get("MACHINA_EC2_VERIFY", "true").lower() != "false"

ec2 = boto3.client(
    "ec2",
    endpoint_url=ENDPOINT,
    region_name=os.environ.get("AWS_DEFAULT_REGION", "machina"),
    verify=VERIFY,
    config=Config(retries={"max_attempts": 1}),
)

failed = 0


def check(name, ok, detail=""):
    global failed
    print(("PASS " if ok else "FAIL ") + name + (f"  ({detail})" if detail and not ok else ""))
    if not ok:
        failed += 1


def code_of(fn):
    try:
        fn()
        return None
    except ClientError as e:
        return e.response["Error"]["Code"]


tag = f"machina-smoke-{uuid.uuid4().hex[:8]}"
tags = [{"ResourceType": "instance", "Tags": [{"Key": "Name", "Value": tag}, {"Key": "smoke", "Value": tag}]}]
instance_id = None

try:
    # ---- DryRun --------------------------------------------------------------------------------------------
    check("DryRun on RunInstances answers DryRunOperation",
          code_of(lambda: ec2.run_instances(ImageId=IMAGE, MinCount=1, MaxCount=1, DryRun=True)) == "DryRunOperation")
    check("DryRun on DescribeInstances answers DryRunOperation",
          code_of(lambda: ec2.describe_instances(DryRun=True)) == "DryRunOperation")

    # ---- options that cannot be honoured are refused, not dropped ------------------------------------------
    check("IamInstanceProfile is refused",
          code_of(lambda: ec2.run_instances(ImageId=IMAGE, MinCount=1, MaxCount=1, IamInstanceProfile={"Name": "x"})) == "UnsupportedOperation")
    check("MetadataOptions HttpTokens=required is refused",
          code_of(lambda: ec2.run_instances(ImageId=IMAGE, MinCount=1, MaxCount=1,
                                            MetadataOptions={"HttpTokens": "required"})) == "UnsupportedOperation")
    # a root-disk size is applied since the instances package (not refused); DryRun checks it parses without launching
    check("a root-disk size passes validation (DryRun)",
          code_of(lambda: ec2.run_instances(ImageId=IMAGE, MinCount=1, MaxCount=1, DryRun=True,
                                            BlockDeviceMappings=[{"DeviceName": "/dev/sda1", "Ebs": {"VolumeSize": 99}}])) == "DryRunOperation")

    # ---- ClientToken idempotency ---------------------------------------------------------------------------
    token = uuid.uuid4().hex
    launch = dict(ImageId=IMAGE, MinCount=1, MaxCount=1, ClientToken=token, TagSpecifications=tags,
                  InstanceMarketOptions={"MarketType": "spot"})
    first = ec2.run_instances(**launch)
    instance_id = first["Instances"][0]["InstanceId"]
    again = ec2.run_instances(**launch)
    check("same ClientToken and request returns the same instance", again["Instances"][0]["InstanceId"] == instance_id)
    check("same ClientToken with other parameters is a mismatch",
          code_of(lambda: ec2.run_instances(ImageId=IMAGE, MinCount=1, MaxCount=2, ClientToken=token)) == "IdempotentParameterMismatch")

    # ---- filters -------------------------------------------------------------------------------------------
    def ids(**kw):
        r = ec2.describe_instances(**kw)
        return [i["InstanceId"] for res in r["Reservations"] for i in res["Instances"]]

    check("tag:Name filter finds the instance", instance_id in ids(Filters=[{"Name": "tag:Name", "Values": [tag]}]))
    check("wildcard tag filter finds the instance", instance_id in ids(Filters=[{"Name": "tag:Name", "Values": [tag[:-3] + "*"]}]))
    check("tag-key filter finds the instance", instance_id in ids(Filters=[{"Name": "tag-key", "Values": ["smoke"]}]))
    check("tag-value filter finds the instance", instance_id in ids(Filters=[{"Name": "tag-value", "Values": [tag]}]))
    check("a filter that matches nothing returns nothing", ids(Filters=[{"Name": "tag:Name", "Values": [tag + "-nope"]}]) == [])
    check("an unknown filter name is an error, not an empty answer",
          code_of(lambda: ec2.describe_instances(Filters=[{"Name": "no-such-filter", "Values": ["x"]}])) == "InvalidParameterValue")
    check("an unknown filter on DescribeSubnets is an error",
          code_of(lambda: ec2.describe_subnets(Filters=[{"Name": "vpcid", "Values": ["x"]}])) == "InvalidParameterValue")

    # ---- pagination ----------------------------------------------------------------------------------------
    seen, token_page, pages = [], None, 0
    while True:
        kw = {"MaxResults": 5}
        if token_page:
            kw["NextToken"] = token_page
        r = ec2.describe_instances(**kw)
        seen += [i["InstanceId"] for res in r["Reservations"] for i in res["Instances"]]
        pages += 1
        token_page = r.get("NextToken")
        if not token_page or pages > 200:
            break
    check("paging DescribeInstances visits every instance once", len(seen) == len(set(seen)) and instance_id in seen)
    check("MaxResults=0 is rejected", code_of(lambda: ec2.describe_instances(MaxResults=0)) is not None)

    # ---- the fields Terraform reads ------------------------------------------------------------------------
    inst = {}
    for _ in range(60):
        inst = ec2.describe_instances(InstanceIds=[instance_id])["Reservations"][0]["Instances"][0]
        if inst["State"]["Name"] == "running":
            break
        time.sleep(3)
    check("DescribeInstances reports placement", bool(inst.get("Placement", {}).get("AvailabilityZone")), str(inst.get("Placement")))
    check("DescribeInstances reports metadata options", inst.get("MetadataOptions", {}).get("HttpTokens") == "optional")
    check("DescribeInstances reports a root device", inst.get("RootDeviceName") == "/dev/vda")
    check("DescribeInstances reports the tags", {"Key": "smoke", "Value": tag} in inst.get("Tags", []))

    # ---- DescribeTags --------------------------------------------------------------------------------------
    t = ec2.describe_tags(Filters=[{"Name": "resource-type", "Values": ["instance"]}, {"Name": "key", "Values": ["smoke"]}])["Tags"]
    check("DescribeTags filters by resource-type and key", any(x["ResourceId"] == instance_id for x in t))
    t = ec2.describe_tags(Filters=[{"Name": "resource-id", "Values": [instance_id]}])["Tags"]
    check("DescribeTags filters by resource-id", len(t) >= 2 and all(x["ResourceId"] == instance_id for x in t))
    t = ec2.describe_tags(Filters=[{"Name": "resource-type", "Values": ["volume", "image", "vpc", "subnet"]}])["Tags"]
    check("DescribeTags understands other resource types", all(x["ResourceType"] in ("volume", "image", "vpc", "subnet") for x in t))
    check("DescribeTags rejects an unknown filter",
          code_of(lambda: ec2.describe_tags(Filters=[{"Name": "bogus", "Values": ["x"]}])) == "InvalidParameterValue")
finally:
    if instance_id:
        code = code_of(lambda: ec2.terminate_instances(InstanceIds=[instance_id]))
        check("the smoke instance is terminated", code is None, str(code))

print(f"\n{failed} failed")
sys.exit(failed)

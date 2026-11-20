#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""boto3 test for Machina's ELBv2 API (service `elasticloadbalancing`): a network balancer, a target group with an instance,
a listener, health, tags, attributes, paging, and the things the layer-4 balancer refuses.

    pip install boto3
    MACHINA_ELB_ENDPOINT=https://HOST:5093/elbv2 \
    AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... \
    MACHINA_ELB_INSTANCE=i-0123456789abcdef0 [MACHINA_ELB_PORT=18080] [MACHINA_EC2_VERIFY=false] ./scripts/ec2/boto3_elbv2.py

Create the access key with `POST /api/v1/ec2/access-keys` (admin). MACHINA_ELB_INSTANCE is an existing THROWAWAY instance: the test
registers it as a target and removes it again. It creates objects named `machina-elbv2-<random>` and deletes them at the end.
The listener opens MACHINA_ELB_PORT (default 18080/tcp) on the balancer's host for the duration of the test: pick a free port.
Exit code is the number of failed checks.
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

ENDPOINT = os.environ.get("MACHINA_ELB_ENDPOINT")
INSTANCE = os.environ.get("MACHINA_ELB_INSTANCE")
if not ENDPOINT or not INSTANCE:
    sys.exit("set MACHINA_ELB_ENDPOINT (…/elbv2) and MACHINA_ELB_INSTANCE (a throwaway instance id)")
PORT = int(os.environ.get("MACHINA_ELB_PORT", "18080"))
VERIFY = os.environ.get("MACHINA_EC2_VERIFY", "true").lower() != "false"

elb = boto3.client(
    "elbv2",
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


suffix = uuid.uuid4().hex[:8]
lb_name, tg_name = f"machina-elbv2-{suffix}", f"machina-elbv2-tg-{suffix}"
lb_arn = tg_arn = listener_arn = None

try:
    # ---- create ------------------------------------------------------------------------------------------------
    lb = elb.create_load_balancer(Name=lb_name, Type="network", Tags=[{"Key": "smoke", "Value": suffix}])["LoadBalancers"][0]
    lb_arn = lb["LoadBalancerArn"]
    check("CreateLoadBalancer returns an arn, the name and type", lb["LoadBalancerName"] == lb_name and lb["Type"] == "network", lb)
    check("the balancer is active", lb["State"]["Code"] == "active", lb["State"])
    check("a duplicate name is refused", code_of(lambda: elb.create_load_balancer(Name=lb_name, Type="network")) == "DuplicateLoadBalancerName")

    tg = elb.create_target_group(Name=tg_name, Protocol="TCP", Port=PORT, TargetType="instance")["TargetGroups"][0]
    tg_arn = tg["TargetGroupArn"]
    check("CreateTargetGroup returns the health check defaults", tg["HealthCheckProtocol"] == "TCP" and tg["HealthCheckIntervalSeconds"] == 30, tg)

    # ---- refused, not dropped ----------------------------------------------------------------------------------
    check("an HTTPS listener is refused",
          code_of(lambda: elb.create_listener(LoadBalancerArn=lb_arn, Protocol="TLS", Port=PORT + 1,
                                              DefaultActions=[{"Type": "forward", "TargetGroupArn": tg_arn}])) == "UnsupportedOperation")
    check("a redirect action is refused",
          code_of(lambda: elb.create_listener(LoadBalancerArn=lb_arn, Protocol="TCP", Port=PORT + 1,
                                              DefaultActions=[{"Type": "redirect", "RedirectConfig": {"StatusCode": "HTTP_301"}}])) == "UnsupportedOperation")
    check("security groups on a balancer are refused",
          code_of(lambda: elb.create_load_balancer(Name=f"{lb_name}-sg", Type="network", SecurityGroups=["sg-0123456789abcdef0"])) == "UnsupportedOperation")
    check("an ip target group is refused",
          code_of(lambda: elb.create_target_group(Name=f"{tg_name}-ip", Protocol="TCP", Port=80, TargetType="ip")) == "UnsupportedOperation")

    # ---- targets, listener, health -----------------------------------------------------------------------------
    elb.register_targets(TargetGroupArn=tg_arn, Targets=[{"Id": INSTANCE}])
    th = elb.describe_target_health(TargetGroupArn=tg_arn)["TargetHealthDescriptions"]
    check("a target of an unattached group is unused", len(th) == 1 and th[0]["TargetHealth"]["State"] == "unused", th)

    listener = elb.create_listener(LoadBalancerArn=lb_arn, Protocol="TCP", Port=PORT,
                                   DefaultActions=[{"Type": "forward", "TargetGroupArn": tg_arn}])["Listeners"][0]
    listener_arn = listener["ListenerArn"]
    check("CreateListener forwards to the group", listener["DefaultActions"][0]["TargetGroupArn"] == tg_arn, listener)
    check("a second listener on the same port is refused",
          code_of(lambda: elb.create_listener(LoadBalancerArn=lb_arn, Protocol="TCP", Port=PORT,
                                              DefaultActions=[{"Type": "forward", "TargetGroupArn": tg_arn}])) == "DuplicateListener")

    state = None
    for _ in range(30):  # the host's agent probes every few seconds
        state = elb.describe_target_health(TargetGroupArn=tg_arn)["TargetHealthDescriptions"][0]["TargetHealth"]["State"]
        if state != "initial":
            break
        time.sleep(2)
    check("the target is probed (healthy or unhealthy, not stuck initial)", state in ("healthy", "unhealthy"), state)

    # ---- rules -------------------------------------------------------------------------------------------------
    rules = elb.describe_rules(ListenerArn=listener_arn)["Rules"]
    check("only the default rule exists", len(rules) == 1 and rules[0]["IsDefault"] and rules[0]["Priority"] == "default", rules)
    check("a conditional rule is refused",
          code_of(lambda: elb.create_rule(ListenerArn=listener_arn, Priority=10,
                                          Conditions=[{"Field": "path-pattern", "Values": ["/api/*"]}],
                                          Actions=[{"Type": "forward", "TargetGroupArn": tg_arn}])) == "UnsupportedOperation")

    # ---- attributes, tags, paging --------------------------------------------------------------------------------
    attrs = {a["Key"]: a["Value"] for a in elb.describe_load_balancer_attributes(LoadBalancerArn=lb_arn)["Attributes"]}
    check("attributes have defaults", attrs.get("deletion_protection.enabled") == "false", attrs)
    elb.modify_load_balancer_attributes(LoadBalancerArn=lb_arn, Attributes=[{"Key": "deletion_protection.enabled", "Value": "true"}])
    check("deletion protection blocks DeleteLoadBalancer", code_of(lambda: elb.delete_load_balancer(LoadBalancerArn=lb_arn)) == "OperationNotPermitted")
    elb.modify_load_balancer_attributes(LoadBalancerArn=lb_arn, Attributes=[{"Key": "deletion_protection.enabled", "Value": "false"}])
    check("access logs cannot be switched on",
          code_of(lambda: elb.modify_load_balancer_attributes(LoadBalancerArn=lb_arn, Attributes=[{"Key": "access_logs.s3.enabled", "Value": "true"}])) == "UnsupportedOperation")

    elb.add_tags(ResourceArns=[lb_arn, tg_arn], Tags=[{"Key": "team", "Value": "infra"}])
    tags = {t["ResourceArn"]: {x["Key"]: x["Value"] for x in t["Tags"]} for t in elb.describe_tags(ResourceArns=[lb_arn, tg_arn])["TagDescriptions"]}
    check("tags are listed per resource", tags.get(lb_arn, {}).get("team") == "infra" and tags.get(tg_arn, {}).get("team") == "infra", tags)

    page = elb.describe_load_balancers(PageSize=1)
    check("PageSize limits a page", len(page["LoadBalancers"]) == 1, page)
    seen, marker = [], None
    while True:
        r = elb.describe_load_balancers(PageSize=1, **({"Marker": marker} if marker else {}))
        seen += [x["LoadBalancerArn"] for x in r["LoadBalancers"]]
        marker = r.get("NextMarker")
        if not marker:
            break
    check("paging reaches our balancer", lb_arn in seen, seen)
    check("a missing balancer is LoadBalancerNotFound", code_of(lambda: elb.describe_load_balancers(Names=[f"{lb_name}-nope"])) == "LoadBalancerNotFound")

    # ---- in use ------------------------------------------------------------------------------------------------
    check("a target group in use cannot be deleted", code_of(lambda: elb.delete_target_group(TargetGroupArn=tg_arn)) == "ResourceInUse")
finally:
    for label, fn in (
        ("deregister", lambda: elb.deregister_targets(TargetGroupArn=tg_arn, Targets=[{"Id": INSTANCE}]) if tg_arn else None),
        ("delete listener", lambda: elb.delete_listener(ListenerArn=listener_arn) if listener_arn else None),
        ("delete target group", lambda: elb.delete_target_group(TargetGroupArn=tg_arn) if tg_arn else None),
        ("delete load balancer", lambda: elb.delete_load_balancer(LoadBalancerArn=lb_arn) if lb_arn else None),
    ):
        try:
            fn()
        except ClientError as e:
            print(f"cleanup {label}: {e.response['Error']['Code']}")

print(f"{failed} check(s) failed")
sys.exit(failed)

#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""boto3 check for Machina's Auto Scaling service: a launch configuration, a group, capacity, policies, tags,
activities and cleanup.

    pip install boto3
    MACHINA_EC2_ENDPOINT=https://HOST:5093/autoscaling \
    AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... \
    MACHINA_SUBNET=subnet-... MACHINA_EC2_IMAGE=ubuntu-24.04 [MACHINA_EC2_VERIFY=false] ./scripts/ec2/boto3_asg.py

The subnet must be a ready subnet of a project (Platform -> Cloud -> VPCs). The group is created with
min 0 / desired 0 / max 2 so no instance is launched unless you pass MACHINA_ASG_LAUNCH=1, which raises the desired
capacity to 1 and waits for the instance (the reconciler launches it from the template) before scaling back to 0.
Everything the script creates is named machina-asg-smoke-<random> and deleted at the end. Exit code is the number of
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
SUBNET = os.environ.get("MACHINA_SUBNET")
IMAGE = os.environ.get("MACHINA_EC2_IMAGE")
if not ENDPOINT or not SUBNET or not IMAGE:
    sys.exit("set MACHINA_EC2_ENDPOINT, MACHINA_SUBNET and MACHINA_EC2_IMAGE")
VERIFY = os.environ.get("MACHINA_EC2_VERIFY", "true").lower() != "false"
LAUNCH = os.environ.get("MACHINA_ASG_LAUNCH") == "1"

asg = boto3.client(
    "autoscaling",
    endpoint_url=ENDPOINT,
    region_name=os.environ.get("AWS_DEFAULT_REGION", "machina"),
    verify=VERIFY,
    config=Config(retries={"max_attempts": 1}),
)

failed = 0


def check(name, ok, detail=""):
    global failed
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"  {detail}"))
    if not ok:
        failed += 1


def code_of(fn):
    try:
        fn()
        return None
    except ClientError as e:
        return e.response["Error"]["Code"]


suffix = uuid.uuid4().hex[:8]
lc = f"machina-asg-smoke-{suffix}"
group = f"machina-asg-smoke-{suffix}"

try:
    asg.create_launch_configuration(LaunchConfigurationName=lc, ImageId=IMAGE, InstanceMonitoring={"Enabled": False})
    check("create launch configuration", True)
    check(
        "describe launch configuration",
        [c["LaunchConfigurationName"] for c in asg.describe_launch_configurations(LaunchConfigurationNames=[lc])["LaunchConfigurations"]] == [lc],
    )
    check(
        "unsupported launch configuration options are refused",
        code_of(lambda: asg.create_launch_configuration(LaunchConfigurationName=lc + "x", ImageId=IMAGE, SpotPrice="0.1")) == "UnsupportedOperation",
    )

    asg.create_auto_scaling_group(
        AutoScalingGroupName=group,
        LaunchConfigurationName=lc,
        MinSize=0,
        MaxSize=2,
        DesiredCapacity=0,
        VPCZoneIdentifier=SUBNET,
        Tags=[{"Key": "smoke", "Value": "1", "PropagateAtLaunch": True}],
    )
    check("create group", True)
    g = asg.describe_auto_scaling_groups(AutoScalingGroupNames=[group])["AutoScalingGroups"]
    check("describe group", len(g) == 1 and g[0]["MaxSize"] == 2 and g[0]["DesiredCapacity"] == 0, str(g))
    check("duplicate name", code_of(lambda: asg.create_auto_scaling_group(AutoScalingGroupName=group, LaunchConfigurationName=lc, MinSize=0, MaxSize=1, VPCZoneIdentifier=SUBNET)) == "AlreadyExists")
    check("mixed instances policy is refused", code_of(lambda: asg.create_auto_scaling_group(AutoScalingGroupName=group + "m", MinSize=0, MaxSize=1, VPCZoneIdentifier=SUBNET, MixedInstancesPolicy={"LaunchTemplate": {"LaunchTemplateSpecification": {"LaunchTemplateName": "x"}}})) == "UnsupportedOperation")

    asg.update_auto_scaling_group(AutoScalingGroupName=group, MaxSize=3, DefaultCooldown=60)
    g = asg.describe_auto_scaling_groups(AutoScalingGroupNames=[group])["AutoScalingGroups"][0]
    check("update group", g["MaxSize"] == 3 and g["DefaultCooldown"] == 60, str(g))

    asg.set_desired_capacity(AutoScalingGroupName=group, DesiredCapacity=1)
    g = asg.describe_auto_scaling_groups(AutoScalingGroupNames=[group])["AutoScalingGroups"][0]
    check("set desired capacity", g["DesiredCapacity"] == 1)
    check("desired above max is refused", code_of(lambda: asg.set_desired_capacity(AutoScalingGroupName=group, DesiredCapacity=9)) == "ValidationError")

    if LAUNCH:
        deadline = time.time() + 600
        while time.time() < deadline:
            g = asg.describe_auto_scaling_groups(AutoScalingGroupNames=[group])["AutoScalingGroups"][0]
            if any(i["LifecycleState"] == "InService" for i in g["Instances"]):
                break
            time.sleep(10)
        check("the reconciler launched an instance", any(i["LifecycleState"] == "InService" for i in g["Instances"]), str(g["Instances"]))

    out = asg.put_scaling_policy(AutoScalingGroupName=group, PolicyName="out", AdjustmentType="ChangeInCapacity", ScalingAdjustment=1)
    check("put simple policy", out["PolicyARN"].startswith("arn:aws:autoscaling:"))
    asg.execute_policy(AutoScalingGroupName=group, PolicyName="out")
    g = asg.describe_auto_scaling_groups(AutoScalingGroupNames=[group])["AutoScalingGroups"][0]
    check("execute policy", g["DesiredCapacity"] == 2, str(g["DesiredCapacity"]))
    asg.put_scaling_policy(
        AutoScalingGroupName=group,
        PolicyName="cpu",
        PolicyType="TargetTrackingScaling",
        TargetTrackingConfiguration={"PredefinedMetricSpecification": {"PredefinedMetricType": "ASGAverageCPUUtilization"}, "TargetValue": 60.0},
    )
    pols = {p["PolicyName"] for p in asg.describe_policies(AutoScalingGroupName=group)["ScalingPolicies"]}
    check("describe policies", pols == {"out", "cpu"}, str(pols))

    asg.create_or_update_tags(Tags=[{"ResourceId": group, "ResourceType": "auto-scaling-group", "Key": "team", "Value": "infra", "PropagateAtLaunch": False}])
    tags = {t["Key"] for t in asg.describe_tags(Filters=[{"Name": "auto-scaling-group", "Values": [group]}])["Tags"]}
    check("tags", tags == {"smoke", "team"}, str(tags))

    acts = asg.describe_scaling_activities(AutoScalingGroupName=group)["Activities"]
    check("scaling activities are recorded", len(acts) >= 3, str(len(acts)))

    asg.suspend_processes(AutoScalingGroupName=group, ScalingProcesses=["Launch", "Terminate"])
    g = asg.describe_auto_scaling_groups(AutoScalingGroupNames=[group])["AutoScalingGroups"][0]
    check("suspend Launch and Terminate", {p["ProcessName"] for p in g["SuspendedProcesses"]} == {"Launch", "Terminate"})
    asg.resume_processes(AutoScalingGroupName=group)
finally:
    try:
        asg.delete_policy(AutoScalingGroupName=group, PolicyName="cpu")
    except Exception:
        pass
    try:
        asg.set_desired_capacity(AutoScalingGroupName=group, DesiredCapacity=0)
        asg.update_auto_scaling_group(AutoScalingGroupName=group, MinSize=0)
    except Exception:
        pass
    try:
        asg.delete_auto_scaling_group(AutoScalingGroupName=group, ForceDelete=True)
    except Exception as e:
        print("cleanup: group not deleted:", e)
    try:
        asg.delete_launch_configuration(LaunchConfigurationName=lc)
    except Exception as e:
        print("cleanup: launch configuration not deleted:", e)

print(f"{failed} failed")
sys.exit(failed)

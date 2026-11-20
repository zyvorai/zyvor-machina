#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""boto3 chain against Machina's Auto Scaling (/autoscaling) and, for the launch template, EC2: launch configuration, group
(min 0, desired 0: nothing is launched), policy, tags, activities, then teardown. One line per action.

    pip install boto3
    MACHINA_AUTOSCALING_ENDPOINT=https://HOST:5093/autoscaling MACHINA_EC2_ENDPOINT=https://HOST:5093/ec2 \
    AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... MACHINA_SUBNET=subnet-... [MACHINA_PROJECT_ID=<uuid>] \
    [MACHINA_EC2_VERIFY=false] ./scripts/ec2/boto3_compat_asg.py

MACHINA_SUBNET is a ready subnet (run-compat.sh passes the one boto3_compat_vpc.py left, or you give an existing one).
The image comes from DescribeImages. With MACHINA_PROJECT_ID the script also tries CreateLaunchTemplate (Machina requires
ProjectId there; the script adds it) and a group that uses the template. Objects are named machina-compat-<pid> and
removed in a finally block. Exit code is the number of FAIL lines.
"""
import os
import sys

import _compat as c

SUBNET = os.environ.get("MACHINA_SUBNET")
if not SUBNET:
    sys.exit("set MACHINA_SUBNET (a ready subnet id)")
PROJECT = os.environ.get("MACHINA_PROJECT_ID")

asg = c.client("autoscaling", "MACHINA_AUTOSCALING_ENDPOINT")
ec2 = c.client("ec2")
r = c.Run("boto3_compat_asg")
image, _itype, _zone = c.discover()
r.expect("discover image", bool(image), "DescribeImages returned nothing")
if not image:
    sys.exit(1)
name = f"machina-compat-{os.getpid()}"
lc, grp, lt_grp = name, name, name + "-lt"

try:
    r.step("DescribeAccountLimits", lambda: asg.describe_account_limits())
    r.step("CreateLaunchConfiguration", lambda: asg.create_launch_configuration(LaunchConfigurationName=lc, ImageId=image, InstanceMonitoring={"Enabled": False}))
    r.undo("DeleteLaunchConfiguration", lambda: asg.delete_launch_configuration(LaunchConfigurationName=lc))
    r.step("DescribeLaunchConfigurations", lambda: [x for x in asg.describe_launch_configurations(LaunchConfigurationNames=[lc])["LaunchConfigurations"] if x["LaunchConfigurationName"] == lc][0])
    if r.step("CreateAutoScalingGroup", lambda: asg.create_auto_scaling_group(
            AutoScalingGroupName=grp, LaunchConfigurationName=lc, MinSize=0, MaxSize=1, DesiredCapacity=0, VPCZoneIdentifier=SUBNET,
            Tags=[{"Key": "compat", "Value": "1", "PropagateAtLaunch": True}])):
        r.undo("DeleteAutoScalingGroup", lambda: asg.delete_auto_scaling_group(AutoScalingGroupName=grp, ForceDelete=True))
        r.step("DescribeAutoScalingGroups", lambda: asg.describe_auto_scaling_groups(AutoScalingGroupNames=[grp])["AutoScalingGroups"][0])
        r.step("UpdateAutoScalingGroup", lambda: asg.update_auto_scaling_group(AutoScalingGroupName=grp, MaxSize=2))
        r.step("PutScalingPolicy", lambda: asg.put_scaling_policy(AutoScalingGroupName=grp, PolicyName="out", AdjustmentType="ChangeInCapacity", ScalingAdjustment=1))
        r.step("DescribePolicies", lambda: asg.describe_policies(AutoScalingGroupName=grp)["ScalingPolicies"][0])
        r.step("DeletePolicy", lambda: asg.delete_policy(AutoScalingGroupName=grp, PolicyName="out"))
        r.step("CreateOrUpdateTags", lambda: asg.create_or_update_tags(Tags=[{"ResourceId": grp, "ResourceType": "auto-scaling-group", "Key": "team", "Value": "infra", "PropagateAtLaunch": False}]))
        r.step("DescribeTags", lambda: asg.describe_tags(Filters=[{"Name": "auto-scaling-group", "Values": [grp]}])["Tags"])
        r.step("SetDesiredCapacity 0", lambda: asg.set_desired_capacity(AutoScalingGroupName=grp, DesiredCapacity=0))
        r.step("DescribeScalingActivities", lambda: asg.describe_scaling_activities(AutoScalingGroupName=grp))
    r.refused("MixedInstancesPolicy is refused, not dropped", lambda: asg.create_auto_scaling_group(
        AutoScalingGroupName=grp + "-m", MinSize=0, MaxSize=1, VPCZoneIdentifier=SUBNET,
        MixedInstancesPolicy={"LaunchTemplate": {"LaunchTemplateSpecification": {"LaunchTemplateName": "zz-compat-lt", "Version": "$Latest"}}}), ("UnsupportedOperation", "ValidationError"))

    if PROJECT:
        c.inject(ec2, "CreateLaunchTemplate", {"ProjectId": PROJECT})
        lt = r.step("CreateLaunchTemplate", lambda: ec2.create_launch_template(LaunchTemplateName=name, LaunchTemplateData={"ImageId": image})["LaunchTemplate"]["LaunchTemplateId"])
        if lt:
            r.undo("DeleteLaunchTemplate", lambda: ec2.delete_launch_template(LaunchTemplateId=lt))
            r.step("DescribeLaunchTemplates", lambda: ec2.describe_launch_templates(LaunchTemplateIds=[lt])["LaunchTemplates"][0])
            if r.step("CreateAutoScalingGroup (launch template)", lambda: asg.create_auto_scaling_group(
                    AutoScalingGroupName=lt_grp, LaunchTemplate={"LaunchTemplateId": lt, "Version": "$Latest"}, MinSize=0, MaxSize=1, VPCZoneIdentifier=SUBNET)):
                r.undo("DeleteAutoScalingGroup (launch template)", lambda: asg.delete_auto_scaling_group(AutoScalingGroupName=lt_grp, ForceDelete=True))
    else:
        r.skip("CreateLaunchTemplate", "MACHINA_PROJECT_ID not set")
finally:
    r.cleanup()
    print(f"{r.failed} failed")
sys.exit(r.failed)

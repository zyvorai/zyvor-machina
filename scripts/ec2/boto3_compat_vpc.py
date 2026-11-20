#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""End-to-end boto3 chain against Machina's EC2 API, one line per action: VPC, subnet, internet gateway, route table + route +
association, security group + rules, EBS volume, instance, volume attach/detach, tags, then teardown in reverse order.

    pip install boto3
    MACHINA_EC2_ENDPOINT=https://HOST:5093/ec2 AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... \
    MACHINA_PROJECT_ID=<project uuid> [MACHINA_EC2_VERIFY=false] ./scripts/ec2/boto3_compat_vpc.py

boto3 cannot name Machina's `ProjectId` on CreateVpc; this script adds it to that one request. The image, instance type and
availability zone are the first ones DescribeImages / DescribeInstanceTypes / DescribeAvailabilityZones report (override with
MACHINA_EC2_IMAGE, MACHINA_EC2_INSTANCE_TYPE, MACHINA_EC2_AZ). MACHINA_EC2_CIDR (default 10.213.0.0/16) is the VPC range:
pick one that is free. Everything created is tagged `machina-compat` and deleted in a finally block, including the instance.
Set MACHINA_COMPAT_NO_INSTANCE=1 to skip the instance and the volume attachment. Exit code is the number of FAIL lines.
"""
import os
import sys
import time

import _compat as c

PROJECT = os.environ.get("MACHINA_PROJECT_ID")
if not PROJECT:
    sys.exit("set MACHINA_PROJECT_ID (a project UUID; Platform -> Cloud -> Projects)")
CIDR = os.environ.get("MACHINA_EC2_CIDR", "10.213.0.0/16")
SUBNET_CIDR = CIDR.rsplit(".", 2)[0] + ".1.0/24" if CIDR.endswith(".0.0/16") else None
if SUBNET_CIDR is None:
    sys.exit("MACHINA_EC2_CIDR must be a /16 ending in .0.0/16, e.g. 10.213.0.0/16")
NO_INSTANCE = os.environ.get("MACHINA_COMPAT_NO_INSTANCE") == "1"

ec2 = c.client("ec2")
r = c.Run("boto3_compat_vpc")
image, itype, zone = c.discover()
r.expect("discover image / instance type / zone", bool(image and itype and zone), f"image={image!r} type={itype!r} zone={zone!r}")
if not (image and itype and zone):
    sys.exit(1)
c.inject(ec2, "CreateVpc", {"ProjectId": PROJECT, "AvailabilityZone": zone})
TAG = lambda kind: [{"ResourceType": kind, "Tags": [{"Key": "Name", "Value": "machina-compat"}]}]  # noqa: E731


def wait_running(iid):
    for _ in range(60):
        st = ec2.describe_instances(InstanceIds=[iid])["Reservations"][0]["Instances"][0]["State"]["Name"]
        if st == "running":
            return True
        time.sleep(3)
    raise RuntimeError("instance not running after 180s")


def wait_terminated(iid):
    for _ in range(60):
        if c.code_of(lambda: ec2.describe_instances(InstanceIds=[iid])) == "InvalidInstanceID.NotFound":
            return
        got = ec2.describe_instances(InstanceIds=[iid])["Reservations"]
        if not got or got[0]["Instances"][0]["State"]["Name"] == "terminated":
            return
        time.sleep(3)
    raise RuntimeError("instance not terminated after 180s")


try:
    r.refused("CreateVpc DryRun", lambda: ec2.create_vpc(CidrBlock=CIDR, DryRun=True), ("DryRunOperation",))
    vpc = r.step("CreateVpc", lambda: ec2.create_vpc(CidrBlock=CIDR, TagSpecifications=TAG("vpc"))["Vpc"]["VpcId"])
    if vpc:
        r.undo("DeleteVpc", lambda: ec2.delete_vpc(VpcId=vpc))
        r.step("DescribeVpcs", lambda: [v for v in ec2.describe_vpcs(VpcIds=[vpc])["Vpcs"] if v["VpcId"] == vpc][0])
        r.step("DescribeVpcAttribute", lambda: ec2.describe_vpc_attribute(VpcId=vpc, Attribute="enableDnsSupport"))

    subnet = None
    if vpc:
        subnet = r.step("CreateSubnet", lambda: ec2.create_subnet(VpcId=vpc, CidrBlock=SUBNET_CIDR, AvailabilityZone=zone)["Subnet"]["SubnetId"])
        if subnet:
            # the subnet's network is provisioned on the host; a launch into it is refused until DescribeSubnets says available
            r.step("subnet becomes available", lambda: ec2.get_waiter("subnet_available").wait(SubnetIds=[subnet], WaiterConfig={"Delay": 3, "MaxAttempts": 40}))
    else:
        r.skip("CreateSubnet", "no VPC")
    if subnet:
        r.undo("DeleteSubnet", lambda: ec2.delete_subnet(SubnetId=subnet))
        r.step("DescribeSubnets (filter vpc-id)", lambda: [s for s in ec2.describe_subnets(Filters=[{"Name": "vpc-id", "Values": [vpc]}])["Subnets"] if s["SubnetId"] == subnet][0])

    igw = r.step("CreateInternetGateway", lambda: ec2.create_internet_gateway(TagSpecifications=TAG("internet-gateway"))["InternetGateway"]["InternetGatewayId"])
    if igw:
        r.undo("DeleteInternetGateway", lambda: ec2.delete_internet_gateway(InternetGatewayId=igw))
        if vpc and r.step("AttachInternetGateway", lambda: ec2.attach_internet_gateway(InternetGatewayId=igw, VpcId=vpc)):
            r.undo("DetachInternetGateway", lambda: ec2.detach_internet_gateway(InternetGatewayId=igw, VpcId=vpc))
        r.step("DescribeInternetGateways", lambda: ec2.describe_internet_gateways(InternetGatewayIds=[igw])["InternetGateways"][0])

    rt = None
    if vpc:
        rt = r.step("CreateRouteTable", lambda: ec2.create_route_table(VpcId=vpc, TagSpecifications=TAG("route-table"))["RouteTable"]["RouteTableId"])
    if rt:
        r.undo("DeleteRouteTable", lambda: ec2.delete_route_table(RouteTableId=rt))
        if igw and r.step("CreateRoute 0.0.0.0/0 via igw", lambda: ec2.create_route(RouteTableId=rt, DestinationCidrBlock="0.0.0.0/0", GatewayId=igw)):
            r.undo("DeleteRoute", lambda: ec2.delete_route(RouteTableId=rt, DestinationCidrBlock="0.0.0.0/0"))
        if subnet:
            assoc = r.step("AssociateRouteTable", lambda: ec2.associate_route_table(RouteTableId=rt, SubnetId=subnet)["AssociationId"])
            if assoc:
                r.undo("DisassociateRouteTable", lambda: ec2.disassociate_route_table(AssociationId=assoc))
        r.step("DescribeRouteTables", lambda: ec2.describe_route_tables(RouteTableIds=[rt])["RouteTables"][0])

    sg = None
    if vpc:
        sg = r.step("CreateSecurityGroup", lambda: ec2.create_security_group(GroupName="machina-compat", Description="machina compat", VpcId=vpc)["GroupId"])
    if sg:
        r.undo("DeleteSecurityGroup", lambda: ec2.delete_security_group(GroupId=sg))
        r.step("AuthorizeSecurityGroupIngress", lambda: ec2.authorize_security_group_ingress(
            GroupId=sg, IpPermissions=[{"IpProtocol": "tcp", "FromPort": 22, "ToPort": 22, "IpRanges": [{"CidrIp": "10.0.0.0/8"}]}]))
        r.step("DescribeSecurityGroups", lambda: ec2.describe_security_groups(GroupIds=[sg])["SecurityGroups"][0])
        r.step("RevokeSecurityGroupIngress", lambda: ec2.revoke_security_group_ingress(
            GroupId=sg, IpPermissions=[{"IpProtocol": "tcp", "FromPort": 22, "ToPort": 22, "IpRanges": [{"CidrIp": "10.0.0.0/8"}]}]))

    vol = r.step("CreateVolume", lambda: ec2.create_volume(AvailabilityZone=zone, Size=1, TagSpecifications=TAG("volume"))["VolumeId"])
    if vol:
        r.undo("DeleteVolume", lambda: ec2.delete_volume(VolumeId=vol))
        r.step("DescribeVolumes", lambda: ec2.describe_volumes(VolumeIds=[vol])["Volumes"][0])

    iid = None
    if NO_INSTANCE:
        r.skip("RunInstances", "MACHINA_COMPAT_NO_INSTANCE=1")
    elif subnet:
        launch = dict(ImageId=image, InstanceType=itype, MinCount=1, MaxCount=1, SubnetId=subnet, TagSpecifications=TAG("instance"))
        iid = r.step("RunInstances", lambda: ec2.run_instances(**launch)["Instances"][0]["InstanceId"])
    else:
        r.skip("RunInstances", "no subnet")
    if iid:
        r.undo("wait for terminated", lambda: wait_terminated(iid))  # registered first, so it runs after the terminate
        r.undo("TerminateInstances", lambda: ec2.terminate_instances(InstanceIds=[iid]))
        r.step("DescribeInstances until running", lambda: wait_running(iid))
        r.step("DescribeInstanceAttribute instanceType", lambda: ec2.describe_instance_attribute(InstanceId=iid, Attribute="instanceType"))
        if vol:
            if r.step("AttachVolume", lambda: ec2.attach_volume(VolumeId=vol, InstanceId=iid, Device="/dev/vdb")):
                r.undo("DetachVolume", lambda: ec2.detach_volume(VolumeId=vol, InstanceId=iid, Force=True))
        r.step("CreateTags", lambda: ec2.create_tags(Resources=[iid], Tags=[{"Key": "compat", "Value": "1"}]))
        r.step("DescribeTags", lambda: [t for t in ec2.describe_tags(Filters=[{"Name": "resource-id", "Values": [iid]}])["Tags"] if t["Key"] == "compat"][0])
        r.step("StopInstances", lambda: ec2.stop_instances(InstanceIds=[iid]))
        r.step("StartInstances", lambda: ec2.start_instances(InstanceIds=[iid]))
finally:
    r.cleanup()
    print(f"{r.failed} failed")
sys.exit(r.failed)

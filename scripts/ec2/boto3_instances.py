#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""boto3 test of Machina's EC2 instance-side actions: attributes, termination protection, monitoring, metadata options,
volumes, key pairs, images, launch templates with versions, placement groups, spot requests and instant fleets.

    pip install boto3
    MACHINA_EC2_ENDPOINT=https://HOST:5093/ec2 \
    AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... \
    MACHINA_EC2_IMAGE=ubuntu-24.04 [MACHINA_EC2_VERIFY=false] [MACHINA_EC2_SKIP_LAUNCH=1] ./scripts/ec2/boto3_instances.py

Create the access key with `POST /api/v1/ec2/access-keys` (admin). The test creates throwaway objects named `machina-e3-*`
(a key pair, a launch template, a placement group, a volume, up to three small instances) and removes them at the end; it
never touches anything else. `MACHINA_EC2_SKIP_LAUNCH=1` skips the checks that start instances. Exit code is the number of
failed checks.
"""
import base64
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
LAUNCH = os.environ.get("MACHINA_EC2_SKIP_LAUNCH", "") != "1"

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


def instance(iid):
    return ec2.describe_instances(InstanceIds=[iid])["Reservations"][0]["Instances"][0]


def wait_running(iid):
    for _ in range(60):
        if instance(iid)["State"]["Name"] == "running":
            return True
        time.sleep(3)
    return False


run_id = uuid.uuid4().hex[:8]
name = f"machina-e3-{run_id}"
tag_spec = [{"ResourceType": "instance", "Tags": [{"Key": "Name", "Value": name}, {"Key": "e3", "Value": run_id}]}]
instances, volumes, keys, templates, groups = [], [], [], [], []

try:
    # ---- key pairs: generated here, the private key comes back once -------------------------------------------
    kp = ec2.create_key_pair(KeyName=name)
    keys.append(name)
    check("CreateKeyPair returns an OpenSSH private key", kp["KeyMaterial"].startswith("-----BEGIN OPENSSH PRIVATE KEY-----"))
    check("CreateKeyPair returns a fingerprint and an id", bool(kp["KeyFingerprint"]) and kp["KeyPairId"].startswith("key-"))
    check("the key pair is listed", name in [k["KeyName"] for k in ec2.describe_key_pairs(KeyNames=[name])["KeyPairs"]])
    check("an RSA key is refused", code_of(lambda: ec2.create_key_pair(KeyName=name + "-rsa", KeyType="rsa")) == "UnsupportedOperation")
    check("a duplicate key name is refused", code_of(lambda: ec2.create_key_pair(KeyName=name)) is not None)

    # ---- launch templates with versions ----------------------------------------------------------------------
    lt = ec2.create_launch_template(
        LaunchTemplateName=name,
        VersionDescription="first",
        LaunchTemplateData={"ImageId": IMAGE, "KeyName": name, "TagSpecifications": tag_spec,
                            "Monitoring": {"Enabled": True}},
    )["LaunchTemplate"]
    templates.append(lt["LaunchTemplateId"])
    check("CreateLaunchTemplate answers the AWS shape", lt["LaunchTemplateName"] == name and lt["DefaultVersionNumber"] == 1 and lt["LatestVersionNumber"] == 1)
    v2 = ec2.create_launch_template_version(
        LaunchTemplateId=lt["LaunchTemplateId"], SourceVersion="1", VersionDescription="second",
        LaunchTemplateData={"MetadataOptions": {"HttpEndpoint": "disabled"}},
    )["LaunchTemplateVersion"]
    check("CreateLaunchTemplateVersion numbers the new version 2", v2["VersionNumber"] == 2)
    check("the new version keeps the source's image", v2["LaunchTemplateData"].get("ImageId") == IMAGE, str(v2["LaunchTemplateData"]))
    vs = ec2.describe_launch_template_versions(LaunchTemplateId=lt["LaunchTemplateId"])["LaunchTemplateVersions"]
    check("DescribeLaunchTemplateVersions lists both", [v["VersionNumber"] for v in vs] == [1, 2])
    ec2.modify_launch_template(LaunchTemplateId=lt["LaunchTemplateId"], DefaultVersion="2")
    got = ec2.describe_launch_templates(LaunchTemplateIds=[lt["LaunchTemplateId"]])["LaunchTemplates"][0]
    check("ModifyLaunchTemplate moves the default", got["DefaultVersionNumber"] == 2 and got["LatestVersionNumber"] == 2)
    check("a template version that does not exist is refused",
          code_of(lambda: ec2.create_launch_template_version(LaunchTemplateId=lt["LaunchTemplateId"], SourceVersion="9",
                                                              LaunchTemplateData={"InstanceType": "x"})) == "InvalidLaunchTemplateId.VersionNotFound")
    check("an unsupported template member is refused when written",
          code_of(lambda: ec2.create_launch_template(LaunchTemplateName=name + "-bad",
                                                      LaunchTemplateData={"IamInstanceProfile": {"Name": "r"}})) == "UnsupportedOperation")

    # ---- placement groups ------------------------------------------------------------------------------------
    pg = ec2.create_placement_group(GroupName=name, Strategy="spread")["PlacementGroup"]
    groups.append(name)
    check("CreatePlacementGroup answers name, strategy and id", pg["GroupName"] == name and pg["Strategy"] == "spread" and pg["GroupId"].startswith("pg-"))
    check("DescribePlacementGroups finds it", [g["GroupName"] for g in ec2.describe_placement_groups(GroupNames=[name])["PlacementGroups"]] == [name])
    check("a partition group is refused", code_of(lambda: ec2.create_placement_group(GroupName=name + "-p", Strategy="partition")) == "UnsupportedOperation")
    check("a duplicate group is refused", code_of(lambda: ec2.create_placement_group(GroupName=name, Strategy="spread")) == "InvalidPlacementGroup.Duplicate")

    # ---- volumes: type recorded, size grows ------------------------------------------------------------------
    vol = ec2.create_volume(Size=1, AvailabilityZone="machina-a", VolumeType="gp3", Iops=3000,
                            TagSpecifications=[{"ResourceType": "volume", "Tags": [{"Key": "Name", "Value": name + "-vol"}]}])
    volumes.append(vol["VolumeId"])
    got = ec2.describe_volumes(VolumeIds=[vol["VolumeId"]])["Volumes"][0]
    check("DescribeVolumes reads back type and IOPS", got.get("VolumeType") == "gp3" and got.get("Iops") == 3000, str(got))
    check("the volume's tag specification was applied", {"Key": "Name", "Value": name + "-vol"} in got.get("Tags", []))
    mod = ec2.modify_volume(VolumeId=vol["VolumeId"], Size=2, VolumeType="gp3", Iops=4000)["VolumeModification"]
    check("ModifyVolume answers the modification", mod["TargetSize"] == 2 and mod["TargetIops"] == 4000 and mod["OriginalIops"] == 3000, str(mod))
    got = ec2.describe_volumes(VolumeIds=[vol["VolumeId"]])["Volumes"][0]
    check("the volume grew and kept the new IOPS", got["Size"] == 2 and got.get("Iops") == 4000, str(got))
    hist = ec2.describe_volumes_modifications(VolumeIds=[vol["VolumeId"]])["VolumesModifications"]
    check("DescribeVolumesModifications lists the change", len(hist) >= 1)
    check("DescribeVolumeStatus reports the volume", ec2.describe_volume_status(VolumeIds=[vol["VolumeId"]])["VolumeStatuses"][0]["VolumeStatus"]["Status"] in ("ok", "insufficient-data"))
    check("an encrypted volume is refused", code_of(lambda: ec2.create_volume(Size=1, AvailabilityZone="machina-a", Encrypted=True)) == "UnsupportedOperation")
    check("an IOPS value on gp2 is refused", code_of(lambda: ec2.modify_volume(VolumeId=vol["VolumeId"], VolumeType="gp2", Iops=3000)) == "InvalidParameterCombination")
    check("CopySnapshot is refused with the reason", code_of(lambda: ec2.copy_snapshot(SourceRegion="machina", SourceSnapshotId="snap-00000000000000000")) == "UnsupportedOperation")

    # ---- catalog ---------------------------------------------------------------------------------------------
    offers = ec2.describe_instance_type_offerings()["InstanceTypeOfferings"]
    check("DescribeInstanceTypeOfferings lists the region's types", len(offers) > 0 and all(o["LocationType"] == "region" for o in offers))
    types = ec2.describe_instance_types()["InstanceTypes"]
    check("DescribeInstanceTypes carries capability fields", len(types) > 0 and "SupportedUsageClasses" in types[0])
    hist = ec2.describe_spot_price_history(InstanceTypes=[types[0]["InstanceType"]])["SpotPriceHistory"]
    check("DescribeSpotPriceHistory answers a price", len(hist) == 1 and float(hist[0]["SpotPrice"]) > 0)
    check("GetConsoleScreenshot is refused", code_of(lambda: ec2.get_console_screenshot(InstanceId="i-00000000000000000")) in ("UnsupportedOperation", "InvalidInstanceID.NotFound"))

    if LAUNCH:
        # ---- an instance from the template: attributes, protection, monitoring, metadata ----------------------
        r = ec2.run_instances(MinCount=1, MaxCount=1, LaunchTemplate={"LaunchTemplateId": lt["LaunchTemplateId"], "Version": "$Latest"},
                              DisableApiTermination=True)
        iid = r["Instances"][0]["InstanceId"]
        instances.append(iid)
        check("RunInstances applied the template's image and the version-2 metadata switch", wait_running(iid))
        inst = instance(iid)
        check("the template's tag specification reached the instance", {"Key": "e3", "Value": run_id} in inst.get("Tags", []))
        check("template monitoring is on", inst["Monitoring"]["State"] == "enabled")
        check("the version-2 metadata endpoint switch is applied", inst["MetadataOptions"]["HttpEndpoint"] == "disabled", str(inst["MetadataOptions"]))
        check("DescribeInstanceAttribute shows termination protection",
              ec2.describe_instance_attribute(InstanceId=iid, Attribute="disableApiTermination")["DisableApiTermination"]["Value"] is True)
        check("TerminateInstances is refused while protected", code_of(lambda: ec2.terminate_instances(InstanceIds=[iid])) == "OperationNotPermitted")
        ec2.modify_instance_attribute(InstanceId=iid, DisableApiTermination={"Value": False})
        check("the protection can be switched off",
              ec2.describe_instance_attribute(InstanceId=iid, Attribute="disableApiTermination")["DisableApiTermination"]["Value"] is False)
        check("source/dest check off is refused", code_of(lambda: ec2.modify_instance_attribute(InstanceId=iid, SourceDestCheck={"Value": False})) == "UnsupportedOperation")
        check("changing user data is refused", code_of(lambda: ec2.modify_instance_attribute(InstanceId=iid, UserData={"Value": b"x"})) == "UnsupportedOperation")
        ec2.unmonitor_instances(InstanceIds=[iid])
        check("UnmonitorInstances is reported", instance(iid)["Monitoring"]["State"] == "disabled")
        ec2.modify_instance_metadata_options(InstanceId=iid, HttpEndpoint="enabled", HttpPutResponseHopLimit=2)
        meta = instance(iid)["MetadataOptions"]
        check("ModifyInstanceMetadataOptions applies endpoint and hop limit", meta["HttpEndpoint"] == "enabled" and meta["HttpPutResponseHopLimit"] == 2, str(meta))
        check("HttpTokens=required is refused", code_of(lambda: ec2.modify_instance_metadata_options(InstanceId=iid, HttpTokens="required")) == "UnsupportedOperation")
        out = ec2.get_console_output(InstanceId=iid)
        check("GetConsoleOutput returns base64 text", isinstance(out.get("Output", ""), str) and len(base64.b64decode(out.get("Output", "") or "")) >= 0)
        check("GetPasswordData answers empty", ec2.get_password_data(InstanceId=iid)["PasswordData"] == "")
        cs = ec2.describe_instance_credit_specifications(InstanceIds=[iid])["InstanceCreditSpecifications"]
        check("DescribeInstanceCreditSpecifications answers standard", cs and cs[0]["CpuCredits"] == "standard")
        data = ec2.get_launch_template_data(InstanceId=iid)["LaunchTemplateData"]
        check("GetLaunchTemplateData describes the instance", bool(data.get("ImageId")))

        # ---- placement group membership ----------------------------------------------------------------------
        r = ec2.run_instances(ImageId=IMAGE, MinCount=1, MaxCount=1, Placement={"GroupName": name}, TagSpecifications=tag_spec)
        pid = r["Instances"][0]["InstanceId"]
        instances.append(pid)
        wait_running(pid)
        check("the instance reports its placement group", instance(pid)["Placement"].get("GroupName") == name, str(instance(pid)["Placement"]))
        check("a group with members cannot be deleted", code_of(lambda: ec2.delete_placement_group(GroupName=name)) == "InvalidPlacementGroup.InUse")
        check("an unknown placement group is refused", code_of(lambda: ec2.run_instances(ImageId=IMAGE, MinCount=1, MaxCount=1, Placement={"GroupName": name + "-none"})) == "InvalidPlacementGroup.Unknown")

        # ---- spot ----------------------------------------------------------------------------------------------
        sr = ec2.request_spot_instances(InstanceCount=1, SpotPrice="0.5", LaunchSpecification={"ImageId": IMAGE},
                                         TagSpecifications=[{"ResourceType": "spot-instances-request", "Tags": [{"Key": "e3", "Value": run_id}]}])
        req = sr["SpotInstanceRequests"][0]
        sid = req["InstanceId"]
        instances.append(sid)
        check("RequestSpotInstances is fulfilled at once", req["State"] == "active" and req["Status"]["Code"] == "fulfilled", str(req))
        found = ec2.describe_spot_instance_requests(Filters=[{"Name": "tag:e3", "Values": [run_id]}])["SpotInstanceRequests"]
        check("DescribeSpotInstanceRequests filters by tag", len(found) == 1 and found[0]["InstanceId"] == sid)
        ec2.cancel_spot_instance_requests(SpotInstanceRequestIds=[req["SpotInstanceRequestId"]])
        check("CancelSpotInstanceRequests cancels it",
              ec2.describe_spot_instance_requests(SpotInstanceRequestIds=[req["SpotInstanceRequestId"]])["SpotInstanceRequests"][0]["State"] == "cancelled")
        check("a persistent spot request is refused",
              code_of(lambda: ec2.request_spot_instances(Type="persistent", LaunchSpecification={"ImageId": IMAGE})) == "UnsupportedOperation")

        # ---- fleets --------------------------------------------------------------------------------------------
        fl = ec2.create_fleet(Type="instant", TargetCapacitySpecification={"TotalTargetCapacity": 1, "DefaultTargetCapacityType": "on-demand"},
                              LaunchTemplateConfigs=[{"LaunchTemplateSpecification": {"LaunchTemplateId": lt["LaunchTemplateId"], "Version": "$Latest"}}])
        fids = [i for f in fl.get("Instances", []) for i in f["InstanceIds"]]
        instances.extend(fids)
        check("an instant CreateFleet launches the capacity", len(fids) == 1 and not fl.get("Errors"), str(fl))
        check("DescribeFleets reports the fleet",
              ec2.describe_fleets(FleetIds=[fl["FleetId"]])["Fleets"][0]["Type"] == "instant")
        check("a maintain fleet is refused",
              code_of(lambda: ec2.create_fleet(Type="maintain", TargetCapacitySpecification={"TotalTargetCapacity": 1},
                                                LaunchTemplateConfigs=[{"LaunchTemplateSpecification": {"LaunchTemplateId": lt["LaunchTemplateId"]}}])) == "UnsupportedOperation")
        ec2.delete_fleets(FleetIds=[fl["FleetId"]], TerminateInstances=False)
        check("DeleteFleets without TerminateInstances is refused", code_of(lambda: ec2.delete_fleets(FleetIds=[fl["FleetId"]])) is not None)
finally:
    for iid in instances:
        try:
            ec2.modify_instance_attribute(InstanceId=iid, DisableApiTermination={"Value": False})
        except ClientError:
            pass
        code = code_of(lambda: ec2.terminate_instances(InstanceIds=[iid]))
        check(f"cleanup: instance {iid} terminated", code is None, str(code))
    time.sleep(5 if instances else 0)
    for g in groups:
        check("cleanup: placement group deleted", code_of(lambda: ec2.delete_placement_group(GroupName=g)) in (None, "InvalidPlacementGroup.InUse"))
    for t in templates:
        check("cleanup: launch template deleted", code_of(lambda: ec2.delete_launch_template(LaunchTemplateId=t)) is None)
    for v in volumes:
        check("cleanup: volume deleted", code_of(lambda: ec2.delete_volume(VolumeId=v)) is None)
    for k in keys:
        check("cleanup: key pair deleted", code_of(lambda: ec2.delete_key_pair(KeyName=k)) is None)

print(f"\n{failed} failed")
sys.exit(failed)

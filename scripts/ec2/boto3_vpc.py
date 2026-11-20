#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""boto3 check of Machina's EC2 VPC networking actions: internet gateways, route tables and routes, network ACLs, DHCP
options, VPC attributes, security group rules, network interface addresses, DryRun and the refusals.

    pip install boto3
    MACHINA_EC2_ENDPOINT=https://HOST:5093/ec2 \
    AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... \
    MACHINA_EC2_VPC_ID=vpc-... MACHINA_EC2_SUBNET_ID=subnet-... [MACHINA_EC2_VERIFY=false] \
    ./scripts/ec2/boto3_vpc.py

Needs an existing VPC and a subnet in it (boto3 cannot pass Machina's `ProjectId` to CreateVpc). Everything the script
creates is tagged `machina-vpc-check` and removed at the end; it never changes the VPC or the subnet themselves (the
VPC's DHCP options and DNS hostname attribute are put back). Set MACHINA_EC2_ALLOCATION_ID (an Elastic IP allocation)
to also test NAT gateways. Exit code is the number of failed checks.
"""
import os
import sys
import uuid

try:
    import boto3
    from botocore.config import Config
    from botocore.exceptions import ClientError
except ImportError:
    sys.exit("boto3 is required: pip install boto3")

ENDPOINT = os.environ.get("MACHINA_EC2_ENDPOINT")
VPC = os.environ.get("MACHINA_EC2_VPC_ID")
SUBNET = os.environ.get("MACHINA_EC2_SUBNET_ID")
if not (ENDPOINT and VPC and SUBNET):
    sys.exit("set MACHINA_EC2_ENDPOINT, MACHINA_EC2_VPC_ID and MACHINA_EC2_SUBNET_ID")
ALLOCATION = os.environ.get("MACHINA_EC2_ALLOCATION_ID")
VERIFY = os.environ.get("MACHINA_EC2_VERIFY", "true").lower() != "false"

ec2 = boto3.client(
    "ec2",
    endpoint_url=ENDPOINT,
    region_name=os.environ.get("AWS_DEFAULT_REGION", "machina"),
    verify=VERIFY,
    config=Config(retries={"max_attempts": 1}),
)

failed = 0
cleanup = []  # callables run in reverse order at the end


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


def spec(kind):
    return [{"ResourceType": kind, "Tags": [{"Key": "Name", "Value": "machina-vpc-check"}]}]


def run():
    # ---- DryRun -------------------------------------------------------------------------------------------------
    check("DryRun on a create is DryRunOperation", code_of(lambda: ec2.create_internet_gateway(DryRun=True)) == "DryRunOperation")

    # ---- internet gateway -----------------------------------------------------------------------------------------
    igw = ec2.create_internet_gateway(TagSpecifications=spec("internet-gateway"))["InternetGateway"]["InternetGatewayId"]
    cleanup.append(lambda: ec2.delete_internet_gateway(InternetGatewayId=igw))
    got = ec2.describe_internet_gateways(InternetGatewayIds=[igw])["InternetGateways"]
    check("describe the new internet gateway by id", len(got) == 1 and got[0]["Attachments"] == [])
    tagged = ec2.describe_internet_gateways(Filters=[{"Name": "tag:Name", "Values": ["machina-vpc-check"]}])["InternetGateways"]
    check("filter internet gateways by tag", any(g["InternetGatewayId"] == igw for g in tagged))
    check("unknown filter is refused", code_of(lambda: ec2.describe_internet_gateways(Filters=[{"Name": "nonsense", "Values": ["x"]}])) == "InvalidParameterValue")
    attached = False
    if not ec2.describe_internet_gateways(Filters=[{"Name": "attachment.vpc-id", "Values": [VPC]}])["InternetGateways"]:
        ec2.attach_internet_gateway(InternetGatewayId=igw, VpcId=VPC)
        attached = True
        cleanup.append(lambda: ec2.detach_internet_gateway(InternetGatewayId=igw, VpcId=VPC))
        got = ec2.describe_internet_gateways(Filters=[{"Name": "attachment.vpc-id", "Values": [VPC]}])["InternetGateways"]
        check("attached gateway is found by attachment.vpc-id", any(g["InternetGatewayId"] == igw for g in got))
        check("attaching twice is Resource.AlreadyAssociated", code_of(lambda: ec2.attach_internet_gateway(InternetGatewayId=igw, VpcId=VPC)) == "Resource.AlreadyAssociated")
        check("deleting an attached gateway is DependencyViolation", code_of(lambda: ec2.delete_internet_gateway(InternetGatewayId=igw)) == "DependencyViolation")
    else:
        print("SKIP internet gateway attach: the VPC already has one")

    # ---- route table, routes, association -----------------------------------------------------------------------
    main = ec2.describe_route_tables(Filters=[{"Name": "vpc-id", "Values": [VPC]}, {"Name": "association.main", "Values": ["true"]}])["RouteTables"]
    check("the VPC has a main route table", len(main) == 1)
    if main:
        local = [r for r in main[0]["Routes"] if r.get("GatewayId") == "local"]
        check("the main route table has the local route", len(local) == 1)
    rtb = ec2.create_route_table(VpcId=VPC, TagSpecifications=spec("route-table"))["RouteTable"]["RouteTableId"]
    cleanup.append(lambda: ec2.delete_route_table(RouteTableId=rtb))
    check("describe the new route table", len(ec2.describe_route_tables(RouteTableIds=[rtb])["RouteTables"]) == 1)
    if attached:
        ec2.create_route(RouteTableId=rtb, DestinationCidrBlock="10.99.0.0/16", GatewayId=igw)
        routes = ec2.describe_route_tables(RouteTableIds=[rtb])["RouteTables"][0]["Routes"]
        check("the route to the gateway is listed", any(r["DestinationCidrBlock"] == "10.99.0.0/16" and r.get("GatewayId") == igw for r in routes))
        check("a duplicate route is RouteAlreadyExists", code_of(lambda: ec2.create_route(RouteTableId=rtb, DestinationCidrBlock="10.99.0.0/16", GatewayId=igw)) == "RouteAlreadyExists")
        check("filter by route.gateway-id", len(ec2.describe_route_tables(Filters=[{"Name": "route.gateway-id", "Values": [igw]}])["RouteTables"]) >= 1)
        ec2.delete_route(RouteTableId=rtb, DestinationCidrBlock="10.99.0.0/16")
        check("deleting a missing route is InvalidRoute.NotFound", code_of(lambda: ec2.delete_route(RouteTableId=rtb, DestinationCidrBlock="10.99.0.0/16")) == "InvalidRoute.NotFound")
    check("an IPv6 destination is refused, not ignored", code_of(lambda: ec2.create_route(RouteTableId=rtb, DestinationIpv6CidrBlock="::/0", GatewayId=igw)) == "UnsupportedOperation")
    assoc = ec2.associate_route_table(RouteTableId=rtb, SubnetId=SUBNET)["AssociationId"]
    found = ec2.describe_route_tables(Filters=[{"Name": "association.subnet-id", "Values": [SUBNET]}])["RouteTables"]
    check("the association is found by subnet", any(t["RouteTableId"] == rtb for t in found))
    check("a second association of the subnet is refused", code_of(lambda: ec2.associate_route_table(RouteTableId=rtb, SubnetId=SUBNET)) == "Resource.AlreadyAssociated")
    check("deleting an associated table is DependencyViolation", code_of(lambda: ec2.delete_route_table(RouteTableId=rtb)) == "DependencyViolation")
    ec2.disassociate_route_table(AssociationId=assoc)

    # ---- NAT gateway (needs an Elastic IP allocation) --------------------------------------------------------------
    if ALLOCATION:
        nat = ec2.create_nat_gateway(SubnetId=SUBNET, AllocationId=ALLOCATION, TagSpecifications=spec("natgateway"))["NatGateway"]["NatGatewayId"]
        cleanup.append(lambda: ec2.delete_nat_gateway(NatGatewayId=nat))
        check("describe the NAT gateway", ec2.describe_nat_gateways(NatGatewayIds=[nat])["NatGateways"][0]["State"] == "available")
    else:
        print("SKIP NAT gateway: set MACHINA_EC2_ALLOCATION_ID")

    # ---- network ACL ----------------------------------------------------------------------------------------------
    default = ec2.describe_network_acls(Filters=[{"Name": "vpc-id", "Values": [VPC]}, {"Name": "default", "Values": ["true"]}])["NetworkAcls"]
    check("the VPC has a default network ACL with its two final rules", len(default) == 1 and len(default[0]["Entries"]) >= 4)
    check("the subnet is associated with an ACL", len(ec2.describe_network_acls(Filters=[{"Name": "association.subnet-id", "Values": [SUBNET]}])["NetworkAcls"]) == 1)
    acl = ec2.create_network_acl(VpcId=VPC, TagSpecifications=spec("network-acl"))["NetworkAcl"]["NetworkAclId"]
    cleanup.append(lambda: ec2.delete_network_acl(NetworkAclId=acl))
    ec2.create_network_acl_entry(NetworkAclId=acl, RuleNumber=100, Protocol="tcp", RuleAction="allow", Egress=False, CidrBlock="10.0.0.0/8", PortRange={"From": 22, "To": 22})
    check("a duplicate entry is NetworkAclEntryAlreadyExists", code_of(lambda: ec2.create_network_acl_entry(NetworkAclId=acl, RuleNumber=100, Protocol="tcp", RuleAction="allow", Egress=False, CidrBlock="10.0.0.0/8", PortRange={"From": 22, "To": 22})) == "NetworkAclEntryAlreadyExists")
    ec2.replace_network_acl_entry(NetworkAclId=acl, RuleNumber=100, Protocol="tcp", RuleAction="deny", Egress=False, CidrBlock="10.0.0.0/8", PortRange={"From": 22, "To": 22})
    entries = ec2.describe_network_acls(NetworkAclIds=[acl])["NetworkAcls"][0]["Entries"]
    check("the replaced entry is a deny", any(e["RuleNumber"] == 100 and e["RuleAction"] == "deny" for e in entries))
    check("an ICMP type is refused, not ignored", code_of(lambda: ec2.create_network_acl_entry(NetworkAclId=acl, RuleNumber=101, Protocol="icmp", RuleAction="allow", Egress=False, CidrBlock="10.0.0.0/8", IcmpTypeCode={"Type": 8, "Code": 0})) == "UnsupportedOperation")
    check("the final deny rule cannot be deleted", code_of(lambda: ec2.delete_network_acl_entry(NetworkAclId=acl, RuleNumber=32767, Egress=False)) == "InvalidParameterValue")
    ec2.delete_network_acl_entry(NetworkAclId=acl, RuleNumber=100, Egress=False)

    # ---- DHCP options and VPC attributes --------------------------------------------------------------------------
    dopt = ec2.create_dhcp_options(DhcpConfigurations=[{"Key": "domain-name", "Values": ["machina.test"]}], TagSpecifications=spec("dhcp-options"))["DhcpOptions"]["DhcpOptionsId"]
    cleanup.append(lambda: ec2.delete_dhcp_options(DhcpOptionsId=dopt))
    check("describe the DHCP options", ec2.describe_dhcp_options(DhcpOptionsIds=[dopt])["DhcpOptions"][0]["DhcpConfigurations"][0]["Key"] == "domain-name")
    before = ec2.describe_vpcs(VpcIds=[VPC])["Vpcs"][0]["DhcpOptionsId"]
    ec2.associate_dhcp_options(DhcpOptionsId=dopt, VpcId=VPC)
    check("the VPC shows the associated DHCP options", ec2.describe_vpcs(VpcIds=[VPC])["Vpcs"][0]["DhcpOptionsId"] == dopt)
    check("deleting associated DHCP options is DependencyViolation", code_of(lambda: ec2.delete_dhcp_options(DhcpOptionsId=dopt)) == "DependencyViolation")
    ec2.associate_dhcp_options(DhcpOptionsId=before if before.startswith("dopt-") else "default", VpcId=VPC)
    check("DNS support is on", ec2.describe_vpc_attribute(VpcId=VPC, Attribute="enableDnsSupport")["EnableDnsSupport"]["Value"] is True)
    check("DNS support cannot be switched off", code_of(lambda: ec2.modify_vpc_attribute(VpcId=VPC, EnableDnsSupport={"Value": False})) == "UnsupportedOperation")
    ec2.modify_vpc_attribute(VpcId=VPC, EnableDnsHostnames={"Value": True})
    check("DNS hostnames can be recorded", ec2.describe_vpc_attribute(VpcId=VPC, Attribute="enableDnsHostnames")["EnableDnsHostnames"]["Value"] is True)
    ec2.modify_vpc_attribute(VpcId=VPC, EnableDnsHostnames={"Value": False})

    # ---- read-only describes and refusals --------------------------------------------------------------------------
    check("prefix lists describe empty", ec2.describe_prefix_lists()["PrefixLists"] == [])
    check("VPC endpoints describe empty", ec2.describe_vpc_endpoints()["VpcEndpoints"] == [])
    check("creating a VPC endpoint is refused", code_of(lambda: ec2.create_vpc_endpoint(VpcId=VPC, ServiceName="com.example.s3")) == "UnsupportedOperation")

    # ---- security group rules --------------------------------------------------------------------------------------
    name = "machina-vpc-check-" + uuid.uuid4().hex[:6]
    sg = ec2.create_security_group(GroupName=name, Description="vpc check")["GroupId"]
    cleanup.append(lambda: ec2.delete_security_group(GroupId=sg))
    ec2.authorize_security_group_ingress(GroupId=sg, IpPermissions=[{"IpProtocol": "tcp", "FromPort": 22, "ToPort": 22, "IpRanges": [{"CidrIp": "10.0.0.0/8", "Description": "ssh from the lab"}]}])
    rules = ec2.describe_security_group_rules(Filters=[{"Name": "group-id", "Values": [sg]}])["SecurityGroupRules"]
    check("the new rule is a described object with its description", len(rules) >= 1 and any(r.get("Description") == "ssh from the lab" for r in rules))
    rule = next((r for r in rules if not r["IsEgress"]), None)
    if rule:
        rid = rule["SecurityGroupRuleId"]
        ec2.modify_security_group_rules(GroupId=sg, SecurityGroupRules=[{"SecurityGroupRuleId": rid, "SecurityGroupRule": {"IpProtocol": "tcp", "FromPort": 2222, "ToPort": 2222, "CidrIpv4": "10.0.0.0/8", "Description": "moved"}}])
        again = ec2.describe_security_group_rules(SecurityGroupRuleIds=[rid])["SecurityGroupRules"][0]
        check("modify keeps the rule id and changes the port", again["FromPort"] == 2222 and again["Description"] == "moved")
        ec2.update_security_group_rule_descriptions_ingress(GroupId=sg, IpPermissions=[{"IpProtocol": "tcp", "FromPort": 2222, "ToPort": 2222, "IpRanges": [{"CidrIp": "10.0.0.0/8", "Description": "renamed"}]}])
        check("the description update is visible", ec2.describe_security_group_rules(SecurityGroupRuleIds=[rid])["SecurityGroupRules"][0]["Description"] == "renamed")

    # ---- network interface addresses -------------------------------------------------------------------------------
    eni = ec2.create_network_interface(SubnetId=SUBNET, Description="vpc check")["NetworkInterface"]["NetworkInterfaceId"]
    cleanup.append(lambda: ec2.delete_network_interface(NetworkInterfaceId=eni))
    assigned = ec2.assign_private_ip_addresses(NetworkInterfaceId=eni, SecondaryPrivateIpAddressCount=1)["AssignedPrivateIpAddresses"]
    check("a secondary address is assigned", len(assigned) == 1)
    listed = ec2.describe_network_interfaces(NetworkInterfaceIds=[eni])["NetworkInterfaces"][0]["PrivateIpAddresses"]
    check("the interface lists its primary and secondary addresses", len(listed) >= 2 and any(not a["Primary"] for a in listed))
    if assigned:
        ec2.unassign_private_ip_addresses(NetworkInterfaceId=eni, PrivateIpAddresses=[assigned[0]["PrivateIpAddress"]])
    ec2.modify_network_interface_attribute(NetworkInterfaceId=eni, Description={"Value": "renamed"})
    check("the interface attribute reflects the change", ec2.describe_network_interface_attribute(NetworkInterfaceId=eni, Attribute="description")["Description"]["Value"] == "renamed")
    check("source/destination check cannot be turned off", code_of(lambda: ec2.modify_network_interface_attribute(NetworkInterfaceId=eni, SourceDestCheck={"Value": False})) == "UnsupportedOperation")


try:
    run()
finally:
    for fn in reversed(cleanup):
        try:
            fn()
        except Exception as e:  # noqa: BLE001 - report and keep cleaning
            print("cleanup:", e)
print(f"{failed} failed")
sys.exit(failed)

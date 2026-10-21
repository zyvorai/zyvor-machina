#!/usr/bin/env python3
"""Small no-mock boto3 conformance driver for the EC2/ELBv2 endpoint."""
import os, sys
try:
    import boto3
except Exception as e:
    raise SystemExit(f"boto3 required: {e}")

endpoint=os.environ['MACHINA_EC2_URL'].rstrip('/')
common=dict(
    aws_access_key_id=os.environ['AWS_ACCESS_KEY_ID'],
    aws_secret_access_key=os.environ['AWS_SECRET_ACCESS_KEY'],
    region_name=os.environ.get('AWS_DEFAULT_REGION','machina'),
    verify=os.environ.get('AWS_CA_BUNDLE', True),
)
ec2=boto3.client('ec2', endpoint_url=endpoint + '/ec2', **common)
elb=boto3.client('elbv2', endpoint_url=endpoint + '/elbv2', **common)
checks=[]
def ok(name, f):
    try: f(); checks.append((name, True, ''))
    except Exception as e: checks.append((name, False, repr(e)))

ok('describe-vpcs', lambda: ec2.describe_vpcs(MaxResults=5))
ok('describe-route-tables', lambda: ec2.describe_route_tables(MaxResults=5))
ok('describe-network-acls', lambda: ec2.describe_network_acls(MaxResults=5))
ok('describe-peering', lambda: ec2.describe_vpc_peering_connections(MaxResults=5))
ok('describe-load-balancers', lambda: elb.describe_load_balancers(PageSize=5))
for n,p,e in checks: print(('PASS' if p else 'FAIL'), n, e)
if not all(p for _,p,_ in checks): sys.exit(1)

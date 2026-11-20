# Tutorial: EC2 in ten minutes

You will launch an instance with a key pair and a first-boot script, tag it, put an enforced firewall in front of it, give it a
disk, and then drive all of it from boto3. About 15 minutes on a lab host. Everything you create is prefixed `tut-` so you can
delete it at the end.

**You need:** a running controller (see [INSTALL](../INSTALL.md)), a template named `cirros` (any cloud image works) and an
operator token. The commands use the variables from the [guides](../guides/README.md): `API`, `TOKEN` and the `mc` alias.

## 1. Make an instance type and a key
```bash
mc -X POST $API/flavors -d '{"name":"tut-small","vcpus":1,"memory_mb":256,"disk_gb":1}'
ssh-keygen -t ed25519 -N '' -f ~/.ssh/tut_key
mc -X POST $API/keypairs -d "{\"name\":\"tut\",\"public_key\":\"$(cat ~/.ssh/tut_key.pub)\"}"
```

## 2. Launch with user data
```bash
mc -X POST $API/vms/from-template -d '{"name":"tut-web","template":"cirros","flavor_id":"<tut-small id>",
  "key_name":"tut","cloud_init_user_data":"#!/bin/sh\nmkdir -p /tmp/www && echo hello-from-tut > /tmp/www/index.html\ncd /tmp/www && busybox httpd -p 8080\n"}'
```
Wait until it is running and has an address (`mc $API/vms/tut-web`). You should see `state: running` and an IPv4 address.

## 3. Tag it
```bash
mc -X PUT $API/tags/vm/tut-web -d '{"tags":{"env":"tutorial"}}'
mc "$API/vms?tag_key=env&tag_value=tutorial"      # lists tut-web
```

## 4. Firewall it, safely
Create a group that allows only 8080 and ping, attach it, **preview**, then enforce:
```bash
mc -X POST $API/security-groups -d '{"name":"tut-web","description":"tutorial"}'
mc -X POST $API/security-groups/<id>/rules -d '{"direction":"ingress","protocol":"tcp","port_range":"8080","cidr":"0.0.0.0/0"}'
mc -X POST $API/security-groups/<id>/rules -d '{"direction":"ingress","protocol":"icmp","cidr":"0.0.0.0/0"}'
mc -X PUT  $API/vms/tut-web/security-groups/<id>
mc $API/security-groups/<id>/enforce-preview     # read the warnings: it will tell you if SSH would be cut off
mc -X PUT  $API/security-groups/<id>/mode -d '{"mode":"enforce"}'
```
From another machine on the same network: `curl http://<instance ip>:8080/` answers `hello-from-tut`, a connection to any other
port times out. `mc $API/security-groups/<id>` reports `enforcement.state: enforced` and says which host confirmed it. This is
the part to look at twice: the state is what the host reports, not what you asked for.

## 5. Give it a disk
```bash
mc -X POST $API/volumes -d '{"name":"tut-data","size_gb":1,"delete_on_termination":true}'
mc -X POST $API/volumes/<id>/attach -d '{"vm":"tut-web"}'
mc -X PUT  $API/volumes/<id>/iotune -d '{"read_iops":200,"write_iops":200}'
```
The guest sees a new `/dev/vdb`; the host shows the limit with `virsh blkdeviotune <domain> vdb`.

## 6. The same from boto3
```bash
mc -X POST $API/ec2/access-keys -d '{"description":"tutorial"}'     # note the key and the secret
```
```python
import boto3
ec2 = boto3.client("ec2", endpoint_url="https://HOST:5093/ec2", verify=False, region_name="machina",
                   aws_access_key_id="MCAK...", aws_secret_access_key="...")
r = ec2.describe_instances(Filters=[{"Name": "tag:env", "Values": ["tutorial"]}])
print([i["InstanceId"] for res in r["Reservations"] for i in res["Instances"]])
```

## You should see
`tut-web` running, tagged, answering only on 8080, with an enforced group and an I/O-limited disk, and boto3 returning its
`i-...` id.

## Clean up
```bash
mc -X DELETE $API/vms/tut-web
mc -X DELETE $API/security-groups/<id>
mc -X DELETE $API/flavors/<tut-small id>
mc -X DELETE $API/keypairs/tut
```
The volume was flagged `delete_on_termination`, so it goes with the instance; for the access key use
`DELETE /api/v1/ec2/access-keys/<id>`.

Next: [Scale and survive](03-scale-and-survive.md).

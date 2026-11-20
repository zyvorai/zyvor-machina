# Cookbook: common tasks

Short recipes. Each assumes the `API`, `TOKEN` and `mc` setup from the [guides](../guides/README.md). For a walk-through, see
the [tutorials](../tutorials/README.md) (including [FluxVM day two](../tutorials/07-fluxvm-day-two.md)).

## Resize an instance
`mc -X POST $API/vms/<name>/change-type -d '{"flavor_id":"<id>"}'`. It stops the instance cleanly, applies the new CPU and
memory, and starts it again if it was running. The disk is not grown. Instances managed by a group change through the launch template.

## Run several copies
`mc -X POST $API/vms/run-instances -d '{"name":"web","template":"cirros","count":3,"min_count":2}'` creates `web-1` to `web-3`.

## Find an instance by tag
`mc "$API/vms?tag_key=env&tag_value=prod"`, or in boto3 `Filters=[{"Name":"tag:env","Values":["prod"]}]`.

## Share an image with another project
`mc -X PUT $API/templates/<name>/<version>/visibility -d '{"visibility":"private"}'`, then
`mc -X PUT $API/templates/<name>/<version>/shares/<project>`.

## Give CI its own key
`mc -X POST $API/api-keys -d '{"name":"ci-lab","role":"operator","projects":["lab"]}'`. The key is shown once.

## Firewall an instance without locking yourself out
Create the group, attach it, run `GET .../enforce-preview`, read the warnings (especially "no inbound TCP 22"), then switch
the mode to `enforce`. If the host's enforcement restarts the state shows `auditing` for up to 30 seconds, then returns.

## Why can't I reach my instance?
1. Is it running and does it have an address? `mc $API/vms/<name>`.
2. Is a security group enforcing? `mc $API/security-groups/<id>`: check `enforcement.state` and the rules.
3. On an isolated cloud subnet the guest needs a default route via the subnet's `.1` address; Machina does not set it.
4. For an Elastic IP, is the address routed to the host, and is the association confirmed (`applied`)?

## Why is a load balancer member out of rotation?
`mc $API/load-balancers/<id>` shows each member's `health` and the last probe's detail. A member leaves after
`unhealthy_threshold` failed probes and returns after `healthy_threshold` good ones.

## Give a guest its identity
Read `http://169.254.169.254/latest/meta-data/instance-id` from inside the guest. Guests on isolated cloud subnets need a
route to that address via the subnet's `.1`.

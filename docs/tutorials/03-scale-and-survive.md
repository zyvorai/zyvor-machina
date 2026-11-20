# Tutorial: scale and survive

Build a small web tier that grows under load and keeps serving when a backend dies: a launch template, an instance group, an
alarm that scales it, and a load balancer that stops sending traffic to a member that stops answering. About 20 minutes.
Prerequisite: [EC2 in ten minutes](02-ec2-in-ten-minutes.md) (flavor `tut-small`, key pair `tut`).

## 1. A launch template and a group
```bash
mc -X POST $API/cloud/launch-templates -d '{"name":"tut-tpl","template":"cirros","flavor_id":"<tut-small id>",
  "cloud_init_user_data":"#!/bin/sh\nmkdir -p /tmp/www && hostname > /tmp/www/index.html\ncd /tmp/www && busybox httpd -p 8080\n"}'
mc -X POST $API/cloud/instance-groups -d '{"name":"tut-asg","launch_template_id":"<id>","min":1,"max":3,"desired":2}'
```
Within a minute the group has two members, named `asg-<id>-<slot>`.

## 2. A scaling alarm
```bash
mc -X POST $API/alarms -d '{"name":"tut-hot","subject":"group:<group id, 32 hex>","metric":"cpu_percent","statistic":"Average",
  "period_secs":60,"evaluation_periods":3,"comparator":"gt","threshold":70,
  "action":"scale_group","group_id":"<group id>","step":1,"cooldown_secs":300}'
```
The alarm starts as `INSUFFICIENT_DATA` until three one-minute windows have samples, then reads `OK`. Drive load on a member
(`while :; do :; done` in a guest shell) and it goes to `ALARM`, the group's `desired` rises by one (never above `max`), and the
event carries the reason.

## 3. A load balancer with a health check
Create a load balancer over the members (UI: Fleet Cloud > Load balancers, or `POST /api/v1/load-balancers`), then:
```bash
mc -X PUT $API/load-balancers/<id>/health-check -d '{"protocol":"http","port":8080,"path":"/",
  "interval_secs":5,"timeout_secs":2,"healthy_threshold":2,"unhealthy_threshold":2}'
mc $API/load-balancers/<id>        # every member: health healthy
```
`curl` the balancer address repeatedly: the page alternates between the members' hostnames.

## 4. Kill a backend
In one member's console run `pkill busybox`. After about `interval x unhealthy_threshold` (10 s) that member shows
`unhealthy`, the balancer stops sending it requests, and every `curl` still answers from the other member. Start `httpd` again
and it returns after two good probes. State changes appear as `lb.health` events.

## You should see
Alarm `OK`, then `ALARM` under load with the group one larger; an `unhealthy` member removed from rotation without a failed
request on the surviving one.

## Clean up
Scale to zero first, then delete: set the group's `min` and `desired` to 0, wait for members to stop, then
`DELETE /api/v1/cloud/instance-groups/<id>`, `DELETE /api/v1/cloud/launch-templates/<id>`, the alarm and the load balancer.

## Know the edges
The balancer is L4 with one health check per balancer, on one host; alarms notify nothing by themselves (use alert rules and
webhooks). See the [alarms](../guides/alarms-and-scaling.md) and [health check](../guides/load-balancer-health-checks.md)
guides. Status of each claim: [claims ledger](../claims.md).

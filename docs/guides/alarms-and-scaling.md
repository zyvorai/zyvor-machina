# Alarms, metric statistics and instance groups

## What it is
CloudWatch-style building blocks: statistics over stored metric samples, alarms that move between `OK`, `ALARM` and
`INSUFFICIENT_DATA`, and an alarm action that scales an instance group. Launch templates and instance groups give you
auto scaling; both can be deleted.

## Configure
Samples are collected by the controller's sampler (per VM, for example `cpu_percent`). Create a launch template and a group
(`/api/v1/cloud/launch-templates`, `/cloud/instance-groups`) with min, max and desired sizes. A group-wide alarm uses the
subject `group:<group id, 32 hex>`.

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

## Use
```bash
mc "$API/metrics/statistics?subject=web-1&metric=cpu_percent&period=300&statistics=Average,Maximum"
mc -X POST $API/alarms -d '{"name":"web-hot","subject":"group:<group id>","metric":"cpu_percent","statistic":"Average",
  "period_secs":60,"evaluation_periods":3,"comparator":"gt","threshold":70,
  "action":"scale_group","group_id":"<group id>","step":1,"cooldown_secs":300}'
mc -X PUT $API/alarms/<id>/enabled -d '{"enabled":true}'      # re-enabling resets it to INSUFFICIENT_DATA
mc -X DELETE $API/cloud/instance-groups/<id>                    # once no member is running
mc -X DELETE $API/cloud/launch-templates/<id>                   # once no group uses it
```
The result of a scale action is clamped to the group's min and max.

## Check it works
Drive load on a member, watch the alarm go to `ALARM` (events carry the reason), and confirm the group's desired size grew by
`step`. Unit tests: `cargo test -p machina-controller engine::alarms api::alarms metric_stats groups_and_unused_templates`.

## Limits
Statistics cover 15 days and 1440 datapoints per call, with no custom dimensions or units. Alarms have no notification actions
(use alert rules and webhooks) and `INSUFFICIENT_DATA` triggers nothing. Deleting a group keeps its stopped instances.

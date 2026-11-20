# Load balancer health checks

## What it is
Active health checking for the L4 load balancer. The agent on the balancer's host probes every member (a TCP connect, or an
HTTP GET that must answer 2xx or 3xx) and takes failing members out of the rule set until they recover.

## Configure
`PUT /api/v1/load-balancers/{id}/health-check` with `protocol` (`tcp`, `http`, `none`), optional `port` and `path`,
`interval_secs` (5-300), `timeout_secs`, `healthy_threshold` and `unhealthy_threshold`. `none` is the default and keeps every
member in rotation. Changing the check resets members to `unknown`.

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

## Use
```bash
mc -X PUT $API/load-balancers/<id>/health-check -d '{"protocol":"http","port":8080,"path":"/healthz",
  "interval_secs":10,"timeout_secs":3,"healthy_threshold":2,"unhealthy_threshold":3}'
mc $API/load-balancers/<id>          # each member: health = unknown | healthy | unhealthy, with the last probe's detail
```
A member turns unhealthy after `unhealthy_threshold` consecutive failures and healthy again after `healthy_threshold`
successes. State changes are events of kind `lb.health`.

## Check it works
Stop the service on one backend: after about `interval x unhealthy_threshold` seconds the member shows `unhealthy` and requests
to the VIP stop reaching it. Start it again and it returns. Unit tests: `cargo test -p machina-controller lb_health` and
`cargo test -p machina-agent lbprobe`. The Rivora (eBPF/XDP) balancer evaluation is in `docs/cloud-lb-rivora-spike.md`.

## Limits
One check per balancer, L4 only (no target groups or path routing). Probes run from the host that owns the balancer, the only
place that can reach private guest addresses. A member never probed stays in rotation.

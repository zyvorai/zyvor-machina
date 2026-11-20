# Security groups: audit, preview, enforce

## What it is
Stateful firewall rules attached to instances. Every group starts in `audit` mode (rules are recorded, nothing is blocked).
Switching a group to `enforce` compiles its rules into a VM-edge network policy (eBPF on the instance's tap) that is
default-deny in both directions, with replies to allowed connections passing through connection tracking.

## Configure
1. Create a group; new groups get an allow-all egress rule, as in EC2.
2. Add rules: direction, protocol (tcp, udp, icmp, icmpv6, all), a port or range, and a peer that is a CIDR **or** another group
   (`remote_sg_id`, meaning every instance carrying that group).
3. Attach the group to instances.
Enforcement needs `machina-bpfd` running on the instance's host. It is lease-gated and fails open: the controller renews the
lease every 30 s (`MACHINA_BPF_ENFORCE_LEASE_SECS`, default 900).

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

## Use
```bash
mc -X POST $API/security-groups -d '{"name":"web","description":"web tier"}'
mc -X POST $API/security-groups/<id>/rules -d '{"direction":"ingress","protocol":"tcp","port_range":"22","cidr":"10.0.0.0/8"}'
mc -X PUT  $API/vms/web-1/security-groups/<sg id>              # attach
mc $API/security-groups/<id>/enforce-preview                   # dry run: rule counts and lockout warnings per instance
mc -X PUT  $API/security-groups/<id>/mode -d '{"mode":"enforce"}'
mc $API/security-groups/<id>                                   # enforcement.state: what the hosts report
```
`enforcement.state` is `advisory`, `pending` (nothing attached), `enforced`, `auditing` (the host is not enforcing) or `failed`.

## Check it works
From another instance, probe an allowed and a denied port on a guest in the group; the denied one must time out and the allowed
one answer. Restart `machina-bpfd` on the host: state drops to `auditing` and returns to `enforced` within one 30 s tick.
Unit tests: `cargo test -p machina-controller sg_enforce`.

## Limits
Groups attach to instances, not to network interfaces. Enforce mode is per host, so other VM network policies on that host
are enforced too. Enforcing is refused while an attached instance has no known address. The preview warns but cannot see the
controller's own management paths: allow what you need (usually TCP 22) explicitly.

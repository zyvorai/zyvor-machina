# 30-day evaluation guide

A plan for proving Machina on your own hardware and workload. It uses only things you can check yourself. The gate for going
live is [site readiness](../CUSTOMER_SITE_READINESS.md); claims are tracked in the [ledger](../claims.md).

| Week | Goal | You do | Success looks like |
|---|---|---|---|
| 1 | Running and familiar | Install on one Linux KVM host ([INSTALL](../INSTALL.md)); create a VM; open its console; run [tutorial 02](../tutorials/02-ec2-in-ten-minutes.md) | VM reachable, console in the browser, `boto3.describe_instances` returns your tagged instance |
| 2 | Your workload | Import or rebuild 3-5 representative VMs; launch them from templates with your key pairs and user data | Each boots, is reachable and survives a stop/start; you know the per-VM effort |
| 3 | Safety and scale | Firewall them with security groups (preview, then enforce); run [tutorial 03](../tutorials/03-scale-and-survive.md) with your own service | Allowed traffic flows, denied traffic is blocked, a killed backend leaves rotation, status matches reality after restarting `machina-bpfd` |
| 4 | Operations and decision | Back up and restore a VM; add a second host if you will use a fleet; give a team a scoped API key; review audit logs | You have restore times, a list of gaps, and a go/no-go against your own criteria |

## Agree before you start
1. The workloads and the success numbers that matter to you (restore time, launch time, effort per VM).
2. What counts as a blocker. Known limits today are listed in the [ledger](../claims.md) (VPC internet-gateway egress not yet run; host-loss drill
   needed for multi-host HA; qualification scripts in `scripts/qualification/` produce the evidence).
3. Who at your side owns each week.

## Do not
Test on production uplinks, or power off hosts you cannot afford to lose. Enforcement is lease-gated for a reason, but a
network test on the wrong interface is still a bad day.

## After the 30 days
A one-page write-up: what you measured, what was missing, what it would cost under the [subscription model](../SUBSCRIPTION-MODEL.md).

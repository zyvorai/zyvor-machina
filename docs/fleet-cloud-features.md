# Fleet Cloud: features OpenStack doesn't have

Fleet Cloud covers the usual self-service primitives (flavors, images,
instances, volumes, security groups, stacks, load balancers). This page
describes what goes past them. Each feature runs on the controller and the
host agents that are already deployed; none needs an extra service.

## Scale to zero

An idle VM can sleep: its memory is saved to disk (libvirt managed save),
the host gets its RAM and vCPUs back, and the first packet sent to one of
its addresses restores it. A client's TCP connection usually just takes a
few seconds longer, because the kernel retransmits the SYN while the VM is
restored. OpenStack's closest equivalent, shelve, has to be undone by hand
and takes minutes.

### How it works

1. **Sleep.** `POST /api/v1/vms/{id}/sleep` (or auto-sleep, below) sends a
   `managedsave` power action to the agent. Once the save succeeds, the
   controller sets `desired_state = 'sleeping'` and `lifecycle_phase =
   'sleeping'`. Reconcile never restarts a sleeping VM.
2. **Wake set.** The controller pushes every sleeping VM on a host, with
   its guest addresses, to that host's `machina-bpfd` (`vm_wake_set`).
   bpfd installs two nftables tables called `machina_wake`, which count
   packets without changing them:
   - `inet machina_wake` uses the forward and output hooks. It sees routed
     traffic, port-forwarded traffic (floating IPs), overlay traffic after
     DNAT, and connections the host opens itself.
   - `bridge machina_wake` sees ARP requests for the VM's IPv4 address
     from other VMs on the same bridge.

   bpfd also pins the host's neighbour entry for each sleeping address
   (`nud permanent`, with the VM's MAC). Without the pin, the host's ARP
   for a VM that can't answer fails after about 3 s and the sender gets
   "no route to host" before the restore finishes. The pin goes back to
   `stale` when the VM leaves the wake set.
3. **Wake.** While anything is asleep, bpfd reads the counters every
   250 ms. When a counter moves, it publishes a `vm_wake` event. It repeats
   the event every 3 s for up to a minute, so a subscriber that fell behind
   still gets it. The agent subscribes to `vm_wake` and restores the domain
   (`virDomainCreate` on the managed save). It only acts when the domain is
   off *and* has a managed save, so an out-of-date wake set can never boot
   a VM that someone shut down on purpose.
4. **Back to running.** When inventory reports the VM running while its
   desired state is still `sleeping`, the controller sets it back to
   `running`, records a `wake (traffic)` event and removes the VM from the
   wake set.

A sleeping VM has no tap, so the tap eBPF programs can't see traffic for
it. That is why the hook is in nftables and not on the tap.

### Auto-sleep

A VM is idle when guest CPU stays below 3 % and its NICs move less than
2 KiB/s. ARP, NTP and DHCP chatter stays below that. Every inventory sample
that shows activity stamps `last_active_at`. Once a VM has been idle for
its policy's number of minutes, the controller sends it to sleep. The idle
check runs every 30 s.

Policy, from most to least specific:

| Setting | Meaning |
|---|---|
| VM `sleep_after_minutes = N` (5–10080) | Sleep after N idle minutes |
| VM `sleep_after_minutes = 0` | Never auto-sleep, whatever the project says |
| VM `sleep_after_minutes = null` | Use the project default |
| Project default | `PUT /api/v1/sleep/policies/{project}` |

The controller only auto-sleeps a VM whose guest address it knows (from the
guest agent, the host's ARP and NDP tables, or tap traffic). Without one,
traffic couldn't wake it.

### API

| Route | |
|---|---|
| `POST /api/v1/vms/{id}/sleep` | Sleep now (the VM must be running) |
| `POST /api/v1/vms/{id}/wake` | Restore now |
| `GET/PUT /api/v1/vms/{id}/sleep-policy` | `{ "sleep_after_minutes": 30 \| 0 \| null }`; the response also has idle minutes, whether traffic can wake the VM, and its last 20 sleep/wake events |
| `GET /api/v1/sleep/policies`, `PUT/DELETE /api/v1/sleep/policies/{project}` | Project defaults |
| `GET /api/v1/sleep/summary` | Sleeping VMs, RAM and vCPUs handed back, sleeps and wakes in the last 24 h |

bpfd (local socket): `vm_wake_set {config: {entries: [{vm, addresses}]}}`,
`vm_wake_status`, and the `vm_wake` stream topic (`{vm, address, at}`).

### CLI and UI

```bash
machinactl vm sleep web-1
machinactl vm wake web-1
machinactl vm sleep-policy web-1 30          # or: inherit | never
machinactl vm sleep-policy --project dev 60  # project default (never | clear)
machinactl vm sleeping                       # what's asleep, RAM handed back
```

The Fleet Cloud instance list has a **Sleeping** filter and Sleep/Wake
buttons. The instance detail page has a **Scale to zero** card for the
policy, idle time and history.

### Limits

- On the same bridge, IPv6 neighbour solicitations don't wake the VM yet.
  Routed IPv6 traffic does.
- Wake-on-traffic needs the wake hook on the VM's own host. The overlay
  covers this: traffic from other hosts is DNATed on the VM's host and
  goes through the forward hook there.
- Anything sent to the address wakes the VM, including a monitoring probe
  or the retransmissions of a TCP connection that was open when the VM went
  to sleep. Leave auto-sleep off (`0`) for VMs that are probed constantly.
- The save image is as large as the guest's RAM and is written to
  libvirt's managed-save directory (`/var/lib/libvirt/qemu/save`).

### Tests

- `scripts/bpf/vm-edge-smoke.sh`: the wake tables on veth/netns. A packet
  sent by the host, a packet routed from another namespace and an ARP
  request from a bridge peer each publish `vm_wake`. An address claimed by
  two VMs is rejected, and an empty set removes the tables.
- `scripts/bpf/vm-netpol-realvm.sh`, "fleet: scale to zero": sleeps a real
  VM, checks the managed save and the wake set, wakes it with an HTTP
  request from the host and again from the other VM, and checks that the
  controller returns it to `running`. It also checks that the host's
  neighbour entry is pinned while the VM sleeps and released on wake.

## Time travel

Every instance has a timeline of restore points. You can fork a running
VM into a second VM in seconds without stopping it, fork from any earlier
point, or rewind the VM to a point. A fork can also copy the source's RAM,
so the copy carries on from the same running processes on an isolated
network: a safe place to debug a live incident. OpenStack snapshots are
full image uploads to Glance, and a server can't be rewound in place.

### How it works

- **Restore point.** The agent takes an external, disk-only snapshot of
  every disk at once, with the guest's filesystems frozen through the guest
  agent when it is installed. The current disk file becomes a read-only
  layer and the VM keeps running on a new overlay
  (`{vm}-{disk}.rp-<time>-<id>.qcow2` next to the original). Nothing is
  copied, so it takes about a second whatever the disk size.
- **Fork.** The fork gets its own thin overlays on the source's frozen
  layers and a new domain: a new name, UUID and MACs. By default it also
  gets a new cloud-init seed, so it boots with its own instance-id,
  hostname and machine-id, and requests DHCP with its MAC as the client
  identifier so it never claims the source's lease. Forking "now" takes a
  restore point of the source first.
- **Memory fork.** The agent takes an external snapshot that includes
  memory, rewrites the domain XML inside the save image (new name and UUID,
  disks on the fork's overlays) and restores it as the fork. The fork keeps
  the source's MACs, because its running kernel still uses them, so it is
  attached to `machina-fork`, a libvirt network with no uplink and no
  addresses. Reach it through the console or the guest agent.
- **Rewind.** The VM is stopped, its overlays newer than the point are
  deleted, and it restarts on a fresh overlay on the point's layers. Later
  restore points go with them. A rewind is refused while a fork depends on
  a later point; detach the fork first.
- **Detach.** Copies the source's layers into the fork (a live block pull,
  or `qemu-img rebase` when the fork is off), so the fork no longer depends
  on the source.
- **Scheduled points.** With a schedule, the controller takes a point every
  N minutes and keeps the newest K. Older points are merged away with a
  live block commit, so the backing chain stays the same length. Merging is
  paused for a VM while it has forks, because they sit on its layers.

### API

| Route | |
|---|---|
| `GET /api/v1/vms/{id}/restore-points` | Points (oldest first), schedule, forks of this VM and what it was forked from |
| `POST /api/v1/vms/{id}/restore-points` | `{ "note": "before upgrade" }`; returns a task |
| `PUT /api/v1/vms/{id}/restore-points/policy` | `{ "every_minutes": 60, "keep": 24 }`; `0` turns the schedule off. Range 5–10080 minutes, keep 1–168 |
| `POST /api/v1/vms/{id}/restore-points/{point}/rewind` | 409 `fork_pins_later_point` if a fork depends on a later point |
| `POST /api/v1/vms/{id}/fork` | `{ "name", "restore_point_id"?, "memory", "isolate", "reseed", "start" }`; memory forks need a running VM and no point |
| `POST /api/v1/vms/{id}/fork/detach` | Called on the fork |

All of them except GET run as tasks (`vm.restore_point`, `vm.rewind`,
`vm.fork`, `vm.fork.detach`).

### CLI and UI

```bash
machinactl vm restore-point web-1 --note "before upgrade" --wait
machinactl vm restore-points web-1                    # list; 1 = oldest, -1 = newest
machinactl vm restore-points web-1 --every 60 --keep 24
machinactl vm fork web-1 web-1-debug --wait           # a copy of web-1 as it is now
machinactl vm fork web-1 web-1-old --at 1 --wait      # from the oldest point
machinactl vm fork web-1 web-1-live --memory --wait   # with RAM, isolated network
machinactl vm rewind web-1 -1 --wait
machinactl vm fork-detach web-1-debug --wait
```

The instance detail page has a **Time travel** card: a slider over the
restore points, Rewind here, Fork now or Fork here, the schedule, and the
VM's forks.

### Limits

- Disks must be file-backed qcow2 or raw. Atlas (RBD) disks and LVM
  volumes aren't supported yet.
- Restore points are on the same storage as the VM; they don't replace
  backups.
- Without the guest agent, points are crash-consistent rather than
  quiesced.
- Deleting a VM keeps its disk files (the existing delete policy), so a
  fork that hasn't been detached keeps working after its source is deleted.
- A memory fork can't join the source's network while the source runs,
  because they share MACs and addresses.

### Tests

- `scripts/fleet/vm-timetravel-realvm.sh` runs a real Ubuntu VM: a
  quiesced point; a live fork with its own MAC, address, hostname and
  machine-id, and writes that stay apart; a refused rewind, detach, then a
  rewind; a memory fork that still has a tmpfs file and can't reach the
  libvirt network; and a 5-minute schedule keeping 2 points while the
  chain doesn't grow.
- `web/e2e/fleet-cloud-time-travel.spec.ts` (mocked): the slider, rewind,
  fork from a point, memory fork, schedule and create.

## Stacks you describe

Describe a stack in a sentence ("3 web servers behind the internet on
443, a postgres they can reach on 5432, nightly backups") and Machina
drafts the template, shows what it would do, and deploys it after an
approval. Once it's running, the controller checks the stack against the
template every minute and puts back anything that drifted. Heat needs a
hand-written HOT template, and it doesn't notice when someone deletes a
server behind its back.

### Template

A stack template has instance groups and policies, alongside the older
`vms` / `volumes` / `security_groups` lists:

```json
{
  "instances": [
    { "name": "web", "count": 3, "flavor": "m1.small", "image": "ubuntu-24.04",
      "anti_affinity": true, "sleep_after_minutes": 30 },
    { "name": "db", "count": 1, "flavor": "m1.medium", "ha": true,
      "restore_points": { "every_minutes": 60, "keep": 24 },
      "backup": { "interval_hours": 24, "retain": 7 } }
  ],
  "policies": [
    { "from": "internet", "to": "web", "ports": [443] },
    { "from": "web", "to": "db", "ports": [5432] }
  ]
}
```

- A group makes `count` VMs named `{stack}-{group}-{i}` (just
  `{stack}-{group}` when `count` is 1), labelled `machina.io/stack` and
  `machina.io/stack-group`. `flavor` or `vcpus` / `memory`, `image` (an
  approved template), `network`, `labels`, `anti_affinity` (spread over
  hosts), `ha`, `sleep_after_minutes` ([scale to zero](#scale-to-zero)),
  `restore_points` ([time travel](#time-travel)) and `backup` are all
  applied when the VMs are made.
- Policies are compiled into one VM network policy per target group
  (`stack-{stack}-{group}`, ingress). `from` is a group, `internet`,
  `fleet`, `host`, `any` or a CIDR.
- Up to 20 VMs per group and 40 per stack.

### Draft, plan, approve

- **Draft.** `POST /api/v1/stacks/draft` sends the description to the
  configured LLM provider with the catalogue (flavors and images). The
  reply is validated; if it's wrong the model gets one repair round with
  the errors. With no provider, or if the model still gets it wrong, a
  rule-based parser drafts it from the words it knows (roles such as web,
  api, worker, db, cache and lb; counts; "backups", "ha", "dev"). The response says which one wrote it.
- **Plan.** `POST /api/v1/stacks/plan` is a dry run: project quota for
  every new VM at once, a placement simulation (memory and anti-affinity
  as the VMs pile up), a monthly cost from the cluster's FinOps rates, the
  compiled policies replayed against the flows the fleet actually saw, and
  for an update the VMs it would create, delete or resize. A plan that
  fails quota, can't place a VM, or has template errors is blocked.
- **Approve.** `POST /api/v1/stacks/propose` files a `stack.deploy`
  action with the plan's summary. Approving it creates or updates the
  stack; the action's verify step waits for the result, and undo deletes
  a new stack or puts the previous template back.

### Drift and healing

Every minute (on the leader), each created stack with groups or policies
is compared with its template: missing VMs, VMs that shouldn't
be there, wrong size, missing labels, missing or changed policies and
backup schedules. With auto-heal on (the default) the controller fixes
what it can: it recreates missing VMs and puts labels, policies and
schedules back. A different size is reported, not fixed,
because it needs a restart. The result is stored on the stack.

An update (`PUT /api/v1/stacks/{id}`) applies the new template the same
way. If it fails, the VMs it added are deleted and the previous template
is applied again.

### API

| Route | |
|---|---|
| `POST /api/v1/stacks/draft` | `{ "prompt", "name"?, "project_id"? }` → template, source (`llm` / `rules`), plan |
| `POST /api/v1/stacks/plan` | `{ "name", "template", "project_id"?, "stack_id"? }`; `stack_id` plans an update |
| `POST /api/v1/stacks/propose` | Same body plus `prompt`; 409 `stack_plan_blocked` with the plan when blocked |
| `PUT /api/v1/stacks/{id}` | `{ "template" }`; applies now (operators) |
| `GET /api/v1/stacks/{id}/drift` | Checks now and returns `{ in_sync, open, items }` |
| `POST /api/v1/stacks/{id}/converge` | Heals now |
| `PUT /api/v1/stacks/{id}/auto-heal` | `{ "enabled": false }` to report only |

### UI

Fleet Cloud → Stacks opens with **Compose stack**: describe it, Draft
plan, edit the template if you want, Propose for approval, then approve.
The plan shows cost, quota, placement, the policy graph and the VMs. Stack
detail has a Drift tab (check, converge, auto-heal) and a Template tab
(plan an update, propose it, or apply now).

### Limits

- Resizes are reported but not applied; stop the VM and resize it.
- Stacks made from the older `vms` list aren't reconciled.
- Deleting a stack deletes its VMs, policies and backup schedules; VM
  disks follow the usual delete policy.

### Tests

- `scripts/fleet/stack-realvm.sh` on real VMs: draft from a sentence,
  plan, a blocked proposal, approve and deploy three VMs with labels and a
  policy, delete a VM and the policy behind the stack's back and watch
  drift report them and converge put them back, scale a group up with an
  update, then undo the deploy.
- Controller unit tests: the template compiler and diff, the rules
  drafter, plan quota/placement/cost, drift on a stack whose VMs are
  missing, a blocked and a queued proposal, and `stack.deploy` undo.
- `web/e2e/fleet-cloud-stacks.spec.ts` (mocked): compose, plan, propose
  and approve; drift, converge and auto-heal on stack detail.

## Autopilot capacity

Capacity follows real demand instead of a number someone typed once.
Elastic instance groups scale ahead of the daily or weekly rush, drain from
their load balancer before they shrink, and sleep instead of stopping so the
next scale-out takes seconds. Every running VM gets a size suggestion from
its own history, applied through an approval that is checked afterwards and
can be undone. Under-used hosts are emptied so they can be powered down.
OpenStack splits this across Senlin or Heat autoscaling (reactive alarms
only), Watcher (separate service) and manual resizes.

### Instance groups

These build on the [elastic instance groups](cloud-vpc-elastic-compute.md)
of cloud VPCs. Their scaling policy takes four more optional fields:

| Field | |
|---|---|
| `predictive` | Raise the group before a recurring rush (needs `target_cpu`) |
| `scale_in` | `stop` (default; disks kept) or `sleep` (memory saved, wakes in seconds) |
| `load_balancer` | `{ "id", "port" }`: a native load balancer on the VPC's host, in the group's project or unscoped |
| `drain_secs` | Seconds a member stays out of the load balancer before it stops or sleeps; default 30, at most 900 |

Every 30 seconds, for each group:

1. **Desired count.** The CPU step from the operator guide, then, with
   `predictive`, raised to what the forecast for the coming hour needs at
   the target (summed CPU ÷ target, within min and max). The forecast only
   adds capacity. While the rush is expected, idle CPU doesn't step the
   group back down.
2. **Members below the count** are set to run. A sleeping one is restored
   from its saved memory and keeps its address. Once it is running with an
   address it joins the load balancer.
3. **Members above the count** leave the load balancer first. A running one
   is then marked draining. After `drain_secs` it sleeps (with
   `scale_in: sleep` and a known address) or stops.

A load balancer that can't take the new rule set is marked `error`, but
the group still scales.

### History and forecast

Each metrics pass also records disk IOPS, network bytes and, per group, the
CPU summed over its running members. Samples are rolled up into hourly
averages and peaks (`metric_hourly`), with network bytes turned into a rate.
The rollup is kept for 35 days.

The forecast for an hour is the mean of the same hour in up to 4 past weeks
when at least 2 exist. Otherwise it is the mean of the same hour over up to
7 past days, when at least 3 exist. With less history there is no forecast,
and the group scales on CPU alone.

### Rightsizing

A running VM with at least 72 hourly points in the last 14 days gets a
suggestion from the 95th percentile of its hourly peaks:

- **vCPUs:** enough to run that peak at 60%, between 1 and twice the
  current count.
- **Memory:** that peak plus 30%, rounded up to 256 MiB, at least 512 MiB.

Suggestions show the monthly cost change at the cluster's FinOps rates.
**Apply** files a `vm.resize` action (one per VM at a time) and runs it once
approved. Running guests are resized live where the guest allows it;
otherwise the new size is written to the VM's configuration and applies
at its next restart. Verification compares the size the VM runs with
against the request, so it reports pending until then. Undo resizes it
back to what it had before. A VM resized in the last 14 days, and not
undone, gets no new suggestion: its older history describes the old size.

### Consolidation

Hosts under 30% memory use are emptied, quietest first. Their VMs go
largest first onto the busiest host that stays under 80%. A host is kept
if any of its VMs can't move (host-local cloud network, local-only disk) or
nothing has room, and the last host is always kept. **Consolidate** files a
`drs.consolidate` action. Each live migration is prechecked when it runs,
and the emptied hosts are listed as candidates to power down. Nothing
powers a host off on its own.

### API

| Route | |
|---|---|
| `GET /api/v1/cloud/instance-groups/{id}` | Members with `desired_state` and `draining_since` |
| `GET /api/v1/cloud/instance-groups/{id}/forecast` | Last 48 hours of group demand, the next 24 forecast with the instances each needs, and the next-hour peak |
| `GET /api/v1/rightsizing` | Suggestions and the total monthly change |
| `POST /api/v1/rightsizing/propose` | `{ "vm_id", "vcpus"?, "memory_mib"? }`; without sizes, the suggestion. 409 while one waits |
| `GET /api/v1/drs/consolidation` | `{ moves, emptied, kept }` |
| `POST /api/v1/drs/consolidation/propose` | Files the plan; 400 `nothing_to_consolidate` |

### UI

Fleet Cloud → VPCs & elastic compute: **Autoscale** on a group sets the
CPU target, predictive scaling, scale-in mode, load balancer, member port
and drain. It also lists members (with draining ones marked) and charts
demand with the forecast. Fleet Cloud → Autopilot is the rightsizing inbox
(Apply or Request approval) and the consolidation plan.

### Limits

- Groups still live on the VPC's host, so they don't spread across hosts.
- A group member that is resized keeps its new size. The launch template
  is unchanged, so slots created later use the template's size.
- Predictive scaling raises the count for the coming hour. It doesn't hold
  capacity for a rush further ahead.
- Consolidation uses memory only, not CPU.
- A slot whose first create fails (an image download, for example) isn't
  retried by the group. Two slots on a cold host can both start
  downloading the same template image, so let the first instance finish
  before scaling past one.

### Tests

- `scripts/fleet/autopilot-realvm.sh` on real VMs: a VPC, subnet, Debian
  launch template and load balancer, then a group of 2. Scale in to watch
  a member drain from the load balancer and then sleep. Scale out to watch
  it wake with the same address and rejoin. Seeded history checks a
  rightsizing resize through approval, verification and undo. A seeded
  daily rush checks the group is raised before it.
- Controller unit tests: drain then sleep with load-balancer membership,
  the forecast endpoint and pre-scaling, the hourly rollup with a network
  rate, seasonal weekly/daily selection, rightsizing suggestions and a
  resize that is undone, consolidation plans (pinned VMs, full fleets),
  and the resize undo record.
- Spec tests: older policies serialize unchanged, and the forecast only
  adds capacity.
- `web/e2e/fleet-cloud-autopilot.spec.ts` (mocked): apply a suggestion,
  consolidation, empty states, and saving a group's autoscale settings.

## Game days

Break your own VMs on purpose and see what holds. An experiment injects
latency, packet loss, a network partition, a slow disk or a crash into
chosen VMs, one step at a time. Health probes run throughout, and the run
stops itself, lifting every fault, when they fail too often. Every run ends
with a report that compares each step with the baseline. OpenStack has no
equivalent; teams bolt on a separate chaos tool.

### Steps

| Kind | What happens |
|---|---|
| `latency` | `delay_ms` (up to 10 s) and `jitter_ms` added to traffic towards the VM |
| `loss` | `loss_pct` of packets towards the VM dropped |
| `partition` | Traffic between the VM and `cidrs`, and the addresses of `peers` (VM ids), dropped both ways |
| `disk` | The VM's first disk limited to `read_iops` / `write_iops` through libvirt, then set back to unlimited |
| `kill` | The VM is powered off hard. The step waits up to `recover_secs` for the controller's reconcile loop to bring it back and records how long that took. A VM whose desired state isn't `running` is refused, since nothing would restart it |
| `host_failure` | The digital twin's impact analysis for losing `host_id`: severity, what would be affected and recommendations. Nothing is shut down |

`latency`, `loss`, `partition` and `disk` take `secs`. A run is a
`baseline_secs` phase (default 15), the steps in order, then
`recovery_secs` (default 15). A run lasts at most an hour, with up to 20
steps, 20 targets and 10 probes.

### How faults are applied

`machina-bpfd` on the VM's host applies network faults to the VM's tap:

- **Latency and loss** are a `tc netem` root qdisc on the tap, which acts
  on traffic towards the VM. The tap's previous root qdisc (`fq` from bpfd
  QoS, or none) is put back afterwards. A tap carries one latency/loss
  fault at a time.
- **Partitions** are drop rules for the tap in the bridge table
  `machina_chaos` (forward, input and output hooks), so they hold for
  routed and bridged traffic alike.

Every fault is held under a lease: the step's length plus 30 seconds. bpfd
lifts it at lease end whether or not the controller is still there. Faults
are saved with bpfd's state and put back after a bpfd restart, as long as
their lease hasn't run out. A controller that restarts during a run marks
it `interrupted` and lifts its faults right away.

### Probes and abort

Probes run from the controller every 2 seconds with a 2-second timeout:

| Kind | Succeeds when |
|---|---|
| `tcp` | `target` (`host:port`) accepts a connection |
| `http` | `url` answers with `expect_status` (default 200; certificates aren't checked, redirects aren't followed) |
| `vm_running` | The VM `vm` is observed running |

The run aborts when probe success over the last `window_secs` (default 20)
falls below `min_success_pct` (default 50). At least 3 samples are needed
first. Set `min_success_pct` to 0 to never abort, for a kill whose downtime
is the point. On abort, every network fault and disk limit is removed and
killed VMs that are still down are started.

### Report

Each phase records its probe samples, success rate, p50 and p95 latency,
and notes: the recovery time of a kill, the host-failure analysis, or the
error of a step that couldn't run. Findings call out steps whose success
dropped or whose p95 more than doubled against the baseline. The verdict
is `passed`, `failed` (a step errored, or a kill didn't recover) or
`aborted`.

### Safety

- Starting a run means typing the experiment's name.
- VMs tagged `chaos=protected` can't be targeted.
- An experiment runs once at a time; editing or deleting it waits until
  the run ends.
- Only targeted VMs' taps are touched. Disk limits and kills go through
  the host agent, like any other power or tuning change.

### API

| Route | |
|---|---|
| `GET /api/v1/chaos/experiments` | With each one's last run status |
| `POST /api/v1/chaos/experiments` | `{ "name", "description"?, "spec" }`; 400 on an invalid spec, 403 for a protected target |
| `GET /api/v1/chaos/experiments/{id}` | The experiment, its last 20 runs and the longest it can run |
| `PUT` / `DELETE /api/v1/chaos/experiments/{id}` | 409 while it runs |
| `POST /api/v1/chaos/experiments/{id}/run` | `{ "confirm": "<name>" }`; 400 `confirm_mismatch`, 409 if already running |
| `GET /api/v1/chaos/runs?experiment_id=` | Latest 100 runs |
| `GET /api/v1/chaos/runs/{id}` | The run and its report; `live` phases and samples while it runs |
| `POST /api/v1/chaos/runs/{id}/abort` | Stops it and lifts every fault |
| `GET /api/v1/chaos/faults` | Faults active on every online host, with time left |

Writes need the operator role. bpfd requests: `vm_chaos_start`,
`vm_chaos_stop` (by `id` or `prefix`) and `vm_chaos_status`.

### UI

Fleet Cloud → Game days builds experiments (targets, steps, probes, abort
rule), runs them after the name is typed, follows a run phase by phase
with an abort button, and shows the report. Faults active on any host are
listed at the top.

### Limits

- Latency and loss act on traffic towards the VM only. Traffic the VM
  sends leaves through the tap's ingress, which belongs to the eBPF
  datapath.
- Probes run from the controller, so they see the network as the
  controller does, not as another VM would.
- `host_failure` is an analysis. Powering a host off for real stays a
  manual step.
- A partition from `peers` needs the controller to know their addresses.

### Tests

- `scripts/fleet/chaos-realvm.sh` on real VMs: latency, loss and a
  partition from a peer VM checked from the host and from the peer during
  each step; a disk limit seen in libvirt; a kill recovered by the
  reconcile loop; a simulated host failure; an automatic abort when probes
  fail; a manual abort; and no fault left behind after any of them.
- `scripts/bpf/vm-edge-smoke.sh` (veth): netem applied and the prior `fq`
  put back, one netem fault per tap, limits rejected, lease expiry, a
  partition on a bridged port, and a fault that survives a bpfd restart.
- bpfd unit tests: validation, netem arguments, the nft rules, root qdisc
  parsing. Controller unit tests: spec parsing and validation, abort window
  and report statistics, finding the first disk in domain XML.
- `web/e2e/fleet-cloud-chaos.spec.ts` (mocked): building an experiment,
  running after typing the name, the report, live faults and abort.

## Preemptible instances

Mark VMs that can wait (batch jobs, CI runners, dev boxes) as preemptible.
When a host runs short of memory they give way: saved to disk with their
memory intact, like a sleeping VM, rather than stopped. Once there is room
again they come back on their own, exactly where they left off. A regular
VM that fits on no host also makes room this way. OpenStack has no
preemptible instances; spot capacity there is a separate project, and it
deletes the instance.

### How it works

Each host keeps a share of its memory free: the **reserve** (default 10%,
up to 90%). Every 20 seconds the controller's leader checks each host:

- **Below its reserve:** the host's running preemptible VMs are saved to
  disk (`virsh managedsave`), lowest priority first and, within a
  priority, the largest first, until the reserve is back.
- **Room to spare:** preempted VMs are restored, highest priority first
  and then the longest waiting, as long as the host keeps its reserve
  plus 5% afterwards. The margin stops a VM from being restored and
  preempted again straight away.

A host the controller just acted on is left alone for 60 seconds, so its
next memory report reflects the change. Hosts without a heartbeat in the
last 2 minutes, and VMs with a power, migration or delete task in flight,
are skipped.

Priority runs from 0 to 100; lower gives way first. A preempted VM is out
of the [scale-to-zero](#scale-to-zero) wake set, so traffic to it doesn't
undo the preemption. Making it regular again puts it back in the wake set:
it wakes on its next packet like any sleeping VM.

**Making room at create time.** When a new regular VM fits nowhere, the
controller picks the schedulable host where preempting costs least
(lowest top priority, then the fewest VMs, then the least memory), saves
those VMs and places the new one there. A new preemptible VM never
preempts anything.

### API

| Route | |
|---|---|
| `GET /api/v1/preemption` | Settings, the resume margin, each host's free memory against its reserve, preemptible and preempted VMs, and the last 50 preemptions and resumes |
| `PUT /api/v1/preemption/settings` | `{ "enabled", "reserve_pct" }`; 400 outside 0–90 |
| `PUT /api/v1/vms/{id}/preemptible` | `{ "preemptible", "priority" }`; 400 outside 0–100 |

`POST /api/v1/vms` and `POST /api/v1/vms/from-template` take `preemptible`
and `preempt_priority`. Writes need the operator role. Preemptions and
resumes are recorded with the VM's sleep events (`preempted: <host> below
<n>% free memory`, `capacity freed`).

### UI

Fleet Cloud → Preemptible sets the reserve, shows each host's free memory
against it, lists preemptible and preempted instances with their priority,
and makes instances preemptible or regular. The create form has a
Preemptible box with a priority.

### Limits

- A preempted VM resumes on the host it was saved on, since the saved
  memory is a file on that host. It doesn't move to another host with
  room.
- Instance group templates can't be marked preemptible yet.
- HA failover and DRS don't preempt to make room.
- Memory is the only pressure considered, not CPU or disk.

### Tests

- `scripts/fleet/preempt-realvm.sh` on real VMs: a reserve just above the
  host's free memory preempts only the lower-priority VM (managed save,
  not a stop); traffic doesn't wake it; lowering the reserve restores it
  in the same boot with its processes still running; a preempted VM made
  regular joins the wake set and wakes on traffic; out-of-range settings
  are rejected.
- Controller unit tests: victim order, the cheapest host to make room on,
  resume with the reserve and margin, pressure then resume through the
  loop (and the wake set without the preempted VM), and making room at
  create time.
- `web/e2e/fleet-cloud-preemptible.spec.ts` (mocked): hosts under
  pressure, saving the reserve, priorities and the flag, and the create
  form sending the flag and priority.

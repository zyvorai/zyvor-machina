# VM network policy

Decide which VM may talk to which, in both directions, with the
**CiliumNetworkPolicy schema**. Policies select VMs by label and compile into
the VM edge programs `machina-bpfd` attaches to each VM tap. Cilium does not
need to be installed: libvirt VMs are enforced by Machina's own eBPF. Where
Cilium is present (for example on a Kubernetes node that is also a
hypervisor), it keeps enforcing its own endpoints and Machina enforces the VMs.

| Where | What |
|---|---|
| Web UI | Platform → Security → **VM Network Policies** (`/platform/zyra/security/network-policies`): policies, YAML editor with dry-run preview and templates, policy tester, endpoints / selectors, live **Flows** terminal. Scope switch: *This host* (daemon) or *Fleet* (controller). VM detail → **Network** tab: labels, enforcement state, live flows for that VM. |
| CLI | `machinactl netpol …`, `machinactl flow …`, `machinactl vm label …` (add `--fleet` to go through the controller); `scripts/platformctl netpol|flow|label` talks to the controller directly. |
| Daemon | `/api/v1/vm-network-policies*` (including `/fqdn-cache` and `/auth`), `/api/v1/flows`, `/api/v1/flows/stream`, `/api/v1/vms/{name}/labels` |
| Controller | Same paths; labels are `/api/v1/vms/{id}/labels`. Adds `PUT /vm-network-policies/{name}/enabled` and `POST /vm-network-policies/sync`. |

## Policy documents

Accepted kinds: `CiliumNetworkPolicy` and `CiliumClusterwideNetworkPolicy`
(`apiVersion: cilium.io/v2`) and Machina's own `VmNetworkPolicy`. A request
can carry several YAML documents separated by `---`, a JSON array, or a
`kind: List`. `spec` and `specs` both work. `metadata.namespace` is ignored:
VMs have no namespaces.

```yaml
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: db-from-web
spec:
  description: Only web VMs reach the database, on 5432/TCP
  endpointSelector:
    matchLabels:
      app: db
  ingress:
    - fromEndpoints:
        - matchLabels:
            app: web
      toPorts:
        - ports:
            - port: "5432"
              protocol: TCP
```

Validation is strict, and errors carry Cilium-style paths such as
`document[0].spec.ingress[0].toPorts[0].ports[0].endPort`.

### Supported fields (enforced)

| Field | Notes |
|---|---|
| `endpointSelector` | `matchLabels` and `matchExpressions` (`In`, `NotIn`, `Exists`, `DoesNotExist`). `{}` selects every VM. Key prefixes `k8s:`, `any:`, `machina:` are stripped; `reserved:host` / `reserved:world` and similar become entities. |
| `ingress`, `egress` | Allow rules. |
| `ingressDeny`, `egressDeny` | Deny rules. **Deny wins over allow**, and it applies even to VMs that are not otherwise isolated. |
| `fromEndpoints` / `toEndpoints` | VM selectors, on any host in scope. |
| `fromRequires` / `toRequires` | Narrows every allow peer in the same spec: the peer must also match. |
| `fromCIDR` / `toCIDR`, `fromCIDRSet` / `toCIDRSet` | IPv4/IPv6 prefixes; `except` carves out narrower prefixes. |
| `fromEntities` / `toEntities` | `all`, `world`, `world-ipv4`, `world-ipv6`, `unmanaged`, `host`, `remote-node`, `cluster` (alias `fleet`: every VM). `health`, `init`, `kube-apiserver` and `ingress` match nothing on a hypervisor (a warning says so). |
| `toPorts[].ports[]` | `port` (number or **named port**), `endPort` (ranges up to 256 ports), `protocol` `TCP` / `UDP` / `SCTP` / `ANY`. |
| `icmps[].fields[]` | `type` as a number or Cilium name (`EchoRequest`, `DestinationUnreachable`, …), `family` `IPv4` / `IPv6`. |
| `toFQDNs` | `matchName` (exact) and `matchPattern` (Cilium wildcards: `*` stays within one label, a lone `*` matches every name, a leading `**.` matches one or more labels). Allow rules only, as in Cilium; `toFQDNs` in `egressDeny` is rejected. See [DNS names](#dns-names-tofqdns). |
| `toPorts[].rules` | `http` (`method`, `path` as anchored regexes, `host`, `headers`, `headerMatches` with `value` or `secret` and `mismatch` `LOG` / `ADD` / `DELETE` / `REPLACE`), `kafka` (`role` or `apiKey`, `apiVersion`, `clientID`, `topic`), `dns` (`matchName` / `matchPattern`). Also on `toFQDNs` rules. See [L7 rules](#l7-rules). |
| `toPorts[].serverNames` | TLS SNI names, matched on the ClientHello. |
| `toPorts[].terminatingTLS` / `originatingTLS` | `secret` (`name`, optional `namespace`), `certificate`, `privateKey`, `trustedCA`. Egress only. See [TLS interception and header rewrites](#tls-interception-and-header-rewrites). |
| `toGroups` / `fromGroups` | Select `CiliumCIDRGroup` objects (any provider key; there is no cloud API on a hypervisor). See [Groups](#cidr-groups-and-togroups). |
| `toServices` | `k8sService` (`serviceName`, optional `namespace`) or `k8sServiceSelector` (`selector`, optional `namespace`; no namespace = any). Allows the selected services' frontends and backends on their ports, or on the rule's `toPorts` intersected with them. See [Services](#services-toservices). |
| `cidrGroupRef` | In `toCIDRSet` / `fromCIDRSet`: the prefixes of a named `CiliumCIDRGroup`. |
| `authentication.mode` | `required`, or `test-always-fail`. See [Authentication](#authentication). |
| `enableDefaultDeny` | `ingress: false` / `egress: false` keeps a direction open even when the spec has rules for it (additive policies). |
| `nodeSelector` (CCNP) | Accepted; host policies are not applied to VMs (warning). |

Named ports come from VM labels: `machina.io/port.<name>=<number>` (for
example `machina.io/port.http=8080`).

### Accepted with a warning

- `listener` (Envoy) is ignored.

## Services (toServices)

```yaml
egress:
- toServices:
  - k8sService: {serviceName: web-lb, namespace: shop}
- toServices:
  - k8sServiceSelector: {selector: {matchLabels: {tier: data}}}
  toPorts: [{ports: [{port: "5432", protocol: TCP}]}]
```

The service inventory depends on where policies are compiled:

- **Controller (fleet):** every Fleet Cloud load balancer is a service. Its
  name is the load balancer name, its namespace the project name. Endpoints
  are the listener (owning host address, listener port) and every enabled
  member (VM address, member port), with the load balancer protocol. Load
  balancers carry no labels, so `k8sServiceSelector` only matches them with
  an empty selector.
- **Daemon (single host):** Kubernetes services and endpoints from
  `kubectl get services,endpoints -A`, fetched only while a policy uses
  `toServices` and cached for a minute. Frontends are the cluster IPs,
  external IPs and load-balancer ingress IPs on the service ports; backends
  are the Endpoints addresses on their ports. Without kubectl or a cluster
  the inventory is empty.

A backend that is a managed VM resolves to that VM's identity; any other
address gets its own CIDR identity, so CIDR denies covering it still win.
An entry that selects no service matches nothing and the compiler warns.

## DNS names (toFQDNs)

```yaml
egress:
  - toEntities: [world]
    toPorts: [{ports: [{port: "53", protocol: UDP}]}]
  - toFQDNs:
      - matchName: github.com
      - matchPattern: "*.githubusercontent.com"
    toPorts: [{ports: [{port: "443", protocol: TCP}]}]
```

Enforced natively by snooping DNS, without a DNS proxy:

1. The VM edge program on the tap of every VM with a `toFQDNs` rule copies
   UDP DNS replies (source port 53) to bpfd. The copy goes to the
   `DNS_EVENTS` ring buffer, the same one the host DNS log uses.
2. bpfd parses each reply. If the query name or any name in its CNAME chain
   matches a `toFQDNs` pattern, bpfd keeps the A/AAAA addresses. Each one is
   held for the record TTL, but at least 10 minutes and at most a day,
   because clients cache answers longer than the TTL says. The cache holds
   up to 8192 addresses.
3. A learned address that isn't a VM, the host or a policy CIDR gets its own
   identity, as if it were a /128 CIDR peer. That identity first inherits
   every rule of the identity the address had before: its longest policy
   CIDR, or `world`. So `egressDeny` towards `world` or a CIDR still wins.
   It then gets allow entries for each `toFQDNs` rule whose pattern matches
   one of its names. Bindings are global, as in Cilium: any VM whose rule
   selects the name may use the address, whichever VM looked it up.
4. When the names expire or the rule goes away, the identity and its entries
   are removed.

Things to know:

- Policy has to allow the lookup itself (UDP or TCP 53 to the resolver), as
  in the example. Otherwise the reply never arrives and nothing is learned.
- Learning is asynchronous. The first SYN sent right after the reply can
  race it and be dropped; the TCP retransmit a second later goes through.
- Answers over UDP and over TCP are snooped (a TCP answer must fit in its
  first segment). DoT and DoH are encrypted and aren't; DoT can still be
  restricted by SNI with `serverNames` on port 853.
- `machinactl netpol fqdn` (like `cilium fqdn cache list`), the UI
  **Endpoints → DNS names** table and `GET /vm-network-policies/fqdn-cache`
  list the learned names. `machinactl netpol test --to api.example.com`
  evaluates a name against `toFQDNs` rules.

## L7 rules

```yaml
ingress:
  - fromEndpoints: [{matchLabels: {app: web}}]
    toPorts:
      - ports: [{port: "80", protocol: TCP}]
        rules:
          http:
            - method: GET
              path: "/v1/.*"
egress:
  - toEndpoints: [{matchLabels: {app: kafka}}]
    toPorts:
      - ports: [{port: "9092", protocol: TCP}]
        rules: {kafka: [{role: produce, topic: orders}]}
  - toFQDNs: [{matchPattern: "*.example.com"}]
    toPorts: [{ports: [{port: "443", protocol: TCP}], serverNames: [api.example.com]}]
  - toEntities: [world]
    toPorts:
      - ports: [{port: "53", protocol: UDP}]
        rules: {dns: [{matchPattern: "*.example.com"}]}
```

Enforced natively, without Envoy or a proxy in the data path:

1. The L3/L4 match marks the rule entry L7. A connection is allowed as usual,
   but every client segment past the flow's allowed window in `VM_L7_FLOW`
   is copied whole (up to 64 KiB) to bpfd over the `VM_L7_EVENTS` ring
   buffer. UDP DNS on an L7 port is copied the same way.
2. With the enforcement lease, the edge holds (drops) those segments. bpfd
   puts them in order per connection and parses the client's byte stream
   incrementally:
   - HTTP/1.x: headers may span segments; `Content-Length` and chunked
     bodies are skipped without inspection; pipelined and keep-alive
     requests are each checked; `CONNECT` and `Upgrade` open the connection.
   - HTTP/2, with prior knowledge or after an `h2c` upgrade: frames are
     followed and every header block is HPACK-decoded (Huffman, dynamic
     table, CONTINUATION). Each request's `:method`, `:path` and
     `:authority` are checked, so gRPC is matched by path
     (`/package.Service/Method`); DATA frames pass without inspection.
   - Kafka: whole requests up to 4 MiB, with Produce/Fetch topics.
   - TLS: the ClientHello SNI.
   - DNS over UDP and over TCP: the query name.

   Each request is checked against every L7 rule for the pair.
3. **Allowed:** bpfd widens the window to the end of the request (and of a
   body it does not need to see), then reinjects the held frames at once
   through a private veth pair (`mnl7inj0` → `mnl7inj1`, where
   `mn_vm_l7_inject` redirects them into the tap). A frame that carries the
   end of one request and the start of the next is split. There is no
   retransmission wait. Bodies inside the window pass in the kernel.
4. **Denied:** bpfd answers in place of the server: `HTTP/1.1 403 Access
   denied` for HTTP/1, a TCP reset for HTTP/2, Kafka, TLS and DNS over TCP,
   `REFUSED` for UDP DNS. It resets the server side too (through the same inject path,
   so routed and NAT taps work as well as bridged ones), and the connection
   stays closed.
5. An allowed UDP DNS query is reinjected once. The answer comes back
   through the tap, where `toFQDNs` learning sees it.

Windows are per tap, so a client VM and a server VM on the same host are each
checked against their own rules.

Each decision is a flow record with the L7 type and request (for example
`GET /v1/users`), shown with ◆ in `machinactl flow observe` and the UI
terminal; `machinactl flow top --by l7` groups them. Without the lease, a
denied request is an **AUDIT** flow and the traffic passes.

Rules add up the way Cilium's do: a request is allowed if any L7 rule for the
pair matches it. If a pair also has a plain L3/L4 allow for the same port,
no L7 check applies.

Limits:

- A denied HTTP/2 request resets the whole connection, not just its stream
  (dropping a header block would desynchronise the server's HPACK table).
  The HPACK dynamic table is capped at 64 KiB.
- TLS is matched on SNI only. After the ClientHello the connection is open,
  unless the rule intercepts it (see below).
- A request head over 64 KiB, a Kafka request over 4 MiB, or more than
  5 MiB held for one connection denies the connection.
- A connection already open when an L7 rule is applied is checked from the
  next segment bpfd sees; mid-request it fails to parse and is denied.
- bpfd tracks up to 65536 connections and forgets one after 5 minutes idle.
- If the inject veth cannot be created, allowed segments pass on the
  client's retransmit (at least 200 ms later) and allowed UDP DNS queries are
  forwarded from the host.

## TLS interception and header rewrites

```yaml
egress:
  - toEndpoints: [{matchLabels: {app: api}}]
    toPorts:
      - ports: [{port: "443", protocol: TCP}]
        terminatingTLS: {secret: {namespace: shop, name: api-intercept}}
        originatingTLS: {secret: {namespace: shop, name: api-upstream}, trustedCA: ca.crt}
        rules:
          http:
            - path: "/v1/.*"
              headerMatches:
                - {name: X-Team, value: blue, mismatch: REPLACE}
                - {name: X-Debug, value: "0", mismatch: DELETE}
                - {name: X-Via, value: machina, mismatch: ADD}
                - {name: Authorization, secret: {name: api-token}, mismatch: LOG}
```

These rules change the bytes on the wire, so they go through a terminating
proxy in bpfd instead of the hold-and-reinject path. A `toPorts` entry uses
the proxy when it has `terminatingTLS`, `originatingTLS`, a header match
with `secret`, or a `mismatch` of `ADD`, `DELETE` or `REPLACE`:

- `terminatingTLS`: bpfd completes the VM's TLS handshake with the secret's
  certificate. The VM must trust its issuer. `serverNames`, when present,
  are checked on the ClientHello first.
- `originatingTLS`: bpfd opens TLS to the server, verifying it against the
  secret's CA (`trustedCA`, default `ca.crt`; without either, the host's CA
  bundle). It presents the secret's `tls.crt` / `tls.key` when present. The
  server name is the client's SNI, else the request's `Host`.
- With `terminatingTLS` alone, the request goes to the server in plain HTTP.
  With `originatingTLS` alone, a plain-HTTP client reaches a TLS server.
- `headerMatches` on a mismatch: `LOG` records it, `ADD` appends the header,
  `DELETE` removes every header of that name, and `REPLACE` sets it. The
  request still matches in all four cases. Without `mismatch`, a mismatch
  fails the rule, as before.
- A header `secret` is read from the secret's `value` key. A secret that
  cannot be read never matches.

Secrets are files on each hypervisor, never in the policy or the controller:
`/etc/machina/netpol-secrets/<namespace>/<name>/<key>`. The namespace is
`default` when the reference has none, and `MACHINA_NETPOL_SECRETS_DIR`
moves the directory. TLS keys use the Kubernetes names `tls.crt`,
`tls.key` and `ca.crt` unless `certificate`, `privateKey` or `trustedCA`
name others. Keep them mode 0600, owned by root.

How a proxied connection flows:

1. The rule entry is marked PROXY as well as L7. A TCP connection from the
   VM that **starts** while the enforcement lease is live is recorded in
   `VM_PROXY_FLOW` and redirected through the inject veth. There,
   `mn_vm_l7_inject` hands it to bpfd's transparent listener
   (`127.0.0.1:4251` / `[::1]:4251`) with `bpf_sk_assign`. A policy route
   (`fwmark 0xb6000000 lookup 4251`, a `local` default route in that table)
   delivers it locally.
2. bpfd checks each HTTP/1.x request against every L7 rule for the pair,
   applies the matching rule's header rewrites, and forwards the request on
   its own connection to the original destination. A denied request gets
   `403 Access denied`. Each decision is a flow record (`… (tls intercepted)`,
   `(header X-Via added)`).
3. The upstream connection's socket mark carries the client's identity
   (`VM_PROXY_SRC`), so the server's edge applies its ingress policy to the
   client VM, not the host.

Limits:

- Egress rules only. HTTP/1.x only: bpfd offers ALPN `http/1.1` when it
  terminates TLS.
- Without the lease, or for connections opened before it, proxied ports
  pass untouched and nothing is parsed.
- Replies to the VM leave through the host's routing table, so the host
  needs a route to the VM's network (true for libvirt NAT and routed
  networks, and for bridges where the host has an address).
- The server sees connections from the host's address. Its policy still
  sees the client identity.
- `machinactl netpol status` shows the proxy (`L7 proxy: listening on …`,
  or why it is off).

## CIDR groups and toGroups

```yaml
apiVersion: cilium.io/v2alpha1
kind: CiliumCIDRGroup
metadata:
  name: partner-nets
  labels: {tier: partner, machina.io/region: eu}
  annotations: {machina.io/group-id: sg-123}
spec:
  externalCIDRs: [198.51.100.0/24, "2001:db8::/64"]
```

Apply it like a policy. Its prefixes are then used by:

- `toCIDRSet: [{cidrGroupRef: partner-nets}]` (and `fromCIDRSet`);
- `toGroups` / `fromGroups`. Every provider key (`aws`, `machina`, …) is
  read the same way: `names` and `securityGroupsNames` match the group name,
  `securityGroupsIds` matches the `machina.io/group-id` annotation, `labels`
  match group labels and `region` matches the `machina.io/region` label.

A reference to a group that doesn't exist matches nothing and the dry run
warns. Editing the group recompiles every policy that uses it.

## Authentication

```yaml
ingress:
  - fromEndpoints: [{matchLabels: {app: web}}]
    authentication: {mode: required}
```

Cilium authenticates SPIFFE identities with mutual TLS between agents.
Machina does the same between the bpfds of the two hosts, with a CA the
controller holds:

- The first packet of a new connection on a rule with `authentication` needs
  an entry for the pair in the `VM_AUTH` map. Without one it is dropped with
  reason `auth-required` (with the lease) or audited, and bpfd is asked to
  authenticate the pair. The TCP retransmit after authentication passes.
  Entries last an hour; after that, the next new connection triggers
  authentication again.
- **Peer on the same host:** the pair is accepted when the peer identity is a
  VM on one of this host's taps (the source guard below keeps it honest).
- **Peer on another host:** bpfd opens TLS 1.3 to the bpfd of the host that
  owns the peer identity, on TCP **4250**. Both present host certificates
  signed by the controller's CA; each certificate names its host as the DNS
  SAN `<host-id>.host.machina`, so the client checks that it reached the
  owner of the peer and the server checks which host is asking. The server
  accepts only when the peer identity is a VM on one of its taps and its
  synced state places the subject identity on the client's host. Any
  failure (no certificate, unreachable host, wrong name, foreign CA,
  identity not where the requester claims) leaves the pair
  unauthenticated, with the reason in the auth table.
- **Certificates.** While any fleet policy uses `authentication`, the
  controller asks each host's bpfd for a CSR (the key is generated on the
  host and stays in `/var/lib/machina/bpf/auth/key.pem`, mode 0600),
  signs it for 24 hours with the CA in `MACHINA_NETPOL_CA_DIR` (default
  `/var/lib/machina/netpol-ca`), and re-issues it past half its lifetime.
  The name, usages and lifetime come from the controller, never from the
  CSR. bpfd keeps the certificate across restarts and listens on 4250 once it
  has one, accepting only clients with a certificate from the same CA. Open
  4250/tcp between hypervisors.
- Without the controller (a daemon managing a single host) every VM is local,
  so no certificate is needed.
- `test-always-fail` never authenticates (reason `auth-test-always-fail`), as
  in Cilium.
- **Source guard.** While any authentication rule exists, every tap checks
  that a packet from its VM carries a source address that bpfd maps to that
  VM, or to no identity at all. A VM sending as another VM's address is
  dropped as `spoofed-source`, so identities can't be borrowed.

`machinactl netpol auth` and `GET /vm-network-policies/auth` list the
authenticated pairs and why the others failed. `machinactl netpol status`
shows the host certificate (`Auth cert:`). The bpfd `vm_auth_probe` op
(`{"op":"vm_auth_probe","host_id":…,"address":…}`) runs a handshake with
another host without identities, to check certificates and reachability.
Also, `machinactl netpol test` shows ⚿ on rules that need
authentication.

## Semantics

These follow Cilium:

- **No policy, no change.** A VM that no policy selects keeps talking to
  everything.
- **Default deny per direction.** Once any spec that selects a VM has an
  `ingress` or `ingressDeny` section, that VM is default-deny for ingress, and
  the same for egress. Traffic the rules don't allow is dropped.
- **Allow rules add up** across every policy that selects the VM.
- **Deny wins.** A matching deny beats any allow.
- A rule with only `toPorts` allows those ports from or to **any** peer. An
  empty rule `{}` allows **nothing**: it only turns default deny on.
- A connection must pass **egress at the source VM and ingress at the
  destination VM**, which may be on different hosts.
- Return traffic of an allowed connection is always allowed (conntrack, with
  ICMP echo tracked by identifier). DHCP and IPv6 link-local / neighbour
  discovery always pass.

## Labels

VM labels use Kubernetes syntax (`[prefix/]name`, value up to 63 characters).

- Daemon: stored in `/var/lib/machina/vm-labels.json`.
- Controller: the `vms.labels` column (migration 029). The migration copies
  any `key=value` tags into labels.
- VMs without labels fall back to their `key=value` tags.
- Reserved labels are added automatically: `machina.io/vm-name`,
  `machina.io/host` and `machina.io/project`.

```bash
machinactl vm label web-1 app=web tier=frontend   # add / overwrite
machinactl vm label web-1 tier-                   # remove
machinactl vm label web-1                         # show
```

## How it is enforced

1. **Compile.** The compiler in `machina-bpf` (`bpf/machina-bpf/src/netpol/`)
   turns the policies and the VM inventory into a `VmEdgeState`:
   - every VM gets a stable 32-bit identity (an FNV hash of its name);
   - CIDR peers, the host and other hypervisors get identities too;
   - rules are keyed (subject identity, peer identity, direction, protocol,
     port), with a deny flag and a source string such as
     `db-from-web spec.ingress[0]` for flow attribution.
2. **Push.**
   - Single host: the daemon compiles and pushes on every change, on VM
     start/stop, and every 60 s, because guest addresses change with DHCP.
   - Fleet: the controller compiles one state per host and pushes it through
     the agent. That happens every 30 s on the leader, only when the state
     changed, plus forced pushes every 10 ticks and `POST …/sync`.
   - Whoever writes the state records itself as the edge **owner**. The
     daemon leaves a controller-owned edge alone, so local policies show as
     inactive.
3. **Datapath.** On each tap, the TCX program looks up the peer address in
   `VM_IPS` (exact match), then `VM_CIDR_IDS` (longest prefix), and otherwise
   treats it as `world`. It then checks `VM_POLICY` with wildcards: any peer,
   any port, any protocol. A deny match wins; a miss on an isolated direction
   is a default-deny drop. An allow entry can also require authentication
   (checked against `VM_AUTH` for new connections) or L7 (client segments
   go to bpfd until it allows them, see [L7 rules](#l7-rules)), or send the
   connection through bpfd's proxy (see
   [TLS interception](#tls-interception-and-header-rewrites)).

The usual [safety model](README.md#safety-model) applies. Without the
enforcement lease, a drop is recorded as an **AUDIT** flow and the packet is
forwarded. With the lease, it is dropped and recorded as **DROPPED**.
Policies are persisted; the enforce mode and its lease never are.

## Tools (Cilium / Hubble equivalents)

| Cilium | Machina |
|---|---|
| `kubectl apply -f cnp.yaml` | `machinactl netpol apply -f cnp.yaml` (`--dry-run` previews selected VMs and compiled rule count) |
| `cilium policy get` | `machinactl netpol get [-o yaml\|json\|wide]` |
| `cilium policy trace` | `machinactl netpol test --from web-1 --to db-1 --port 5432` — evaluates egress at the source and ingress at the destination and names the matching rule |
| `cilium policy selectors` | `machinactl netpol selectors` |
| `cilium endpoint list` | `machinactl netpol endpoints` |
| `cilium status` | `machinactl netpol status` |
| `cilium fqdn cache list` | `machinactl netpol fqdn` |
| `cilium policy trace` with L7 | `machinactl netpol test … --http-method GET --path /v1/x` (also `--host`, `--header`, `--sni`, `--dns-name`, `--kafka-api-key`, `--kafka-topic`, `--kafka-client-id`, `--kafka-api-version`) |
| `cilium-dbg bpf auth list` | `machinactl netpol auth` |
| `hubble observe` | `machinactl flow observe [-f] [--vm X] [--verdict DROPPED] [--port 443] …` |
| — | `machinactl flow top --by pair\|src\|dst\|port\|policy\|l7`, `machinactl flow stats` |
| Hubble UI service map | `machinactl flow edges [--vm X] [-o json]`, UI **Service map** tab |
| `cilium policy` audit mode, then hand-written rules | `machinactl netpol learn [--vm X] [--group-by app]` — generates policies from the flow history |
| — | `machinactl netpol replay -f draft.yaml` — what the draft would have done to the last 7 days of traffic |
| — | `machinactl flow alerts` — port scans, host sweeps, deny bursts, new peers |
| — | `machinactl vm quarantine VM [--for 1h] [--allow-host-ssh]`, `vm release VM`, `netpol quarantines` |
| — | `machinactl netpol jit grant --from A --to B --port N --for 1h`, `netpol jit [approve\|reject ID\|revoke NAME]` |

Auth: `MACHINA_API_TOKEN`, or `MACHINA_USER` + `MACHINA_PASS`. With a user
and password, the CLI keeps its session in `~/.machina/cli-session` (mode
0600; override with `MACHINA_COOKIE_JAR`) and logs in again only when that
session expires, because `/auth/login` is rate limited.

## Flows

Every new connection on a policy-managed tap emits a flow event; drops and
audits are rate-limited to one per flow per second. Each flow carries:

- source and destination VM name, address, port, identity and labels;
- protocol, TCP flags or ICMP type, and size;
- verdict `FORWARDED`, `DROPPED` or `AUDIT`, with the drop reason
  (`policy-deny`, `default-deny`, `l7-deny`, `auth-required`,
  `auth-test-always-fail`, `spoofed-source` or `quarantine`);
- the policy rule that decided it;
- for L7 decisions, the request type and summary.

bpfd keeps the last 5000 flows. The controller merges flows from every host
and tags each one with its host.

Filters, the same in the API (query string), the CLI (flags) and the UI
terminal prompt: `vm`, `from_vm`, `to_vm`, `label` (`k=v`), `ip`, `cidr`,
`port`, `protocol`, `verdict` (comma list), `drop_reason`, `policy`
(substring), `direction`, `host` (fleet).

`machinactl flow observe` colours its output on a TTY; set `NO_COLOR` or
pass `--color never` to turn that off. `-o json` prints one record per line.
The UI **Flows** tab is a black macOS-style terminal that takes the same flags
at its prompt; Ctrl-C pauses it and Ctrl-L clears it.

## Flow history, service map, learn, replay and alerts

bpfd folds every flow into a 7-day **history** of edges: source, destination,
direction, protocol, port, verdict, drop reason and policy, with counts,
bytes, and first and last seen. It is saved to
`/var/lib/machina/bpf/flow-history.json` every minute and on shutdown, and
capped at 20 000 edges. VMs appear by name. The host, other nodes, the
internet and CIDR peers appear by address, with `src_entity` / `dst_entity`
set to `host`, `remote-node`, `world` or the CIDR. Each edge also keeps up to
32 L7 requests, normalised so that `GET api/users/42?x=1` and
`GET api/users/7` count as `GET api/users/{id}`. For each request it keeps:

- the count and how many were denied;
- for requests that go through the bpfd proxy (TLS interception or header
  rewrites), the response status class (`2xx`, `4xx`, `5xx`) and the average
  and maximum latency.

API: `GET /api/v1/flows/edges[?vm=X]`, `DELETE /api/v1/flows/edges` (admin,
clears it), `GET /api/v1/flows/alerts[?limit=N]`. The controller serves the
same routes, merged across hosts and tagged with the host.

**Service map.** The UI *Service map* tab draws the history: clients on the
left, servers to their right, the host and external addresses on the far
right. Link colours: green for forwarded, dashed amber for traffic a policy
would drop (observe mode), red for dropped. Pick a link, on the graph or in
the list under it, to see its ports, verdicts, matching policies and L7
metrics.

**Learn.** `POST /api/v1/vm-network-policies/learn` (UI *Learn* tab,
`machinactl netpol learn`) writes least-privilege policies from the history.
Run in observe mode for a while first, so the history covers normal traffic.

- VMs are grouped by a label (`group_by`, default `app`); a VM without it is
  pinned by `machina.io/vm-name`.
- Peers become `toEndpoints` / `fromEndpoints` for VMs and
  `toEntities: [host]` / `[remote-node]` for the host and other nodes.
  External addresses become `toFQDNs` when the `toFQDNs` cache knows their
  name, and a `/32` or `/128` `toCIDR` otherwise.
- With `l7` on (the default), HTTP methods and paths (`{id}` becomes
  `[^/]+`), DNS names and TLS server names are written as L7 rules.
- Port 53 gets a DNS `matchPattern: "*"` rule when the policy uses
  `toFQDNs`, so lookups keep working.
- Edges the datapath dropped are skipped, as are those with fewer than
  `min_count` flows. `lock_unobserved` writes default deny (`[{}]`) for a
  direction with no observed traffic.

The output is YAML to review, not applied: learn also records whatever an
attacker did while it was watching, such as a port scan.

**Replay.** `POST /api/v1/vm-network-policies/replay` with `{yaml}` (the
*Replay history* button in the editor, `machinactl netpol replay -f`) takes
the stored policies, replaces those with the same names and adds the new
ones. It then traces every distinct connection and L7 request in the history
against both the current set and the draft, and reports:

- `would_break`: allowed now, denied by the draft;
- `would_allow`: denied now, allowed by the draft;
- `not_evaluated`: connections whose endpoints no longer resolve, such as
  deleted VMs.

`machinactl netpol replay` exits non-zero when something would break, so it
can gate CI. Policies are additive, as in Cilium: a new policy can only take
traffic away from a VM it newly isolates. To narrow what a VM already allows,
replay a changed version of the policy that allows it, under the same name.

**Alerts.** The history also watches a 60-second window per source, counting
each connection once, at the client:

| Alert | Trigger | Severity |
|---|---|---|
| `port_scan` | 20+ ports on one destination | high |
| `host_sweep` | 20+ destinations on one port | high |
| `deny_burst` | 50+ denied or audited flows | medium |
| `new_peer` | a VM pair talking for the first time, once the history is a day old | low |
| `threat_domain` | a VM resolved a name on a [threat feed](#dns-threat-feeds) | high |
| `new_domain` | the first lookup under a base domain, once the history is a day old | low |

The same alert is suppressed for 10 minutes, and the last 1000 alerts are
kept. bpfd logs each alert. The leader controller turns new ones into
`netpol.alert` events every 30 s, so they reach webhooks and SIEM export.
A fleet `port_scan`, `host_sweep` or `threat_domain` from a VM also creates a pending
`vm.quarantine` action in Approvals (at most one pending per VM): one hour,
SSH from its host allowed. Nothing happens until someone approves it.

## Quarantine

Quarantine cuts a VM off the network for a fixed time, from 1 second to 24
hours (default 1 hour). Every flow on its taps is dropped:

- in both directions;
- including connections that were already open;
- whatever the enforcement mode, so no lease is needed.

The only exceptions are the allowlist you give and DHCP / IPv6 neighbour
discovery, which always pass.

```bash
machinactl vm quarantine web-1 --for 1h --allow-host-ssh --reason "port scan"
machinactl vm quarantine web-1 --for 15m --allow ingress:host:tcp/22 --allow egress:world:udp/53
machinactl netpol quarantines        # VM, time left, taps, exceptions, reason, who
machinactl vm release web-1
```

Add `--fleet` to go through the controller. A fleet quarantine is held on
every online host, so it follows the VM if it migrates; `--host H` limits it
to one host. In the UI, the *Endpoints* tab has a Quarantine panel and a
Quarantine button per VM, and each alert has a button for its source VM.

An exception is `DIRECTION:PEER[:PROTO[/PORT]]`:

- `DIRECTION`: `ingress` (towards the VM) or `egress`.
- `PEER`: `host` (this host's global addresses), `world`, `any`, or a VM in
  the synced policy state.
- `PROTO`: `tcp`, `udp`, `sctp`, `icmp`, `icmpv6`, or empty for any. ICMP
  exceptions allow every ICMP type.

How it works:

- bpfd writes the deadline (monotonic clock) into each tap's `VM_EDGE`
  entry and the exceptions into `VM_POLICY` with a quarantine direction bit.
  The datapath stops applying the quarantine at the deadline by itself, even
  if bpfd or the daemon is down.
- Open connections are cut because a quarantined tap uses its own per-tap
  conntrack (`VM_QCT`) instead of the shared one.
- A VM with no policy has its taps programmed for the quarantine alone.
- bpfd saves quarantines with their wall-clock end, and after a restart
  holds them again for the time left.
- Dropped flows carry the reason `quarantine` and are not counted by the
  detectors.

API, the same on the daemon and the controller:

- `POST /api/v1/vms/{name}/quarantine` with `{secs, allow_host_ssh, allow:
  [{direction, peer, proto, port}], reason}` (admin);
- `DELETE /api/v1/vms/{name}/quarantine` (admin);
- `GET /api/v1/vm-network-policies/quarantines`.

The controller also takes `?host=`. It records each quarantine and release
as a `netpol.quarantine` event. The `vm.quarantine` Zyvor action takes the
same body plus `vm` and `host` in its `object_ref`.

## Just-in-time access

Temporary access is an ordinary policy that removes itself. It lets one VM,
or the host, reach a port on another VM until a deadline (default 1 hour,
max 24 hours).

```bash
machinactl netpol jit grant --from web-1 --to db-1 --port 5432 --for 1h --reason "schema migration"
machinactl netpol jit grant --from host --to db-1 --port 22 --for 15m
machinactl netpol jit                 # active grants (and pending requests on the fleet)
machinactl netpol jit revoke jit-web-1-to-db-1-5432-…
```

On a single host, an admin grants access directly.

Through the controller (`--fleet`, or the UI in fleet scope), a grant is a
request instead. It appears in Approvals as a `vm_netpol.jit` action, and in
the *Temporary access* panel:

- It is applied only when another admin approves it (`netpol jit approve
  ID`, or the Approve button in either place).
- The requester's own approval is refused, and the request stays pending.
- `netpol jit reject ID` turns it down.
- Admins can skip the approval with `--now` (*Grant now* in the UI).

What a grant contains:

- The policy is named `jit-<from>-to-<to>-<port>-<id>` and labelled
  `machina.io/jit: "true"`.
- Its annotations record the end, who granted it and why:
  `machina.io/expires-at` (RFC 3339), `machina.io/granted-by` and
  `machina.io/reason`.
- It has an ingress rule on the target and, for a VM source, an egress
  rule on the source.
- Both rules set `enableDefaultDeny: false`. A grant therefore only adds an
  allow and never isolates a VM that was open before.
- Without `--port`, every port is allowed. `--proto` takes `tcp` (the
  default), `udp`, `sctp` or `any`.

Expiry:

- `machina.io/expires-at` works on any policy, not just grants.
- An expired policy is no longer compiled and is deleted: by the daemon
  within a second (its resync wakes for the next expiry), and by the leader
  controller within 30 s.
- The daemon logs each expiry. The controller records grants, requests and
  expiries as `netpol.jit` events.

API, the same on the daemon and the controller:

- `GET /api/v1/vm-network-policies/jit`: `{items, pending}`.
- `POST /api/v1/vm-network-policies/jit` with `{from, to, port, protocol,
  secs, reason}`. On the controller, add `grant: true` for an immediate
  grant (admins).
- Revoke with `DELETE /api/v1/vm-network-policies/{name}`.

While the controller manages a host's VM edge, the daemon refuses local
grants and points to `--fleet`.

## DNS threat feeds

A threat feed is a named domain list checked against the DNS replies every
VM receives. A listed domain also covers its subdomains. Feeds work with or
without policies: while any feed exists, bpfd programs the taps of every
running libvirt VM for DNS capture.

```bash
machinactl netpol threat set urlhaus --url https://example.org/hosts.txt --block
machinactl netpol threat set lab -f blocklist.txt          # alert only
machinactl netpol threat set c2 --domain evil.example --domain bad.example --block
machinactl netpol threat                                   # feeds, watched VMs, blocked addresses
machinactl netpol threat refresh urlhaus
machinactl netpol threat rm lab
```

A feed accepts one domain per line, hosts files (`0.0.0.0 evil.example`),
Adblock rules (`||evil.example^`) and URLs; `#` and `!` start comments. It
holds at most 500 000 domains, and a download at most 64 MiB. Feeds with a
URL are fetched again every 12 hours.

When a VM resolves a listed name:

- bpfd raises a `threat_domain` alert (high). It names the VM, the name,
  the listed domain, the feed and the answer addresses.
- For a blocking feed (`--block`), each A/AAAA answer address gets the
  reserved identity `threat-domain` for the record's TTL (10 minutes to
  1 day), and every VM gets an egress deny towards it. Connections show as
  AUDIT in observe mode and are dropped under the enforcement lease, with
  the reason `threat-domain`. These flows are logged even when flow logging
  is off, once per connection.
- The addresses of known VMs are never blocked, so a forged answer cannot
  cut a VM off.

Alongside, bpfd remembers the base domain (`example.org`) of every
successful VM lookup. Once the history is a day old, the first lookup
under a new base domain raises a `new_domain` alert (low). This runs for
every VM whose DNS replies bpfd captures (feeds or `toFQDNs` policies).

Both alerts follow the rules in [Alerts](#flow-history-service-map-learn-replay-and-alerts):
a fleet `threat_domain` also files a pending `vm.quarantine` action.

Feeds persist in `/var/lib/machina/bpf/threat-feeds.json` and survive a bpfd
restart. Blocked addresses do not; they are learned again from the next
lookup.

With `--fleet`, the controller stores feeds in its database
(`vm_netpol_threat_feeds`) and pushes them to every online host. Once it
holds at least one feed, each host carries exactly the fleet's feeds, and
host-local feeds are removed within 30 s. The controller records changes
and refreshes as `netpol.threat` events. While the controller manages a
host's VM edge, the daemon refuses local feed changes.

In the UI, the *Alerts* tab has a *DNS threat feeds* panel: add a feed by
URL or domain list, refresh or remove it, and see the blocked addresses.

API, the same on the daemon and the controller:

- `GET /api/v1/vm-network-policies/threat-feeds`: `{feeds, blocked,
  watched_vms}` (the controller adds `hosts` and `errors`);
- `PUT /api/v1/vm-network-policies/threat-feeds/{name}` with one of `url`,
  `domains` or `text`, plus `block` (admin);
- `POST /api/v1/vm-network-policies/threat-feeds/{name}/refresh` (admin);
- `DELETE /api/v1/vm-network-policies/threat-feeds/{name}` (admin).

## Plain-English policies

Describe a policy in sentences and get ordinary policy YAML back. A draft
is validated and replayed against the flow history, and nothing is applied
until someone applies it (on one host) or a second admin approves it (on
the fleet).

```bash
machinactl netpol draft "only web servers can reach the db on port 5432"
machinactl netpol draft "block db-1 from reaching the internet; isolate app=cache" > draft.yaml
machinactl --fleet netpol draft "web-1 can reach github.com on https" --propose
machinactl --fleet netpol draft pending        # drafts waiting for approval
machinactl --fleet netpol draft approve ID     # another admin; reject ID turns it down
```

The YAML goes to stdout. Notes, unused sentences, validation messages and
the replay summary go to stderr as `#` lines.

Who drafts:

- The daemon, and the controller with `--rules`, use a built-in sentence
  parser. It is deterministic and never guesses: a sentence it cannot read,
  or an endpoint that matches nothing in the inventory, is reported instead
  of turned into a rule.
- The controller asks Zyvor's LLM first when one is configured (Settings →
  AI). The reply must be valid policy YAML that selects only existing VMs.
  An invalid reply gets one repair round; after that, or when the LLM
  declines or is not configured, the sentence parser drafts instead and
  the note says why.

Sentences the parser understands (several per request, separated by new
lines, `;` or full stops):

| Sentence | Policy |
|----------|--------|
| `only A can reach B [on PORTS]`, `B can only be reached by A` | Ingress rule on B: isolates B to A (and the ports). |
| `allow A to reach B [on PORTS]`, `A can reach B`, `allow traffic from A to B` | Additive allow with `enableDefaultDeny: false`; nothing is isolated. |
| `block A from reaching B`, `A must not reach B`, `deny traffic from A to B` | `ingressDeny` on B, or `egressDeny` on A for the internet, CIDRs and domains. |
| `isolate A` | `ingress: [{}]` and `egress: [{}]`: A talks to nothing. |

- **Endpoints:** a VM name, a `key=value` label, a label value (`web
  servers` → `app=web`; `app`, `role`, `tier`, `service` and `component` are
  tried first), `host`, `the internet` / `world`, `everyone`, a CIDR or IP,
  or a domain.
- **Internet, CIDR and domain targets:** these become egress rules on the
  source.
- **Domains:** a domain target becomes `toFQDNs` plus the DNS rule it needs.
- **Ports:** numbers, `N/udp` (or `udp N`), or service names
  (`ssh`, `http`, `https`, `dns`, `postgres`, `mysql`, `redis`, `mongo`,
  `rdp`, `smtp`, `ntp`, `kafka`).

Drafted policies are named `nl-<verb>-<from>-to-<to>`, carry the sentence
as `spec.description`, and are labelled `machina.io/source: plain-english`.

Approval on the fleet:

- `--propose` (or *Request approval* in the UI) files a `vm_netpol.apply`
  action holding the YAML and the original request.
- Another admin approves it in Approvals, in the editor's draft panel or
  with `netpol draft approve ID`. The requester's own approval is refused.
- On approval the YAML is validated again, applied and pushed to every
  host. Requests and approvals are recorded as `netpol.nl` events.

Zyvor chat: a message that starts with `/policy`, `policy:`, `netpol:` or
`draft a network policy:` is answered with a draft (the same pipeline)
instead of a general reply. The answer links back to the editor to replay
and request approval.

In the UI, the *YAML editor* tab starts with a *Describe in plain English*
box. *Draft policy* fills the editor and shows the notes, the validation
result and how many recorded connections the draft would break.

API:

- `POST /api/v1/vm-network-policies/draft` with `{prompt}`, plus
  `rules_only` on the controller. It returns `{yaml, source, notes,
  unparsed, preview, replay}`, or 422 with the reasons when nothing could
  be drafted. This needs the operator role.
- Controller only:
  - `POST /api/v1/vm-network-policies/draft/propose` with `{yaml, prompt}`
    returns `{pending}`.
  - `GET /api/v1/vm-network-policies/draft` lists the pending drafts.

## Project isolation (Fleet Cloud)

On the fleet, a VM's Fleet Cloud project is also a policy label:
`machina.io/project`. Projects can be isolated from each other without
writing any YAML.

```bash
machinactl --fleet netpol projects                    # every project and its settings
machinactl --fleet netpol project assign payments web-1 db-1   # put VMs in a project (- clears)
machinactl --fleet netpol project default isolated    # new projects start isolated
machinactl --fleet netpol project open shared-tools   # this one stays open
machinactl --fleet netpol project isolate payments --no-host
machinactl --fleet netpol project inherit payments    # back to the default
machinactl --fleet netpol project reset payments      # drop every setting
```

- **Isolated:** the project's VMs accept connections only from VMs of the
  same project, plus the host unless `--no-host` is given. The host is
  every global address on the hypervisor (management address, libvirt
  bridges), so SSH and health checks from the host keep working. **Open:** no
  generated rule. **Inherit:** follow the default, which is open until
  set.
- **Host addresses:** each host's `machina-bpfd` reports all of its global
  addresses (`node_addrs` in its status), and the controller refreshes them
  every few minutes. On a host, its own addresses are `host`; the other
  hosts' addresses are `remote-node`. An address that several hosts share
  (libvirt's `192.168.122.1`, for example) is left out of `remote-node`.
- **VM addresses:** every address seen for a VM is part of its identity,
  not only the first one. The sources are:
  - libvirt DHCP leases;
  - the guest agent (every NIC, IPv6 included), when its channel is
    connected;
  - otherwise the host's neighbour tables (ARP and IPv6 NDP), matched by
    the MAC addresses of the VM's NICs (`virsh domiflist`). This finds
    static IPv6 addresses and second NICs without `qemu-guest-agent`;
  - source addresses `machina-bpfd` sees in traffic from the VM's tap
    (`learned` in `vm_edge_status`). bpfd keeps up to 16 per VM and
    forgets an address 15 minutes after its last packet.
- **Learned addresses that are refused:** the controller adds learned
  addresses to the VM's identity unless the address belongs to a host,
  another VM on the same host already owns it, or two VMs on the same host
  both send from it. A VM cannot take over another VM's identity by
  spoofing its address.
- **Cross-host NAT:** VMs on libvirt NAT networks reach VMs on other hosts
  with their host's address, so isolation cannot tell them apart from the
  host. A project with VMs on two or more hosts whose VM subnets are NATed
  per host (the same `addr/prefix` on several hosts) is flagged:
  `cross_host_nat` in the API, a warning in `netpol projects`, a badge in
  the UI and a line in evidence exports. Turn on the
  [overlay](#cross-host-overlay-wireguard) to keep each VM's identity
  across hosts (the flag then goes away), or use routed or bridged
  networks.
- **Exceptions:** ordinary allow policies still apply, because policies
  add up. One `CiliumNetworkPolicy` that lets `app=monitor` reach port
  9100 opens that port into an isolated project.
- **Generated policies:** each setting becomes a policy named
  `project-isolation-<project>-<hash>`, labelled
  `machina.io/managed-by: project-network`. It is compiled, traced, replayed
  and pushed like any other policy. It is listed by `netpol trace` and in
  evidence exports, but not stored: changing the setting replaces it.
- **Enforcement:** drops need the enforcement lease like every policy. In
  observe mode, cross-project connections show up as AUDIT flows.
- **Storage:** settings live in the controller table `vm_netpol_projects`.
  Changes are recorded as `netpol.project` events.

### Preview and approval

```bash
machinactl --fleet netpol project isolate payments --preview   # replay only
machinactl --fleet netpol egress payments allow 10.0.0.0/8 --propose
```

- **Preview:** `--preview` (or `--dry-run`) on any project or egress change
  replays the result against the 7-day flow history and prints what it
  would break or newly allow. Nothing is applied.
- **Approval:** `--propose` sends the change to *Approvals* as a
  `vm_netpol.project` action with the replay summary. A second admin
  applies it (`netpol project approve ID`, or *Approvals* in the UI);
  the requester cannot. Anyone with operator rights can withdraw it with
  `netpol project reject ID`. With `MACHINA_NETPOL_PROJECT_APPROVAL=1` on
  the controller, every project change goes through approval. Approvals
  and rejections are listed in evidence exports.
- **Identity through the daemon:** when the daemon's controller proxy uses
  `MACHINA_PLATFORM_AUTH`, the controller sees every daemon user as that
  service account. The requester is then recorded as the service account,
  and two daemon users cannot approve each other's changes. The second
  admin needs their own controller login (`POST /api/v1/users`, then
  `/api/v1/auth/login` on the controller).

### Who may change what

- **Fleet admins:** everything.
- **Project admins** (the `admin` role in a Fleet Cloud project): that
  project's isolation, egress allowlist and `require-ip`. Egress IPs are
  host addresses and need a fleet admin, as does the default (`*`).
- **Members** (any project role) may preview changes and export their
  project's evidence.

### UI

The *Projects* view (fleet scope) has:

- the default isolation;
- *Preview against recorded traffic* and *Ask a second admin* toggles,
  which apply to every change made on the page;
- an *Assign* form that puts any VM in a new or existing project;
- warnings (cross-host NAT, VMs without their egress IP);
- a row per project with its VM count, isolation picker and badges, and
  *Manage…* for its VMs, egress allowlist, egress IPs, `require-ip`, a
  per-project reachability matrix (with configurable probes) and evidence
  export.

Fleet Cloud instance pages show the instance's isolation, egress and egress
IP, including whether its host lacks the project's egress IP.

### API

All on the controller.

- `GET /api/v1/vm-network-policies/projects` returns
  `{default, items, warnings}`. Each item has the stored settings, the
  effective `isolated` / `allow_host`, the VMs, the generated policy
  names, `cross_host_nat` and `egress_gaps`.
- `PUT /api/v1/vm-network-policies/projects/{project}[?propose=1]` with
  `{isolation, allow_host, egress_restricted, egress_allow, egress_ips,
  egress_ip_required}`. Project `*` is the default and takes only
  `isolation` and `allow_host`. When approval applies, it returns
  `{pending, preview}` instead.
- `DELETE /api/v1/vm-network-policies/projects/{project}[?propose=1]`
  resets it.
- `POST /api/v1/vm-network-policies/projects/{project}/preview` with the
  same body returns `{describe, summary, replay}`.

## Egress allowlists and egress IPs

A project's outbound traffic can be limited to a list, and given its own
source address on the way out.

```bash
machinactl --fleet netpol egress payments allow '*.stripe.com' --port 443
machinactl --fleet netpol egress payments allow 10.20.0.0/16 --port 5432
machinactl --fleet netpol egress payments allow 203.0.113.10 --port 53/udp
machinactl --fleet netpol egress payments                    # show it
machinactl --fleet netpol egress payments remove 10.20.0.0/16
machinactl --fleet netpol egress payments unrestrict         # keep the list, stop limiting
machinactl --fleet netpol egress payments ip hv-1 198.51.100.7
machinactl --fleet netpol egress payments ip hv-2 '198.51.100.8,2001:db8::8'
machinactl --fleet netpol egress payments require-ip         # block hosts without one
machinactl --fleet netpol egress                             # egress IPs on every host
```

### Allowlist

- **Restricted project:** its VMs reach only the allowlist, the project's
  own VMs, the host (unless `--no-host`) and DNS on the host.
- **Entries:** a CIDR or IP, a domain, `*.domain`, or `world`. Each entry
  takes optional ports (`443`, `53/udp`); without ports every port is
  allowed. Domains become `toFQDNs` rules, so they work once the VM has
  resolved the name.
- **DNS:** lookups must go to the host's resolver (port 53 on the host), which
  is what libvirt networks hand out. To use another resolver, add it to
  the list with `--port 53/udp`.
- **Generated policy:** `project-egress-<project>-<hash>`, with the same
  labels and handling as the isolation policy.

### Egress IPs

- **What it does:** an egress IP rewrites the source address of the
  project's traffic leaving one host. Firewalls and SaaS allowlists
  outside the fleet can then tell tenants apart.
- **Setting it:** set one per host, by host name or id: one IPv4, one
  IPv6, or both comma-separated. IPv4 VM addresses leave with the IPv4
  egress IP and IPv6 ones with the IPv6 egress IP.
- **Adding the address:** when the egress IP is not on the host yet,
  `machina-bpfd` adds it as a `/32` or `/128` to the interface of the
  default route for that family and announces it: a gratuitous ARP for
  IPv4, an unsolicited neighbour advertisement for IPv6. It removes the
  address with the rule, and only addresses it added itself (they are kept
  in its state file across restarts). An address that is already
  configured is used as it is and never removed.
- **Routing:** the announcement is enough when the egress IP is in the
  uplink's subnet. An address from another range needs the upstream
  router to route it to that host (a static route, or a routed IP from
  the provider); otherwise replies never arrive.
- **Controller settings:** `MACHINA_NETPOL_EGRESS_MANAGE=0` stops hosts
  adding addresses (missing ones are then skipped, as before).
  `MACHINA_NETPOL_EGRESS_INTERFACE=eth1` picks the interface instead of
  the default route's.
- **What is never rewritten:** destinations in private (RFC 1918),
  CGNAT, link-local, loopback and multicast ranges keep the VM's own
  source, and so do IPv6 ULA (`fc00::/7`), link-local, loopback and
  multicast. VM-to-VM traffic and policies are unaffected.
- **How it works:** on every reconcile the controller sends each host the
  rules for its VMs. `machina-bpfd` installs them as the nftables tables
  `ip machina_egress` and `ip6 machina_egress` (a `srcnat` chain with
  `ip saddr {VMs} snat to IP`, and the `ip6` equivalent), replaced
  atomically together on each change.
- **VMs on other hosts:** a VM that migrates (or starts) on a host without
  one of its project's egress IPs leaves with that host's address. The
  controller records a `netpol.egress_gap` event when this starts and
  stops, and lists the VM in `egress_gaps`, `netpol projects` and evidence
  warnings. With `require-ip` (`egress_ip_required`), a generated
  `project-egress-ip-<project>` policy instead blocks those VMs' internet
  egress (everything except the never-rewritten ranges above, and IPv6
  outside `2000::/3`); `allow-host-ip` turns it off.
- **Missing addresses:** a rule whose address is not on the host and
  could not be added (no default route, adding turned off, `ip` failed) is
  skipped and reported, not applied. `managed` in the status lists the
  addresses bpfd added, as `IP/prefix@interface`.
- **Persistence:** the table is address translation, not a security
  control, so it stays in place while bpfd is stopped. bpfd re-applies the
  saved rules when it starts.
- **Off by default:** with no egress IPs set, nothing is installed and no
  `nft` command runs.

`GET /api/v1/vm-network-policies/egress-ips` (controller) shows each host's
rules, whether they are active, skipped rules, added addresses and errors.

## Cross-host overlay (WireGuard)

```bash
machinactl --fleet netpol overlay                 # status, fleet addresses
machinactl --fleet netpol overlay enable          # 100.96.0.0/12, fd6d:6163:6869::/48, udp/51871
machinactl --fleet netpol overlay enable --prefix4 100.80.0.0/12 --port 51900
machinactl --fleet netpol overlay disable
```

On libvirt NAT networks a VM's traffic to another host leaves with its
host's address, so the receiving host cannot tell which VM sent it. The
overlay carries VM-to-VM traffic between hosts in encrypted WireGuard
tunnels and keeps the sender's identity.

- **Fleet addresses:** each host gets a fleet prefix: a `/24` of the IPv4
  range and a `/64` of the IPv6 range. Each VM address on the host gets a
  fleet address in it. `.1` (`::1`) is the host's own. Allocations are
  stored by the controller and stay the same while the VM keeps its
  address on that host.
- **Reaching a VM on another host:** use its fleet address (`netpol overlay`
  lists them). The receiving host sees the sending VM's fleet address,
  which is part of that VM's identity, so policies, project isolation and
  flows name the VM. A VM that moves to another host gets a fleet address
  in that host's prefix.
- **How it works:** each host's `machina-bpfd` runs the `machina-wg`
  WireGuard interface and the nftables tables `ip machina_overlay` and
  `ip6 machina_overlay`. They hold 1:1 maps: arriving traffic to a fleet
  address is DNATed to the VM, and VM traffic into the tunnel is SNATed
  to its fleet address (ahead of libvirt's masquerade). TCP MSS is
  clamped to the tunnel MTU (1420).
- **What a peer host can send:** each peer may only use its own prefixes
  as source addresses (WireGuard `allowed-ips`), so one host cannot pose as
  another host's VMs. New connections from the tunnel are dropped unless
  they go to a mapped fleet address; a peer cannot use the host as a
  router. Replies to connections VMs opened pass.
- **No leaks:** the fleet ranges and the host's own prefix are
  `unreachable` routes. Traffic for a host that is not peered yet, or for
  an unmapped fleet address, is refused rather than sent out the uplink.
- **Keys:** each host generates its private key in bpfd's state directory
  (`overlay/wg.key`, mode 0600) and hands it to the kernel over netlink,
  never to the `wg` command (distro AppArmor profiles only let `wg` read
  `/etc/wireguard`). The
  controller learns the public keys from the push responses; the first
  push after enabling generates them and the next one sets the peers.
- **Endpoints:** a peer's endpoint is the address the controller has for
  that host in its inventory, port 51871 (`--port`). Allow that UDP port between
  hosts.
- **Requirements:** `wireguard-tools` (`wg`) on every host (`machinactl
  deps` installs it) and the `wireguard` kernel module.
- **Disabling:** removes the interface, the routes, the forward rule and
  the tables on every host, and the fleet addresses from identities.
- **Storage:** settings and allocations live in the controller table
  `vm_netpol_overlay`. Changes are recorded as `netpol.overlay` events.

API (controller): `GET /api/v1/vm-network-policies/overlay` returns the
settings and, when on, each host's status (interface, public key,
prefixes, fleet addresses, peers with their last handshake and bytes).
`PUT` with `{enabled, prefix4?, prefix6?, port?}` changes them (admin).

## Segmentation evidence

`netpol evidence` exports what an auditor asks for when checking network
segmentation (PCI DSS 1.2/1.3 or SOC 2 CC6.6, for example) as one document:

```bash
machinactl --fleet netpol evidence                        # summary and matrix
machinactl --fleet netpol evidence -o json --out ev.json  # full document
machinactl --fleet netpol evidence -o md --out ev.md      # for the audit binder
machinactl --fleet netpol evidence --project payments --probes tcp/22,tcp/5432
machinactl netpol evidence verify ev.json                 # digest and signature
machinactl --fleet netpol evidence archive                # scheduled exports
machinactl --fleet netpol evidence archive NAME -o md --out ev.md
```

The document (`kind: machina.io/segmentation-evidence/v1`) contains:

- **Summary:** VMs, how many are covered by a policy, policies (stored
  and generated), projects and how many are isolated, hosts in sync and
  enforcing, denied connections and alerts.
- **Hosts:** for each host, whether it is reachable, whether it is
  enforcing, who owns its edge, and its last sync and error.
- **Policies:** every policy, with its kind, whether it is enabled or
  generated, its description, the selected VMs and a SHA-256 of its YAML
  (as `netpol get NAME -o yaml` prints it).
- **Projects:** isolation, egress allowlist and egress IPs per project
  (fleet only).
- **Reachability matrix:** for every pair of groups, plus `host` and
  `world`, which probe ports the policy set allows.
  - Groups are projects, or VMs when no VM has a project, up to 40.
  - The probes are TCP 22, 80, 443, 3389, 5432 and UDP 53 unless
    `--probes` (`?probes=tcp/22,udp/53`, up to 16) picks others.
  - Each cell is computed by the policy tracer from up to three sample VMs
    per group, and lists the pairs it traced. It shows what the policy
    allows, not what was observed.
- **Denied connections:** the 50 largest dropped or audited edges in the
  7-day flow history, with the total.
- **Operations:** alerts, active quarantines, temporary access grants,
  threat feeds and egress IPs.
- **Approvals:** on the fleet, 90 days of `vm_netpol.jit`,
  `vm_netpol.apply` and `vm.quarantine` approval actions, with requester and
  approver.
- **Warnings:** what the report cannot show as configured: projects
  across per-host NAT and VMs running without their project's egress IP.
- **Digest:** a SHA-256 over the compact JSON with `digest` empty and no
  `signature`. `netpol evidence verify FILE` recomputes it (with `jq` and
  `sha256sum`), so later edits to a saved file are detected.
- **Signature (fleet):** the controller signs the digest with ECDSA P-256
  (`signature.alg: ecdsa-p256-sha256`). The signing certificate
  (`evidence.pem`, issued by the controller's netpol CA, which also issues
  host authentication certificates) and the CA certificate are embedded.
  `verify` checks the signature and the chain with `openssl` and prints the
  CA fingerprint; pass `CA.pem` as the second argument to pin your own copy.

**Project reports:** `--project P` (`?project=P`) limits the document to
one project: its VMs, its policies and settings, and a matrix of the
project against `(other projects)`, `host` and `world`. Fleet-wide reports
need an operator; project reports are open to the project's members.

**Scheduled exports:** the controller writes a signed JSON report to
`MACHINA_NETPOL_EVIDENCE_DIR` (default `/var/lib/machina/netpol-evidence`)
every `MACHINA_NETPOL_EVIDENCE_EVERY_HOURS` (default 24; 0 turns it off)
and deletes reports older than `MACHINA_NETPOL_EVIDENCE_KEEP_DAYS` (default
90). `GET /api/v1/vm-network-policies/evidence/archive` lists them and
`…/evidence/archive/{name}[?format=md]` serves one.

The Markdown form has the same content as tables, and is what the UI's
*Export evidence → Markdown* button downloads. *JSON* downloads the
document byte for byte, so its digest verifies.

API: `GET /api/v1/vm-network-policies/evidence[?format=md][&probes=…]` on
the daemon (this host) and the controller (the fleet, also `&project=P`). Exports are logged, as
`netpol.evidence` events on the controller.

## Known limitations

- **Approvals through the daemon proxy:** with `MACHINA_PLATFORM_AUTH`
  set, the controller records every daemon user as the proxy's service
  account (see *Preview and approval*). The two-person rule then needs a
  second admin with their own controller login.
- **Self-approval status code:** refusing a requester's own approval
  returns HTTP 500 with the reason in the body, not 403. The shared
  approvals handler maps every refusal to an internal error.
- **Egress IPs from another range:** bpfd adds and announces the egress
  IP, but an address outside the uplink's subnet still needs an upstream
  route to the host.
- **Overlay addressing:** VMs reach VMs on other hosts by fleet address,
  not by their own address, and the fleet address changes when a VM moves
  to another host. Without the overlay, projects spanning NATed hosts are
  only flagged.
- **Silent VMs:** without the guest agent, an address outside libvirt DHCP
  is only seen once the VM sends from it (neighbour tables or tap traffic).
- **Timing-sensitive smoke checks:** the large-body L7 checks in
  `vm-edge-smoke.sh` (384 KiB and chunked POST) can fail on a heavily
  loaded host; rerun before investigating.
- **Mocked UI tests:** `platform-vm-network-policies.spec.ts` mocks the
  API, so it does not catch request-format mismatches with the
  controller. The real-VM script and the live spec do.

## Test

`scripts/bpf/vm-edge-smoke.sh` has a *VM network policy* section. It runs on
a veth pair in a scratch netns and covers:

- identity peers;
- port ranges and ICMP type rules;
- deny over allow;
- CIDR longest-prefix match;
- `ingressDeny` without isolation;
- AUDIT, FORWARDED and DROPPED flow events with rule attribution;
- `toFQDNs`: a stub resolver on the host answers one name. The checks cover
  blocking before the lookup, learning from the reply, allowing after it,
  ignoring NXDOMAIN and unmatched names, `world` deny still winning, and
  removing the rule.
- L7: HTTP allow, 403 and keep-alive requests; HTTP/2 and gRPC-style paths
  with `curl --http2-prior-knowledge`; no retransmission wait; a
  request head split across segments; a 384 KiB body and a chunked body
  followed by a denied request on the same connection; HTTP over IPv6; TLS
  SNI with `openssl`; Kafka Produce to an allowed and a denied topic; DNS
  over UDP (reinjected, REFUSED) and over TCP (answered, reset, learned by
  `toFQDNs`); AUDIT in observe mode.
- Authentication: `test-always-fail`; `required` against a VM on another
  host while this host has no certificate (blocked, with the reason in the
  auth table) and against a VM on this host (authenticated locally); the
  source guard with and without the lease.
- Egress IPs: a second netns stands in for the internet, routed through
  the host. A server there reports the source address it sees: the VM's
  address before the rule, the egress IP after it, and the VM's address
  again when the rule is skipped (non-local address with adding turned
  off) or cleared. The checks also cover refusing an invalid config and
  removing the nftables table. Then, with `interface` set to the test
  veth, bpfd adds a missing IPv4 and IPv6 egress IP, lists them as
  `managed`, teaches the outside netns its MAC by gratuitous ARP and
  unsolicited NA, and is seen with the added address; clearing removes
  them and keeps the address it did not add. The section is skipped when
  the host already has an `ip machina_egress` table.
- Learned addresses: the VM's source address shows up in `learned` after
  its traffic.
- Overlay: a netns with its own WireGuard interface stands in for a peer
  host. The checks cover the VM reaching the remote VM as its fleet
  address, the remote reaching the VM by fleet address, the handshake and
  encrypted bytes, a source outside the peer's prefixes and an unmapped
  destination both dropped, the `unreachable` fleet route, and disabling
  removing the interface, tables, routes and forward rule. Skipped without
  `wg` or when `machina-wg` exists.

`scripts/bpf/vm-netpol-realvm.sh` runs against two real VMs. It boots
`np-client` and `np-server` from a Debian cloud image on the `default` NAT
network, labels them and applies an ingress policy: the client may only
`GET /ok` on the server's port 80. In observe mode everything still works
and flows show AUDIT. Under a short enforcement lease (`LEASE`, default
300 s), `/ok` answers, other paths and methods get 403, port 8080 and the
host are dropped, and flows record DROPPED and the L7 request. An 8-second
temporary grant then opens 8080 for the client (the host stays dropped),
and the port is dropped again once the grant expires. A second
policy then sends the client's HTTPS through the proxy, with a throwaway CA
and secret. It checks the `REPLACE` / `DELETE` / `ADD` rewrites as the
server receives them, a 403 for a path outside the rule, the policy
certificate on the client, the client identity at the server (the host is
refused on 443), and `originatingTLS` from a plain-HTTP client to a TLS
server. Back in observe mode it then checks the flow history, using the
traffic the earlier phases generated (the history is cleared when the
script starts):

- the client → server `GET /ok` edge, the dropped 8080 edge, and status and
  latency on the proxied 443 traffic;
- that `netpol learn` writes a valid policy for the server which replays
  without breaking anything;
- that a narrowed proxy policy is reported as breaking 443;
- that a port scan from the client raises `port_scan`;
- that the history is saved to disk.

Then it quarantines the server, still in observe mode, and checks:

- that an SSH session from the client, open before the quarantine, stops;
- that new connections fail in both directions, while SSH from the host
  still works and the client is unaffected;
- that the drops carry the reason `quarantine`;
- that releasing restores traffic;
- that a 6-second quarantine lifts itself;
- that a quarantine survives a bpfd restart;
- that a VM with no policy at all can be quarantined.

Last, with no policies left, it sets a blocking threat feed for
`example.com` (`THREAT_DOMAIN`) and checks:

- that a lookup from the client raises `threat_domain` and blocks the
  answer addresses;
- that the site stays reachable in observe mode, with AUDIT flows labelled
  `threat-domain`;
- that under a 60-second lease the site is dropped, with DROPPED flows,
  while the gateway stays reachable;
- that the feed survives a bpfd restart, and the site is reachable again
  once the feed is removed.

The fleet phase then hands the edge to the controller. It waits for the
controller inventory to list both VMs, puts the server in project `np-red`
and the client in `np-blue`, isolates `np-red`, and checks:

- that a cross-project request is only audited in observe mode, and dropped
  under a lease;
- that the host and the server's own outbound requests still work;
- that moving the client into `np-red` lets it in, and `--no-host` drops
  the host;
- that an egress allowlist on `np-blue` (`example.com:80`,
  `1.1.1.1:443`) lets DNS through the host, the domain and the listed IP
  and port through, and drops other ports and destinations, with DROPPED
  flows; `unrestrict` restores access;
- that a project egress IP rewrites the client's source as seen from a
  netns on the host (198.51.100.1 → 198.51.100.77). The check also covers
  private destinations keeping libvirt's NAT, the other project not being
  rewritten, and the nftables table going away when the IP is removed;
- the same over IPv6: `virbr0` and the client get a ULA (`fd00:6e70::/64`),
  the netns `2001:db8:78::/64`. The guest agent reports the client's ULA,
  the controller puts it in `ip6 machina_egress`, and the netns sees
  `2001:db8:77::77` instead of the ULA until the IP is removed. The VMs
  run `qemu-guest-agent` for this. The IPv6 part is skipped on a host
  with its own IPv6 default route (enabling IPv6 forwarding would stop it
  accepting router advertisements); the forwarding and `virbr0` sysctls
  are restored on exit;
- that bpfd learned the client's address from its tap, and that enabling
  the overlay brings up `machina-wg` with a public key, maps both VMs to
  fleet addresses in nftables, and installs the `unreachable` fleet route
  and the drop for unmapped connections; disabling removes them all;
- that the evidence export marks `np-red` isolated, shows `np-blue → np-red`
  segmented and `host → np-red` allowed, lists the dropped connection, and
  verifies;
- approval: opening `np-red` with `--propose` leaves it isolated, the
  requester cannot approve it, and a rejected proposal changes nothing. A
  temporary controller admin (`np-approver-PID`, random password) then
  approves a new proposal, `np-red` opens, and the evidence lists the
  approval with the recorded requester and the approver. The user is
  deleted on exit.

On exit it returns the edge to observe, releases any quarantine, and
deletes the VMs, the policies, the feed, the secret, the temporary admin
and the image.
It enforces on every tap with policy state, so run it only on a disposable
host. The password is read from stdin:

```bash
printf '%s\n' "$PASS" | bash scripts/bpf/vm-netpol-realvm.sh
```

Compiler and tracer unit tests: `cargo test -p machina-bpf netpol`. The
policy page has Playwright tests: `web/e2e/platform-vm-network-policies.spec.ts`
(mocked API; assertions wait up to 15 s, because lazy chunks and toasts are
slower under a full parallel run) and `web/e2e/live-vm-network-policies.spec.ts`
(read-only, with `PLAYWRIGHT_LIVE_URL`).

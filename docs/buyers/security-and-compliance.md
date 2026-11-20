# Security and compliance

What exists today, in plain terms. This is a checklist for your review, not a certification; the controls to switch on are in
[compliance hardening](../compliance-hardening.md). Claims are tracked in the [ledger](../claims.md).

## Who can do what
- **Authentication**: PAM, LDAP, OIDC or SAML, with roles; unknown users default to read-only.
- **Least privilege keys**: API keys carry a role, and can be scoped to named projects so a CI job reaches its own project and
  nothing else ([guide](../guides/scoped-api-keys.md), verified live).
- **Two-person approval** for risky changes such as deletions and project network changes (`MACHINA_NETPOL_PROJECT_APPROVAL=1`).
- **Audit**: every change is logged; export as NDJSON to your SIEM (`GET /api/v1/audit/export`) or send to syslog or a webhook.

## Network isolation
- **Security groups** are enforced in the kernel on each VM's network interface, with replies to allowed connections passing.
  The status reports what each host says and never claims protection that is not active ([guide](../guides/security-groups.md),
  verified live, including through a restart of the enforcement service).
- **Fail-open by design.** Enforcement is leased: the controller renews it every 30 seconds, and if the controller or
  `machina-bpfd` stops, the host drops back to observe-only rather than cutting traffic. The state reads `auditing` until the next
  renewal. Decide with your security team whether that trade-off fits; it is documented, not hidden.
- **Dry run first.** Enforcing shows rule counts and lockout warnings (for example no inbound SSH) before you confirm.
- **Segmentation evidence.** Network policy, flow history and signed segmentation evidence exports are described in
  [the eBPF docs](../ebpf/vm-network-policy.md).

## Data and secrets
- EC2 access-key secrets and LLM provider keys are encrypted at rest when `MACHINA_API_KEY_MASTER_KEY` is set. **Set it in
  production**: unset means plaintext.
- User data is never logged. The instance metadata service has no token requirement, as on early EC2; keep long-lived
  secrets out of user data ([guide](../guides/metadata-service.md)).
- TLS on the API and consoles; install your own certificate in place of the self-signed one.

## Supply chain
Releases ship `SHA256SUMS` and a CycloneDX SBOM ([INSTALL](../INSTALL.md)).

## Not claimed
A formal certification (FIPS included), per-interface security groups, cross-host VPC isolation proven on real traffic, and multi-host failover under real host loss
(needs a drill at your site).

# Compliance and hardening checklist

Use this when preparing Machina for security review (not a certification).

## Network

- [ ] TLS enabled (`[tls]` with org-signed certs)
- [ ] Daemon bound only as needed (`host = 0.0.0.0` vs `127.0.0.1` + reverse proxy)
- [ ] Firewall allows only operator subnets to port 5092
- [ ] No legacy `/ws/v1/ssh/{host}` unless required (`[ssh_terminal].legacy_plain_host_websocket`)

## Authentication

- [ ] PAM or LDAP/OIDC — disable unused methods
- [ ] LDAP: `ldaps://` or STARTTLS; avoid `insecure_tls` in production
- [ ] OIDC: confidential client, short-lived sessions
- [ ] API tokens: least-privilege `scopes` (see automation API)
- [ ] `roles.json` reviewed — unknown users default to readonly

## Host

- [ ] Daemon runs as dedicated user where possible (default install: root for libvirt)
- [ ] `NoNewPrivileges` in systemd unit (see `contrib/machina-daemon.service`)
- [ ] Audit log retained: `/var/lib/machina/audit.log` (optional rotation, syslog, HTTP webhook in `[audit]`)
- [ ] OTLP export configured (`[observability.otlp]` → Alloy / OpenTelemetry Collector)
- [ ] Linux auditd visible via `GET /api/v1/host/linux-audit` and optional `health_avc_threshold`
- [ ] Export audit via `GET /api/v1/audit/export` (NDJSON) to SIEM

## Observability

- [ ] Prometheus scrape configured (`GET /api/v1/prometheus`)
- [ ] Optional Grafana dashboard: `contrib/grafana/machina-overview.json`

## Data

- [ ] Backups scheduled (`machina-backup.timer`)
- [ ] Backup checksum verification periodic

## Not in scope today

Stated plainly so a security review does not have to find it:

- **No FIPS-validated cryptography.** The controller and agent use rustls with the
  `ring` provider (TLS 1.2 and 1.3). There is no FIPS module build.
- **No certification or benchmark mapping.** There is no CIS, STIG, SOC 2 or ISO 27001
  mapping; this page is a checklist, not evidence for an audit.
- **Agent traffic is token-only** unless `MACHINA_AGENT_TLS_CERT` and `_KEY` are set.
- **The default certificate is self-signed.**
- **JWT signing-key rotation is manual** ([PRODUCTION_READINESS.md](PRODUCTION_READINESS.md)).
- **No data-residency or retention policy** is enforced for audit, telemetry or AI data.
  With a local LLM provider nothing leaves the host; a hosted provider sends prompts to it.
- **BMC credentials** for bare-metal power are encrypted at rest only when
  `MACHINA_API_KEY_MASTER_KEY` is set; the fencing password in `hosts.ipmi_password`
  is still stored as plaintext.

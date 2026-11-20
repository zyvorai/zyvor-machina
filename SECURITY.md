# Security

Report suspected vulnerabilities privately through GitHub's **[private vulnerability reporting](https://github.com/zyvorai/machina/security/advisories/new)** or by email to [security@zyvor.dev](mailto:security@zyvor.dev). Do not publish exploitable details in a public issue before coordination.

## Scope

Machina runs privileged code on hypervisor hosts. In scope:

- `machina-daemon` (REST + WebSocket API on `:5092`, PAM/OIDC/SAML/LDAP auth, RBAC, VNC/SPICE/serial/SSH console proxies)
- `machina-controller` (fleet control plane on `:5093`, JWT auth, task bus)
- `machina-agent` (gRPC on `:50051`, TLS)
- The web UI in `web/`
- Install and deploy scripts (`install.sh`, `machinactl`, `scripts/deploy-remote.sh`)

## Hardening defaults

- The controller refuses to boot with the well-known development JWT secret unless `MACHINA_ALLOW_DEV_SECRETS=1`.
- `MACHINA_SKIP_AUTH=1` is for local development only and must never be set in production.
- LLM provider API keys are encrypted at rest with AES-256-GCM when `MACHINA_API_KEY_MASTER_KEY` is set.

## Supported versions

Security fixes land on `main` and the latest release. Subscribers receive backports per their [subscription](docs/SUBSCRIPTION-MODEL.md).

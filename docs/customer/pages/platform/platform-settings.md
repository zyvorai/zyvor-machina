# Settings

## Purpose

One hub for platform configuration: general and cluster preferences, identity and SSO, users and groups, API keys, AI providers, Zyra, network, policy and quotas, webhooks, reports, console, keychain and updates.

## When to use it

- Configuring SSO or adding users
- Creating API keys for automation
- Setting policy (for example, require confirmation for production VM deletion) or quotas
- Connecting AI providers for Zyra

## How to get there

- Route: `/platform/settings`
- Nav: **Platform → Settings** (or spotlight / Finder search)

## What you can do

1. Pick a section on the left: **General**, **Identity & SSO**, **Users & Groups**, **Security**, **API Keys**, **AI Providers**, **Zyra**, **Network**, **Infrastructure**, **Policy & Quotas**, **Apps & Integrations**, **Webhooks**, **Reports**, **Console**, **Keychain**, **Stage Manager**, **Updates**.
2. Many sections show a summary with an **Open full … workspace** link to the dedicated page.
3. **Security** holds cluster-wide switches such as **Require confirmation for production VM deletion** (deletes queue for approval), air-gap bundles with artifact checksums, and tenant policy export as YAML (**Generate YAML**).
4. **Policy & Quotas** opens the full policy rules and project quota workspace.
5. Integrations moved here: the old `/platform/integrations` URL opens `?section=integrations`.

## If something is wrong

- **Section read-only:** your role is not Admin.
- Host-level daemon settings (PAM, TLS, libvirt URI) are in `/etc/machina/config.toml`, not here; see the admin configuration guide.

## Related pages

- [AI Providers](platform-ai-providers.md)
- [Configure Zyra](../platform-security/platform-zyra-configure.md)
- [Users](platform-users.md)
- [API keys](platform-api-keys.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)

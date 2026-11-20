# Getting Started with Machina

## What you need

| Requirement | Notes |
|-------------|--------|
| Linux host with KVM/libvirt | Machina daemon builds/runs on Linux |
| URL | **`https://<host>:5092`** (TLS on by default) |
| Login | Package installs: the generated `admin` user (`sudo machinactl show-login`). Source installs: PAM (host Linux account). LDAP/OIDC when configured |
| Browser | Modern Chromium, Firefox, or Safari |

## 1. Open the dashboard

Open `https://<host>:5092`. Accept the self-signed certificate in labs, or install your org cert in production.

Health check:

```bash
curl -sk https://localhost:5092/api/v1/health
```

## 2. Sign in

| Mode | What you do |
|------|-------------|
| Generated admin | Package installs: `sudo machinactl show-login` prints the URL, user and password; change it after first sign-in |
| PAM | Source installs: use your Linux username/password on the host |
| LDAP | Directory credentials when LDAP is enabled |
| OIDC | SSO via `/auth/oidc/login` when configured |

RBAC roles: Admin / Operator / ReadOnly via `roles.json`, OIDC groups, or API tokens. Empty `roles.json` means everyone is Admin — fix that before production.

## 3. Orient yourself

After sign-in you get a **Mac-style desktop**: menubar (Zyvor mark + Machina / View / Window / Help), left **icon rail** sidebar, and Spotlight (`⌘K` or Window menu).

1. **Sidebar** — pinned icons (Mission Control, VMs, Machine Finder, Hosts, Settings) plus Workloads / Infra / Ops / Secure / Admin section flyouts. Hover a section icon for its routes; chevron at the bottom expands labels.
2. **Platform** (`/platform`) — Mission Control for the fleet.
3. **Classic home** (`/`) — this hypervisor’s guests and host health.
4. **Fleet Cloud** (`/fleet-cloud`) — native instances; use the pill nav (Overview / Instances / … / **More**).
5. Settings → Appearance → **Desktop density** (Normal / Power / Advanced) controls how much of the fleet surface is unlocked; **Apple** light theme is the default.

## 4. First workflows

### A. List and open a VM console

`/vms` → select a VM → Console (VNC/SPICE/SSH/serial/RDP as available).

### B. Create a VM

`/create` or Platform → VM Builder / Advanced Create.

### C. Check host health

`/node` or Platform → Hosts.

### D. Fleet Cloud

`/fleet-cloud` pages work out of the box — instances, images, volumes, security groups, networking, load balancers, and keypairs are all native, no external cloud connection or credentials required.

### E. Containers (Vessel)

**Infrastructure → Containers** (`/containers`) when Podman/Docker is connected — see [Admin basics](admin-basics.md).

## Next steps

- [Using the Dashboard](using-the-dashboard.md)
- [Admin basics](admin-basics.md)
- [Page guides](pages/README.md)

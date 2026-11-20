# Machina — Customer Documentation

**Machina** manages libvirt VMs, storage, networks, and consoles on Linux KVM hosts — with a native Fleet Cloud layer, fleet Mission Control, and enterprise auth.

| You want to… | Open |
|--------------|------|
| Install and log in | [Getting Started](getting-started.md) |
| Learn the shell | [Using the Dashboard](using-the-dashboard.md) |
| Follow a page, step by step | [Page-by-page guides](pages/README.md) |
| Look up any screen by route | [Complete page index](PAGE_INDEX.md) |
| Deploy, auth, ports | [Admin basics](admin-basics.md) |
| Multi-page jobs | [Common workflows](workflows.md) |
| Capability map | [Feature Guide](../machina-customer-feature-guide.md) |

## Printable PDFs

```bash
node scripts/customer-docs/build-customer-pdfs.mjs
```

Output lands in [`pdf/`](pdf/):

| PDF | Contents |
|-----|----------|
| `Machina-Customer-README.pdf` | This overview |
| `Machina-Getting-Started.pdf` | Access, login, dashboard basics, workflows |
| `Machina-Page-by-Page.pdf` | Complete page manual |
| `Machina-Admin-Basics.pdf` | Deploy, auth, ports |

## Product at a glance

```text
  Daemon UI/API  →  https://<host>:5092
  Controller     →  :5093 (optional multi-host)
  Surfaces       →  Web · machinactl CLI
  Workloads      →  libvirt VMs · Fleet Cloud · fleet platform
```

## Support surfaces (quick map)

| Need | Typical path |
|------|----------------|
| This host VMs | `/vms`, `/create` |
| Fleet / Mission Control | `/platform` |
| Consoles | VM detail → ConsoleHub / VNC / SSH / RDP |
| Storage / networks | `/storage`, `/networks` or `/platform/storage` |
| Fleet Cloud | `/fleet-cloud` (native, no wiring required) |
| Settings / users | `/settings`, `/platform/users` |

---

*ZyvorAI Labs · [zyvor.dev](https://zyvor.dev) · Machina*

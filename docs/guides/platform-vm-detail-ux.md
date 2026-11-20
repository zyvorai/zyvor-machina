# Platform VM Detail UX

Machina platform VM detail is organized **machine-first, task-second**: VNC hero, compact action row, attention stack, Connect hub, and deep work on dedicated tabs — **apple.com chapter** first viewport (no card boxes in the hero band).

## Page zones

| Zone | Component | Purpose |
|------|-----------|---------|
| VNC hero | `VmConsoleHeroPreview` | Live display preview + Open Cinema CTA above the fold (Overview tab) |
| Hero | `VmDetailHero` | State, health/doctor pills, blocker CTA |
| Actions | `VmDetailActionBar` | Open Cinema, SSH, contextual power, Power & more overflow |
| Attention | `VmAttentionStack` | One expanded banner; lower issues as chips |
| Connect | `VmConnectHub` | Cinema, SSH, laptop path, NAT, export (Access tab) |
| Overview | Info cards, usage, compute, doctor one-liner | Live metrics only — no host FS or XML bloat |
| Tabs | `DetailTabs` — Access, Console, Doctor, … | Deep panels moved out of Overview |

## Action bar

**Always visible:** Open Cinema (primary), SSH, one contextual power button (Start / Shutdown / Resume).

**Power & more overflow:** Pause, reboot variants, NMI, force stop, Studio, Virt-Viewer, pop out, Delete.

Ask Zyra opens Spotlight with VM blockers and suggested intents (`vmDetailSpotlight.ts`).

## Attention stack

Priority order:

1. Pending config shutdown
2. Guest agent offline
3. Laptop NAT path incomplete

Rules: show **top priority expanded**; click chips to swap expanded banner; guest-agent dismiss persists per VM in `localStorage`.

## Connect hub

Single card (`data-testid="vm-daily-access"`) on Overview; full panel on **Access** tab (`?tab=access`).

Sections:

- **Connect** — Open Cinema, SSH, copy laptop command, copy IP
- **Laptop path** — progress bar + checklist for NAT VMs (`192.168.122.x`)
- **Guest ports** — expose top listeners inline
- **Hypervisor NAT** — collapsed on Overview; expanded on Access tab
- **Export** — spec JSON + domain XML (Access tab)

SSH user prefers cloud-init user (e.g. `ubuntu`) over stored prefs when available.

## Access tab

Primary tab for connectivity deep work:

- Full Connect hub with NAT panel and export
- Links to Guest security & ports and Guest health tabs
- Same NAT presets as Network → Hypervisor NAT and ConsoleHub Ops Shelf

## Slim Overview

**Removed from Overview** (deep links instead):

- Host resources panel → host detail link
- Full doctor panel → Doctor tab + one-line summary
- Domain XML editor → Settings tab
- AI terminal tips → Console tab
- Organization fields → Settings tab

**Kept:** desired state, lifecycle, project, backup cards; live usage bars; compute topology; Connect hub; attention stack.

## Tests

| Suite | Coverage |
|-------|----------|
| `web/e2e/platform-feature-matrix.spec.ts` | F01–F18 serial mock matrix |
| `web/e2e/platform-live-feature-matrix.spec.ts` | Live lab smoke (read-only) |
| `web/e2e/platform-vm-operator.spec.ts` | Connect hub, action bar, Access tab |

See [Platform feature QA matrix](platform-feature-qa.md) for feature IDs F01–F18.

## Related guides

- [VM daily access (Connect hub)](vm-daily-access.md)
- [Machina Cinema Mode](../machina-cinema-mode.md)
- [UX wiring & QA](../ux.md)

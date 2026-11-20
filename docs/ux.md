# Machina web UX conventions

Shared patterns for integration states, errors, and empty lists in `web/src/`.

## Integration phases

Optional backends use the same mental model:

| Phase | Meaning |
|-------|---------|
| `off` | Disabled in daemon config |
| `unreachable` | Configured but API not answering |
| `live` | Healthy; operational UI enabled |

Hooks:

- **Help → About** — top-nav **Help** menu (`?` shortcuts, **About** tab with [zyvor.dev](https://zyvor.dev) links and copyright)
- K8s — [`K8sConnectionErrorBanner`](../web/src/components/K8sConnectionErrorBanner.tsx) + [`k8sErrors.ts`](../web/src/utils/k8sErrors.ts)

Gate destructive or cloud-side actions on `phase === 'live'`. Nav and command palette may still list destinations with disabled labels (“Wire cloud first”).

## Primitives

| Component | Use when |
|-----------|----------|
| [`EmptyState`](../web/src/components/EmptyState.tsx) | Zero rows in a list; include primary CTA |
| [`PlatformEmptyState`](../web/src/components/platform/PlatformEmptyState.tsx) | Platform Mac pages — glass panel empty state with CTA |
| [`semanticColors.ts`](../web/src/utils/semanticColors.ts) | Status/task/host tone helpers — prefer over raw Tailwind green/amber/red |

### Semantic color helpers (Batch 57–60)

| Helper | Use when |
|--------|----------|
| `statusToneClass(tone)` | Inline text for ok / warn / error / info / neutral |
| `statusBadgeClasses(tone)` | Pill/chip backgrounds (host health, compliance grades) |
| `statusPillClasses(tone)` | Bordered action chips (K8s node ops, KubeVirt live console) |
| `statusSurfaceClasses(tone, extra?)` | Bordered callout panels (readiness, drift, destructive hints) |
| `statusDestructiveButtonClasses(extra?)` | Secondary destructive actions (Fleet Cloud delete/dissociate) |
| `hubLinkClasses()` | Platform hub links and cross-shell navigation accents (info tone) |
| `navActiveChipClasses()` | Active filter pills / segmented nav chips |
| `riskTone(risk)` | Firewall / security risk strings → critical/high → error, warning/medium → warn, low/info → ok |
| `utilizationBarClass(percent, thresholds?)` | Gauge/progress bars for CPU, memory, PSI, thermal |

**Intentionally unchanged:** primary CTAs (`bg-blue-600`), [`ChoiceCards`](../web/src/components/ChoiceCards.tsx) selection accents (wizard tone palette, not operational status), ApiDocs HTTP method colors, orange Zeus branding in Help.

**Help — Platform guide:** GlobalBar's **Machina** dropdown ("Platform guide…") opens the in-app Help dialog **Platform** tab via `dispatchOpenHelp('platform')` — the old separate menubar/classic Navbar Help menus (`PlatformMacAppMenus`, `Navbar`) are gone; both were unimported dead code and have been deleted.

**Machine Finder geography:** Mission Control ([`InfrastructureEarthView`](../web/src/components/platform/InfrastructureEarthView.tsx)) links to `/platform/hosts/finder` — four-column site → rack → host → VM browser aligned with `GET /api/v1/fleet/mission`.

**GPU Command Center:** [`PlatformGpuCommandCenter`](../web/src/pages/platform/PlatformGpuCommandCenter.tsx) at `/platform/gpu` — MIG/vGPU/CUDA profile chips from host tags, GPU VM inventory, and CUDA placement advisor (`GET /api/v1/fleet/gpu` + `GET /api/v1/ai/fleet/gpu-placement`).

**Maintenance Mission:** [`PlatformMaintenance`](../web/src/pages/platform/PlatformMaintenance.tsx) **Mission** tab (`?tab=mission`) — 7-step [`BuildStepTimeline`](../web/src/components/BuildStepTimeline.tsx) per host from `GET /api/v1/fleet/maintenance-mission`; operator-confirmed schedule / enter / exit / agent upgrade (no autonomous package apply). Default tab when fleet has pending updates.

**Infrastructure DNA:** [`InfrastructureDnaStrip`](../web/src/components/platform/InfrastructureDnaStrip.tsx) — score ring + grade + pillar chips from `GET /api/v1/fleet/dna` on Platform dashboard (power tier+) and Mission Control header.

**Infrastructure Earth globe (Phase 58 v3):** [`InfrastructureEarthGlobe`](../web/src/components/platform/InfrastructureEarthGlobe.tsx) — **WebGL** globe (lazy `three.js`) with **canvas 2D fallback**, per-site health markers, and a site legend (links to Machine Finder) on Mission Control and Machine Finder topology lens.

**Enterprise security strip:** [`EnterpriseSecurityStrip`](../web/src/components/platform/EnterpriseSecurityStrip.tsx) on advanced dashboard; vault sync failures use persistent [`ErrorBanner`](../web/src/components/ErrorBanner.tsx) on [`PlatformEnterprise`](../web/src/pages/platform/PlatformEnterprise.tsx).

| [`JsonInspector`](../web/src/components/platform/JsonInspector.tsx) | Power-user API payloads — human summary first, raw JSON behind toggle |
| [`PlatformIntegrationEmbeds`](../web/src/components/platform/PlatformIntegrationEmbeds.tsx) | Integrations hub — live Fleet Cloud/K8s inventory preview when backends are reachable |
| [`ErrorBanner`](../web/src/components/ErrorBanner.tsx) | Actionable failure with hints + optional copy |
| [`CopyButton`](../web/src/components/CopyButton.tsx) | Wire scripts, kubectl, verify commands |
| [`WizardStepper`](../web/src/components/WizardStepper.tsx) | Multi-step Create VM / Import VM |

## API errors (daemon JSON, HTML, codes)

The daemon returns `{ "error": "…", "error_code": "operation_failed" }` on failure. Proxies or down backend services may return **HTML** instead of JSON.

| Utility | Use when |
|---------|----------|
| [`formatHttpErrorBody`](../web/src/utils/apiError.ts) | Parsing a non-OK `fetch` body (used by [`client.ts`](../web/src/api/client.ts)) |
| [`parseResponseError`](../web/src/api/parseResponseError.ts) | Custom `fetch` calls outside `apiPost` / `readJsonObject` |
| [`formatUserError`](../web/src/utils/apiError.ts) | Any `catch (e: unknown)` shown in toasts or banners |

**Do not** display raw `response.text()` or bare `error_code` strings. Toasts run through [`Toast.tsx`](../web/src/components/Toast.tsx), which sanitizes error messages globally.

Page loads: set `loadError` state and show [`ErrorBanner`](../web/src/components/ErrorBanner.tsx) with domain hints ([`libvirtHints.ts`](../web/src/utils/libvirtHints.ts) or [`k8sErrors.ts`](../web/src/utils/k8sErrors.ts)). Use `Promise.allSettled` when loading multiple catalogs so one failure does not hide partial data.

Tests: `cd web && npm test` ([`apiError.test.ts`](../web/src/utils/apiError.test.ts)). E2E: `cd web && npm run test:e2e` (Playwright, mocked API).

Rust/TUI: [`core/src/api_error.rs`](../core/src/api_error.rs) mirrors web formatting; TUI HTTP client uses it for status bar messages.

Multi-host VM list: configure `[libvirt] extra_uris` in daemon config; VMs from remote URIs appear with a connection badge (read-only federation; lifecycle on primary/dual connections only).

## Developer / API Console

- [`PlatformDeveloper.tsx`](../web/src/pages/platform/PlatformDeveloper.tsx) — SDK tab + **API Console** (OpenAPI try-it for all controller routes)
- [`PlatformApiConsole.tsx`](../web/src/components/platform/PlatformApiConsole.tsx) — **Controller | Host** tabs, OpenAPI try-it, agent/ws hints
- Generate specs: `node scripts/generate-openapi.mjs` → [`docs/openapi-controller.json`](openapi-controller.json), [`docs/openapi-daemon.json`](openapi-daemon.json)
- Coverage gate: `cd web && npm run api-ux-coverage:check` (see [`docs/api-ux-coverage.json`](api-ux-coverage.json))

## Live UX → API verification (P12)

Proves buttons and page loads hit working backends on a real host (not mocked Playwright).

```bash
# Regenerate page matrix (~200+ routes, tabs, and dynamic detail sweeps)
node scripts/generate-ux-live-manifest.mjs

# Against remote host (requires PAM credentials)
VSPASS='…' ./scripts/e2e-live-ux-remote.sh operator <ephemeral-ip>

# Included in deploy when --e2e and VSPASS are set (skip with --skip-live-ux)
VSPASS='…' ./scripts/deploy-remote.sh operator HOST --quick --e2e
```

Report: `docs/ux-wiring-live-report.json` (written by the live UX run, not committed) — pass/fail per route with API failure details.

Optional GitHub Actions: workflow_dispatch job `live-ux` (secrets: `LIVE_HOST`, `LIVE_USER`, `LIVE_PASS`).

## Overall UX polish (P14)

Cross-shell presentation pass after backend wiring (P6–P13).

| Area | Pattern |
|------|---------|
| Initial fetch | [`PageSkeleton`](../web/src/components/PageSkeleton.tsx) — never a blank content area |
| Zero rows | [`PlatformEmptyState`](../web/src/components/platform/PlatformEmptyState.tsx) (Platform) or [`EmptyState`](../web/src/components/EmptyState.tsx) (Classic/Fleet Cloud/K8s) with at least one CTA |
| API payloads | [`JsonInspector`](../web/src/components/platform/JsonInspector.tsx) — summary/table first; raw JSON behind toggle |
| Domain failures | [`formatUserError`](../web/src/utils/apiError.ts) + hints ([`libvirtHints`](../web/src/utils/libvirtHints.ts), [`hostErrorPresentation`](../web/src/utils/hostErrorPresentation.ts), [`storageErrorPresentation`](../web/src/utils/storageErrorPresentation.ts)) |

**E2E (mocked):**

```bash
cd web && npm run build && npm run test:e2e -- e2e/platform-full.spec.ts e2e/shell-bridge.spec.ts
```

**Manual QA additions (P14):**

| Scenario | Check |
|----------|--------|
| Platform Storage discover with no hosts | Structured banner + link to Hosts |
| Platform Host detail, agent offline | Remediation links to Enroll + classic Node |
| K8s Workloads explorer | Table/summary default; raw JSON toggle |

## Platform shell (current — GlobalBar / ChapterBar, no sidebar)

The Mac menubar, desktop tabs, sidebar / icon rail and dock described in earlier UX waves are gone
(`de22b0e9` replaced them) — don't reintroduce them. Full contract:
[design/APPLE-UX-CONTRACT.md](design/APPLE-UX-CONTRACT.md).

| Layer | Role | When visible |
|-------|------|--------------|
| **GlobalBar** (`components/nav/GlobalBar.tsx`) | The only navigation: Z mark + wordmark, Machine Finder link (attention dot), six product groups with Netra mega-panel flyouts, Search (⌘K), Control Center, New VM, **Machina** dropdown (About, Settings, Mission Control, tier switch, Add Host, Sign Out) | Always on authenticated routes |
| **MobileNavSheet** (`components/nav/MobileNavSheet.tsx`) | The same groups as collapsible sections in a full-height sheet | ≤1024px, opened from the burger |
| **ChapterBar** (`components/nav/ChapterBar.tsx`) | Cross-links between hubs | **Normal:** hidden · **Power:** hub roots only · **Advanced:** unless route uses in-page [`DetailTabs`](../web/src/components/platform/DetailTabs.tsx) — see [`shouldShowContextBar`/`suppressContextBar`](../web/src/utils/platformNavRegistry.ts) |
| **Page header** | [`PlatformPageChrome`](../web/src/components/platform/PlatformPageChrome.tsx) → [`PageLayout`](../web/src/components/PageLayout.tsx) | Every platform page |
| **DetailTabs** | In-app sections with `?tab=` | Tab-heavy pages only |
| **Fleet Cloud pills** | Section switch | Primary Overview/Instances/Images/Volumes/Create + **More** ([`FleetCloudSubNav`](../web/src/components/FleetCloudSubNav.tsx)) |

**Overflow menus:** GlobalBar's product flyouts, ChapterBar's **More**, and Fleet Cloud **More** share
[`PlatformFloatingMenu`](../web/src/components/platform/mac/PlatformFloatingMenu.tsx) (portaled,
`role="menu"`) and [`PlatformMenuItem`](../web/src/components/platform/mac/PlatformMenuItem.tsx) row
tokens (`--surface-hover`, `--accent-soft`).

**Browse lists:** [`TahoeListKit`](../web/src/components/platform/tahoe/TahoeListKit.tsx) — `TahoeToolbar`, `TahoeTableWrap`, `TahoeListEmpty`.

**Zeus/Zyra status:** pending approvals and attention surface in GlobalBar's attention dot / product
flyouts, not a separate island component; [`ZyraAmbientBar`](../web/src/components/ai/ZyraAmbientBar.tsx)
is hidden on `/platform/*`.

**Dashboard / Story:** [`MissionControlPage`](../web/src/pages/platform/MissionControl/MissionControlPage.tsx) (`/platform`); classic [`Dashboard.tsx`](../web/src/pages/Dashboard.tsx) and [`FleetCloudOverview`](../web/src/pages/FleetCloudOverview.tsx) use `apple-story-stack` / `apple-metric-band` / destination rows (not card grids).

**Glass tokens:** `--glass-panel` in `main.css` unifies [`MacGlassPanel`](../web/src/components/platform/mac/PlatformMacUi.tsx), `.tahoe-glass-card`, and `.platform-mac-panel`. Work panels prefer `.tahoe-glass-card`.

**Naming:** Product shell stays **Machina**; the AI assistant is always **Zyra** ([`aiBrand.ts`](../web/src/config/aiBrand.ts), [`AskZyraButton`](../web/src/components/ai/AskZyraButton.tsx)). Use "Ask Zyra" — not "Ask Machina" or "Copilot" — in user-facing AI entry points.

## Dashboard & shell

- **Help** (top bar) — dropdown: **Keyboard shortcuts** (`?`) and **About** ([`HelpDialog.tsx`](../web/src/components/HelpDialog.tsx), [`ZyvorAbout.tsx`](../web/src/components/ZyvorAbout.tsx)): [zyvor.dev](https://zyvor.dev), product links, copyright © 2026, documentation hub.
- [`Dashboard.tsx`](../web/src/pages/Dashboard.tsx) — apple.com Story home (metric band + explore destination rows); deep links to Platform / VM Center / K8s / Containers
- [`Hero.tsx`](../web/src/components/Hero.tsx) — capability badges reflect phase, not config-only
- Command palette — always list Fleet Cloud routes; sublabel when not live

## Theming

Product themes map to Zeus: **Tahoe Light** (`light`, **default**), **Classic Blue** (`dark`, opt-in), plus Machina **steel** / **aurora** / **rack**. Prefer semantic tokens (`--text-*`, `--accent`, `--apple-*`). Avoid hard-coded `slate-*` / `sky-*` / `blue-600` / slate hex (`#64748b`, `#94a3b8`, …).

**Interactive blue:** apple.com `#0071e3` (CTAs, `--accent`, focus, active nav). Light links `#0066cc`; dark links `#2997ff`. Hover/pressed `#0077ed` / `#006edb`.

**Box type (Apple shop):** cards inherit Apple TV buy-flow fonts — light `#1d1d1f` / `#6e6e73` / `#86868b`; dark `#f5f5f7` / `#a1a1a6` / `#86868b`. SoT: [design/DAYLIGHT-CONTRACT.md](design/DAYLIGHT-CONTRACT.md) + `zeus-parity.css`. Immersive Console Hub / VNC keep carbon chrome but the same `--text-*` tokens.

**Story type ([AirPods](https://www.apple.com/airpods/)):** `.apple-display` / `.apple-lede` SF Pro Display metrics.

**Shell:** `GlobalBar` (top bar + flyout), `MobileNavSheet` and `ChapterBar` inside `PlatformLayout`. The sidebar, dock, Mac menubar and icon rail are gone — do not reintroduce them. See [design/APPLE-UX-CONTRACT.md](design/APPLE-UX-CONTRACT.md).

**Look layer:** `web/src/styles/netra-look.css` is loaded last and holds the current look. Namespace new tokens `--nl-*`; do not add global element resets (Tailwind utilities are layered, so an unlayered rule would clobber them).

**Audit:** `node scripts/ux-audit.mjs` (see the contract) sweeps every route for contrast, clipping, headings, names, tap targets and focus indicators.

## Login & accessibility

- [`Login.tsx`](../web/src/pages/Login.tsx) — apple.com **machina** wordmark via [`PremiumLoginShell`](../web/src/components/PremiumLoginShell.tsx); Zyvor mark icon-only; SSO when OIDC enabled; host label from `window.location.hostname`
- Login CSS: [`zyvor-premium-login.css`](../web/src/styles/zyvor-premium-login.css) (Apple Account paper + SF Pro / system display stack)
- Top bar (authenticated): the Zyvor mark links home; the **Machina** dropdown holds About, Settings, Mission Control, tier switch, Add Host and Sign Out
- Dialogs restore focus to their trigger via [`useCaptureTrigger`](../web/src/hooks/useCaptureTrigger.ts) (used by `useFocusTrap`, `GlassModal`, `ConfirmDialog`); capture happens during render because a child's `autoFocus` moves focus before effects run
- **URL behavior:** the login page renders outside `BrowserRouter` when unauthenticated. `/` and `/login` both work. After auth, [`AuthContext`](../web/src/contexts/AuthContext.tsx) replaces `/login` with `/`, and authenticated routes register `<Navigate from="/login" to="/" />` so bookmarked `/login` never shows 404
- **Zyra AI shell:** [`AiProvider`](../web/src/contexts/AiContext.tsx) must stay **inside** `BrowserRouter` (uses `useLocation` / `useParams` for ambient route context)
- E2E: [`smoke.spec.ts`](../web/e2e/smoke.spec.ts) — `authenticated /login redirects to dashboard`
- [`NotFound.tsx`](../web/src/pages/NotFound.tsx) — dashboard styling + Ctrl+K hint
## Manual QA (Phase 7)

| Scenario | Check |
|----------|--------|
| Zero VMs | VM list EmptyState |
| K8s API down | K8s overview + workloads banner |
| OIDC enabled | Login: SSO primary, password secondary |
| Sign in at `/login` | Lands on dashboard (`/`), not 404 |
| `prefers-reduced-motion` | Login: no orb animation |
| Light / dark / steel | Dashboard, Login (**machina** wordmark), one Fleet Cloud page (pill nav + More) |

## UX Wave 9 (2026-06)

Compound operating surfaces across admin, security, and observability — briefing stats, `DetailTabs`, `GlassDataTable`, and `PlatformEmptyState` on zero-row panels.

| Route / area | Pattern |
|------------|---------|
| `/platform/users` | `OperatingSurfaceLayout` + user/workspace tabs + `GlassDataTable` |
| `/platform/projects` | Spaces briefing + project table empty state |
| `/platform/api-keys`, `/platform/webhooks` | Command bar + glass table empty states |
| `/platform/observability` | SLO / traces / metrics lens tabs + briefing stats |
| `/platform/events` | Audit log stream + source tabs |
| `/platform/zeus/security/ports`, `/services`, `/activity` | `SecurityLensLayout` + exposure briefing |
| `/platform/applications`, `/enroll`, `/placement`, `/recommendations`, `/fleet-snapshots` | Operations compound surfaces (Wave 3) |
| Hub roots (`/platform/infrastructure`, `/workloads`, `/operations`) | `PageSkeleton` while async stats load |
| Classic (`/vms`, `/networks`, `/events`, `/jobs`) | `PageSkeleton` + `EmptyState` CTAs |
| Fleet Cloud detail routes | `PageSkeleton` replaces full-page `Loader2` |
| K8s overview / workloads | Full-page `PageSkeleton` on initial fetch |

E2E: `cd web && npm run test:e2e -- e2e/platform-admin-ux.spec.ts e2e/platform-observability-ux.spec.ts e2e/platform-security-ux.spec.ts e2e/classic-operator-ux.spec.ts`

Build: `cd web && npm run build`. Deploy: `./scripts/deploy remote user@host --quick`.

**Live E2E (optional):**

```bash
PLAYWRIGHT_LIVE_URL=https://HOST:5092 PLAYWRIGHT_LIVE_USER=operator PLAYWRIGHT_LIVE_PASS=… \
  npm run test:e2e -- e2e/live-host.spec.ts
```

Includes PAM login at `/login` → dashboard when credentials are set.

## Docs

- **Apple.com / Zeus UX contract:** [`design/APPLE-UX-CONTRACT.md`](design/APPLE-UX-CONTRACT.md)
- **Daylight / Tahoe Light tokens:** [`design/DAYLIGHT-CONTRACT.md`](design/DAYLIGHT-CONTRACT.md)
- Platform VM detail UX: [`guides/platform-vm-detail-ux.md`](guides/platform-vm-detail-ux.md)
- Connect hub & daily access: [`guides/vm-daily-access.md`](guides/vm-daily-access.md)
- Feature QA matrix (F01–F13): [`guides/platform-feature-qa.md`](guides/platform-feature-qa.md)
- Cinema / Studio modes: [`machina-cinema-mode.md`](machina-cinema-mode.md)
- Client presentation decks: [`client-presentations/`](client-presentations/) · `./scripts/generate-client-presentation-pdfs.sh`

# Daylight Contract — Machina light theme (Zeus OS 1:1)

Shipped **default** light shell is apple.com white / Magichromatic Tahoe Light
(`html.apple-light`, `html[data-theme='tahoe-light']`) — port from
`../zeus-os/ui/src/index.css` (sibling repo).
The dark theme is opt-in via Appearance (light and dark are the only two themes).

Live SoT in Machina: **`web/src/styles/netra-look.css`** (the final layer, loaded after `main.css`;
it wins by source order and holds every current look override). Underneath it:
`zeus-parity.css` (Zeus HSL tokens), `machina-daylight.css` (light tokens, superseded where they
conflict) and `machina-apple-ux.css` (Story / Browse primitives). See the "Where the look lives"
section of [APPLE-UX-CONTRACT.md](APPLE-UX-CONTRACT.md) for the load order and the rules for the
final layer.

Dark product theme is **Classic Blue** (`html[data-ui-shell='default']`, `data-theme=tahoe`).
It now follows the Netra dark theme: a flat `#000` canvas (no ambient wash or mesh), `#1d1d1f`
cards, and the same apple.com `#0071e3` CTA as light — the old "Mist Blue on graphite" CTAs are
gone.

Login is one centered composition ([`PremiumLoginShell`](../../web/src/components/PremiumLoginShell.tsx)
+ [`zyvor-premium-login.css`](../../web/src/styles/zyvor-premium-login.css), chapter geometry
overridden in `netra-look.css`): hero wordmark **machina**, title, tagline, then the sign-in card, on
white. The page stays light in every theme.

Shell / nav / tiers: [APPLE-UX-CONTRACT.md](APPLE-UX-CONTRACT.md).

## Palette (iPhone 17 / 17 Pro)

| Finish | Role |
|---|---|
| **apple.com Blue** (`#0071e3`) | Intent / primary CTAs / links / focus / active nav (`--primary` / `--accent` / `--link`) |
| **Sage** | Confirmed-good only |
| **Lavender** | AI accent sparingly |
| **White** | Cards / elevated work surfaces (`.tahoe-glass-card`) |
| **Black** | Graphite text / Classic Blue canvas |
| **Cosmic Orange** (Pro) | Warn / deviation |
| **Silver** (Pro) | Neutral tracks / hairlines |

Hover / pressed CTA blues (AirPods CSS): `#0077ed` / `#006edb`. Dark-mode links: `#2997ff`.

### Box type (Apple shop `.form-selector`)

Card / panel font colors match [Apple TV buy flow](https://www.apple.com/in/shop/buy-tv/apple-tv-4k/64gb) 1:1.
Cascade winners: `zeus-parity.css` `:root` + `html[data-theme='tahoe-light']` (mirrored in `machina-daylight.css` / `.light-glass`).
`.tahoe-glass-card` / `.glass-card` set `color: var(--text-primary)`.

| Token | Light | Dark |
|---|---|---|
| `--text-primary` | `#1d1d1f` | `#f5f5f7` |
| `--text-secondary` | `#6e6e73` | `#a1a1a6` |
| `--text-muted` | `#86868b` | `#86868b` |

Status / deviation icons may use `--amber` / `--verdant` / `--accent` — never slate hex for body type.

### Neutral hairlines, shadows and fills (light)

The light tokens are neutral, not blue-grey tinted. They are set in `netra-look.css` on the light
selectors and read by the 800+ `border-[var(--apple-hairline)]` call sites:

| Token | Light | Dark |
|---|---|---|
| `--apple-hairline`, `--hairline-1/2`, `--divider` | `rgba(0,0,0,.10)` | `rgba(255,255,255,.12)` |
| `--apple-fill-tertiary` (`--nl-fill`) | `rgba(0,0,0,.04)` | `rgba(255,255,255,.08)` |
| `--surface-1` (card), `--surface-sunken` | `#fff`, `#f5f5f7` | (unchanged) |
| `--surface-hover` (`--nl-fill-hover`) | `#f5f5f7` | `#2c2c2e` |
| `--shadow-1/2/3` | `0 1px 2px .04`, `0 4px 16px .08`, `0 12px 40px .14` (black) | (unchanged) |
| `--focus-ring` | `0 0 0 4px rgba(0,113,227,.3)` | (Zeus ring) |
| `--text-faint` | `#7d7d83` | `#8e8e93` |

### Text-safe accents

The bright accent hues are fills and strokes (~2:1 as text on white). Text uses these; in the light
theme the pale Tailwind text shades (`text-emerald-400`, `text-orange-400`, … and the 500/600 status
shades) are remapped to them centrally, outside the dark islands.

| Token | Light | Dark |
|---|---|---|
| `--nl-accent-green-text` | `#1e7b34` | `#30d158` |
| `--nl-accent-amber-text` | `#8b4b00` | `#ff9f0a` |
| `--nl-accent-red-text` | `#b3261e` | `#ff6961` |
| `--nl-accent-cyan-text` | `#0a6b99` | `#64d2ff` |
| `--nl-accent-purple-text` | `#8a3fb5` | `#bf5af2` |
| `--nl-status-warn-text` / `-bg` / `-border` | `#8b3d00` / `#fff6ed` / `#ffd8ba` | `#ffd60a` / 12% amber / 35% amber |

### Story type ([AirPods](https://www.apple.com/airpods/))

`.apple-display` / `.apple-lede` use SF Pro Display metrics (clamp ~40–64px hero, ~17–21px lede) with the box-type ink colors above.

## Law 0 — Elevation runs up, not down

| Level | Classic Blue | Tahoe Light |
|---|---|---|
| page | `#000` graphite | flat white `#ffffff` (apple.com Support paper — see below) |
| panel | `#1d1d1f` | `#FFFFFF` + hairline (no separate grey panel) |
| card | lighter still | `#FFFFFF` + hairline (18px radius) |
| popover | lightest | white + soft shadow |

**Reference: support.apple.com guide pages** (e.g. the iPod touch User Guide) —
captured live: body `#ffffff` flat, top nav translucent near-white
`rgba(250,250,252,0.8)`, hero directly under the nav a single subtle
`linear-gradient(#ffffff 0%, #f2f2f2 100%)`, everything else flat white. The
earlier "Magichromatic washes" (multiple full-opacity saturated radial-
gradient color blooms layered on `.tahoe-mesh` / `.mac-desktop-root` on every
page) were an intentional earlier direction but read as visibly diverging
from apple.com's own product pages — superseded by this flatter, single-
gradient treatment. `--page-bg` / `--surface-0` are `#ffffff`; `.tahoe-mesh`
is disabled (`opacity: 0`) in Tahoe Light.

**A grey panel on a white page is always a bug.**

## Law 1 — Color is deviation

Nominal values are graphite. Mist Blue = intent only.

## Invariants

- Terminals / Console Hub / Host SSH cinema stay carbon islands (chrome / backdrop) — **type** still uses Apple `--text-*` (no `#64748b` / slate hex).
- Theme attribute: `html[data-theme=tahoe-light]` when Tahoe Light is selected.
- Classic Blue: `html[data-ui-shell=default]`.
- Story pages use `apple-story-stack` / `apple-metric-band` — not dense bordered tile grids.
- Primary CTAs use apple.com blue `#0071e3` (`.btn-primary` / `.tahoe-btn-primary`). Do not invent other blues (`blue-600`, Mist as CTA fill).

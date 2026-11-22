# Social assets

| File | What it is | Rebuild |
|---|---|---|
| `machina-share-card.html` / `.jpg` | 1200x630 card: README hero and GitHub social preview. Embeds a live dashboard capture from `docs/ux/machina-dashboard.png` | `./docs/social/build-social-card.sh` |
| `machina-social-card.html` / `.jpg` | 1600x900 (16:9) card for LinkedIn and X: install, run, console, fleet, operate in five steps | `./docs/social/build-social-card.sh` |
| `fluxvm-results-card.html` / `.jpg`, `fluxvm-results-share.html` / `.jpg` | 1600x900 and 1200x630 FluxVM results: vhost-net throughput, HA across hosts, idle balloon, regression. Numbers from [RESULTS.md](../../scripts/regression/RESULTS.md) | `./docs/social/build-social-card.sh` |
| `readme/*.html` | README cards written to `docs/ux/`: `readme-architecture.jpg`, `readme-capabilities.jpg`, `readme-vs-openstack.jpg` | `./docs/social/readme/build.sh` |

Both scripts need Google Chrome and macOS `sips` (already on a Mac); nothing is installed.
The look follows the [Netra](https://github.com/zyvorai/netra) social cards in Machina's colors
(interactive blue `#0071e3`, ink `#1d1d1f`, violet accent).

Every claim on the cards maps to code in this repository. Licence wording follows `LICENSE`:
the Zyvor Production Licence v2.0, with a commercial
subscription required for production ([SUBSCRIPTION-MODEL.md](../SUBSCRIPTION-MODEL.md)).

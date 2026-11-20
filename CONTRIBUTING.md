# Contributing

1. Open an issue describing the behavior or change before sending a large PR.
2. Read [CLAUDE.md](CLAUDE.md) for the architecture (daemon, controller, agent, web) and [docs/ENGINEERING_ONBOARDING.md](docs/ENGINEERING_ONBOARDING.md) for the day-one setup.
3. The Rust workspace needs Linux libvirt headers. Build on a Linux host or with `./scripts/deploy-remote.sh HOST USER --remote-build`; the web UI builds on macOS.
4. Before submitting a PR, run the same gates CI runs:
   - `make fmt-check`, `make lint`, `make test`
   - `cd web && npm run build && npm test`
   - `./scripts/add-spdx.py --check`
5. Contributions are accepted under the [Zyvor Production License v1.0](LICENSE). New source files carry the two-line header `./scripts/add-spdx.py` writes:

   ```
   // Copyright 2026 Zyvor AI Labs · https://zyvor.dev
   // SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
   ```

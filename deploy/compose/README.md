# Controller demo with Docker Compose

```bash
cp .env.example .env && $EDITOR .env         # JWT secret: openssl rand -hex 32
cp /path/to/release/machina-controller .      # from the release assets
docker compose up -d --build
curl -fsS http://127.0.0.1:5093/api/v1/health
```

The published agent files (`machinactl dist publish`) are not part of this image: copy `machina-agent`, `machina-bpfd` and
their `.service` files plus a `SHA256SUMS` into the `machina-data` volume under `dist/` if nodes should install from it, or install the agent from packages.
This is a demo layout; production controllers run from packages (see `docs/QUICKSTART.md`).

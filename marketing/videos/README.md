# Machina customer demo videos

Caption-only cinematic shorts for YouTube + [hypersdk-web](https://zyvor.dev/machina).

## Quick start

```bash
# 1) Record against the live lab (requires network + PAM creds)
cd /path/to/machina
PLAYWRIGHT_LIVE_URL=https://80.79.5.173:5092 \
PLAYWRIGHT_LIVE_USER=sus PLAYWRIGHT_LIVE_PASS=max \
  node marketing/videos/scripts/record-demos.mjs all
# or a single clip: … record-demos.mjs 01

# 2) Edit → publish-ready MP4 + thumbnails
python3 marketing/videos/scripts/edit-demos.py all
# (shell wrapper still available: ./marketing/videos/scripts/edit-demos.sh all)

# 3) Upload finals from marketing/videos/out/final/ (see youtube/UPLOAD.md)
```

## Layout

| Path | Purpose |
|------|---------|
| `clips.json` | Manifest (titles, VM IDs, tags) |
| `scripts/record-demos.mjs` | Playwright capture → `out/raw/` |
| `scripts/edit-demos.sh` | ffmpeg titles/captions/music → `out/final/` |
| `captions/*.srt` | Burned-in lower-thirds |
| `youtube/UPLOAD.md` | YouTube titles/descriptions |
| `out/` | Gitignored binaries |

## Website

After upload, paste YouTube IDs into `hypersdk-web/src/data/product-demo-videos.ts` (`MACHINA_DEMO_VIDEOS`).

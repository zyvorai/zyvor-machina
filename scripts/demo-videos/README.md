# Machina wow-reel pipeline

Playwright recordings against the live Machina platform, stitched with ffmpeg
title-card/caption composites. Live client runbook: [`CLIENT_DEMO.md`](CLIENT_DEMO.md).

## Requirements

- `playwright` (symlinked `node_modules`) with the `chrome` channel installed
- `ffmpeg` (overlay/fade/concat) · `python3`
- Live Machina (`MACH_URL`, default tunnel `https://127.0.0.1:15092`)

```bash
# Prefer SSH tunnel (avoids PAM rate limits):
ssh -f -N -L 15092:127.0.0.1:5092 sus@212.8.248.187
export MACH_URL=https://127.0.0.1:15092 MACH_USER=sus MACH_PASS=max
export MACH_LINUX_VM=chrome-e2e-vm MACH_WINDOWS_VM=win10-msedge
```

## Golden image demos (Linux + Windows, separate)

```bash
node render-cards-golden.mjs
rm -rf raw/seg-golden-linux raw/seg-golden-windows
node seg-golden-linux.mjs     # Create VM from linux-ubuntu-golden
node seg-golden-windows.mjs   # stops win10 golden briefly, clones, restarts
./build-golden.sh             # short highlight cuts (~20s)
./build-golden-full.sh        # FULL process (entire take + title/outro) → Desktop
```

Saved templates live on the hypervisor under `/var/lib/machina/templates/`
(`linux-ubuntu-golden.json`, `windows-win10-golden.json`).

**Full process on screen (what the full MP4 shows):**

1. Open **Disk images** / the live golden guest  
2. Go to **Create VM** (`/create`)  
3. Choose **Clone from golden image**  
4. Pick **Saved template** (`linux-ubuntu-golden` or `windows-win10-golden`)  
5. **Thin clone** disk mode → name the new VM  
6. **Create VM from golden image** (virt-install + overlay)  
7. Open the new guest in the VM list  
8. (Windows) restart the golden after cloning  

## Recording

```bash
node render-cards.mjs
rm -rf raw/seg01-login raw/seg02-dashboard-console raw/seg03-cinema-wall
node seg01-login.mjs                # -> raw/seg01-login/*.webm
node seg02-dashboard-console.mjs    # Mission Control + Linux VM
node seg03-cinema-wall.mjs          # Cinema Linux + Windows + Live Wall
./build.sh                          # -> out/machina-wow-reel.mp4 (+ linkedin-1080p)
cp out/machina-wow-reel*.mp4 ~/Desktop/
```

Each seg script prints `mark()` timestamps from process start — NOT from when
`recordVideo` starts. Translate to video-relative time before editing
`extract_clip` offsets in `build.sh`: `offset = last_mark - actual_video_duration`.
Spot-check with `ffmpeg -ss <t> -frames:v 1`.

## Hardware Studio reel (NIC / video / USB·PCI / firmware)

```bash
export MACH_URL=https://127.0.0.1:15092 MACH_USER=sus MACH_PASS=max
export MACH_VM_NAME=chrome-e2e-vm
node render-cards-hw.mjs
rm -rf raw/seg-hw-feats
node seg-hw-feats.mjs                 # classic Devices tab → Hardware drawer
# Translate mark() times → video-relative (see build-hw.sh header), then:
./build-hw.sh /tmp/hw-marks.env       # → out/ + Desktop/Machina-Demo-Videos/machina-hardware/
```

## Publishing

Upload `out/machina-wow-reel.mp4` (or `machina-hardware-wow-reel.mp4`) with `upload-wow-reel.py`
(Zeus OS OAuth token), copy under `~/Desktop/`, wire the YouTube id into
`../hypersdk-web/src/data/product-demo-videos.ts`, and link from
[`docs/CUSTOMER_SITE_READINESS.md`](../../docs/CUSTOMER_SITE_READINESS.md).

Current public cuts:
- Desktop wow: https://youtu.be/GYjvbKwUufA
- GuestKit agent: https://youtu.be/LYoqOye3P3I

# YouTube upload checklist — Machina · Product demos

Playlist suggestion: **Machina · Product demos**

Upload files from `marketing/videos/out/final/*.mp4` with matching thumbs from `out/thumbs/`.
After upload, paste each `youtubeId` into `hypersdk-web/src/data/product-demo-videos.ts`.

---

## 01 — Machina Cinema

- **File:** `01-machina-cinema.mp4`
- **Title:** Machina Cinema — full-screen VM like a player
- **Description:**
  ```
  Open a running desktop guest in Machina Cinema — player-style HUD, Fit/Fill scale, idle-hide controls, and one-click screenshot.

  Machina is Zyvor's enterprise Linux hypervisor platform for libvirt / QEMU / KVM.

  → https://zyvor.dev/machina
  → Request a trial: https://zyvor.dev/contact?intent=trial&product=machina

  #Machina #KVM #libvirt #VNC #Zyvor
  ```
- **Tags:** Machina, Cinema, VNC, libvirt, KVM, Zyvor, virtualization
- **youtubeId:** _paste after upload_

## 02 — Mission Control Live Wall

- **File:** `02-mission-control-live-wall.mp4`
- **Title:** Machina Mission Control — Live Preview Wall
- **Description:** Watch live VM thumbnails on the Mission Control wall, then jump straight into Cinema. https://zyvor.dev/machina
- **Tags:** Machina, Mission Control, fleet, NOC, Zyvor
- **youtubeId:** _paste after upload_

## 03 — Machine Finder Gallery

- **File:** `03-machine-finder-gallery.mp4`
- **Title:** Machina Machine Finder Gallery → Open Cinema
- **Description:** Browse machines as a cinematic gallery of posters, then open Cinema from a tile. https://zyvor.dev/machina
- **Tags:** Machina, Machine Finder, Gallery, Cinema, Zyvor
- **youtubeId:** _paste after upload_

## 04 — Studio + Ops Shelf

- **File:** `04-studio-ops-shelf.mp4`
- **Title:** Machina Studio + Ops Shelf — engineer console mode
- **Description:** Switch from Cinema to Studio for Display, Serial, and Shell lenses, then open the Ops Shelf for health and access. https://zyvor.dev/machina
- **Tags:** Machina, Studio, Ops Shelf, serial, SSH, Zyvor
- **youtubeId:** _paste after upload_

## 05 — VM Detail → Cinema

- **File:** `05-vm-detail-connect-cinema.mp4`
- **Title:** Machina VM Detail — Connect hub to Cinema in one motion
- **Description:** From the platform VM detail page: health, Connect checklist, and Open Cinema. https://zyvor.dev/machina
- **Tags:** Machina, VM detail, Connect, Cinema, Zyvor
- **youtubeId:** _paste after upload_

## 06 — Snapshots

- **File:** `06-snapshot-restore.mp4`
- **Title:** Machina Snapshots — create and protect a running VM
- **Description:** Create a libvirt snapshot from the platform UI and confirm the task completes. https://zyvor.dev/machina
- **Tags:** Machina, snapshot, backup, libvirt, Zyvor
- **youtubeId:** _paste after upload_

## 07 — NAT SSH Expose

- **File:** `07-nat-ssh-expose.mp4`
- **Title:** Machina — expose guest SSH on the hypervisor
- **Description:** Expose guest SSH via hypervisor NAT and copy the host:port path for laptop access. https://zyvor.dev/machina
- **Tags:** Machina, SSH, NAT, port forward, Zyvor
- **youtubeId:** _paste after upload_

## 08 — Ask Zeus

- **File:** `08-ask-zeus-spotlight.mp4`
- **Title:** Machina Ask Zeus / Spotlight — ops AI at your fingertips
- **Description:** Open Zeus Spotlight with page context and ask for an operational assist. https://zyvor.dev/machina
- **Tags:** Machina, Zeus, AI, Spotlight, Zyvor
- **youtubeId:** _paste after upload_

## 09 — Network Canvas

- **File:** `09-network-canvas.mp4`
- **Title:** Machina Network Canvas — service topology at a glance
- **Description:** Pan and zoom the Network Canvas to see how hosts, networks, and VMs connect. https://zyvor.dev/machina
- **Tags:** Machina, network, topology, canvas, Zyvor
- **youtubeId:** _paste after upload_

## 10 — Create VM → Cinema

- **File:** `10-create-vm-cinema.mp4`
- **Title:** Machina — Create Ubuntu VM and open Cinema
- **Description:** Guided New VM wizard: Ubuntu 24.04 Small → running → Open Cinema. https://zyvor.dev/machina
- **Tags:** Machina, create VM, Ubuntu, Cinema, cloud-init, Zyvor
- **youtubeId:** _paste after upload_

---

## After upload

Send a JSON map like:

```json
{
  "01": "xxxxxxxxxxx",
  "02": "xxxxxxxxxxx"
}
```

Then we replace `TODO_YOUTUBE_ID_*` placeholders in `hypersdk-web/src/data/product-demo-videos.ts`.

## Rendered durations (local finals)
- `01-machina-cinema.mp4` — 46s · 2.3 MB
- `02-mission-control-live-wall.mp4` — 34s · 2.3 MB
- `03-machine-finder-gallery.mp4` — 32s · 2.9 MB
- `04-studio-ops-shelf.mp4` — 51s · 3.6 MB
- `05-vm-detail-connect-cinema.mp4` — 32s · 2.3 MB
- `06-snapshot-restore.mp4` — 28s · 2.3 MB
- `07-nat-ssh-expose.mp4` — 25s · 2.4 MB
- `08-ask-zeus-spotlight.mp4` — 26s · 2.4 MB
- `09-network-canvas.mp4` — 29s · 1.9 MB
- `10-create-vm-cinema.mp4` — 74s · 5.5 MB

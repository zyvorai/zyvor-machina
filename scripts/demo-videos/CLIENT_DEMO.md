# Client live demo runbook (~12 minutes)

**Host:** lab `212.8.248.187` (tunnel `https://127.0.0.1:15092` → `:5092`)  
**Guests:** `chrome-e2e-vm` (Linux + GuestKit) · `win10-msedge` (Windows golden)  
**Positioning:** pilot-ready **single-host** KVM/libvirt — not multi-host HA or Atlas as guaranteed.

**One-liner:** “Machina is the operator desktop for KVM — Cinema for the console, GuestKit for the guest, platform control plane for fleet day-2.”

## Pre-flight (2 min)

- [ ] Both VMs **running** (`virsh list`)
- [ ] Linux GuestKit/QGA healthy on `chrome-e2e-vm`
- [ ] Windows VNC paints in Cinema for `win10-msedge`
- [ ] Prefer localhost/tunnel login (avoids PAM rate limits)
- [ ] Dark theme, Normal desktop density; clear noisy toasts
- [ ] No NMI, destructive disk ops, or Zeus lockdown apply during the take

## Beats

| # | Time | Route / action | Say |
|---|------|----------------|-----|
| 1 | 0:00 | macOS-style login → `/platform` | Product identity in first 10s |
| 2 | 1:00 | `/platform` (Mission Control home) → Launchpad → **Live Preview Wall** | Fleet at a glance |
| 3 | 2:30 | Machine Finder / `/platform/vms` → Open Cinema | Poster / gallery UX |
| 4 | 4:00 | Cinema on **`chrome-e2e-vm`** (`…/consolehub?mode=cinema`) | Hero console moment |
| 5 | 6:00 | One GuestKit live op (guest health / network apply) | Agent value (matrix **29/29**) |
| 6 | 7:30 | Cinema / VNC on **`win10-msedge`** | Cross-OS; RDP firstboot staged via hyper2kvm |
| 7 | 9:00 | Pause/resume or reboot → back **running** | Day-2 ops confidence |
| 8 | 10:30 | Zeus AI landing or cost/capacity one-liner → pilot checklist | Soft close + next step |

## Do not demo live

- Zeus lockdown **apply** (dry-run only if asked)
- Offline Windows `enable-rdp` while NTFS dirty / mid-backup (force-stop first; prefer clean shutdown)
- Atlas / Ceph (disabled on lab)
- Real host-loss HA drill

## Leave-behinds

- [Customer site readiness](../../docs/CUSTOMER_SITE_READINESS.md)
- Decks: [01 Business value](../../docs/client-presentations/01-business-value.html) · [08 Cinema](../../docs/client-presentations/08-consolehub-cinema-studio.html) · [03 Architecture](../../docs/client-presentations/03-technical-architecture.html)
- Hardware Studio reel: https://youtu.be/MXX4d46_4Ew
- GuestKit reel: https://youtu.be/LYoqOye3P3I
- Desktop wow reel: https://youtu.be/GYjvbKwUufA (`~/Desktop/machina-wow-reel.mp4`, rebuild via README)
- Linux golden → Create VM (full): https://youtu.be/8PRWTuKZ3kM
- Windows golden → Create VM (full): https://youtu.be/wtrp6pp8I0s

## Known demo caveats (lab)

- Prefer Cinema on **`chrome-e2e-vm`** for the hero console shot (Ubuntu login).
- `win10-msedge` may show Windows Recovery after heavy offline GuestKit/disk work — reboot before the Windows beat, or show it Running on Live Preview Wall / VM cards only.
- Live Preview Wall thumbnails may say “unavailable” while **Open Cinema** still works.

## Golden image → Create VM (separate reels)

Lab templates (on hypervisor):

| Template | `base_image` |
|----------|----------------|
| `linux-ubuntu-golden` | `/var/lib/libvirt/images/ubuntu-demo-src.qcow2` |
| `windows-win10-golden` | `/var/lib/libvirt/images/win10-msedge.qcow2` |

**UI path:** `/create` → **Clone from golden image** → **Saved template** → thin clone → **Create VM from golden image**.

Windows: stop the golden first (backing qcow2 must not be exclusively locked), then restart it after the clone. Delete demo clones when finished so the golden can run again.

```bash
cd scripts/demo-videos
export MACH_URL=https://127.0.0.1:15092 MACH_USER=sus MACH_PASS=max
node render-cards-golden.mjs
rm -rf raw/seg-golden-linux raw/seg-golden-windows
node seg-golden-linux.mjs
node seg-golden-windows.mjs
./build-golden.sh
# → out/machina-golden-linux.mp4 · out/machina-golden-windows.mp4 (+ Desktop copies)
```

## Record the LinkedIn wow cut


```bash
cd scripts/demo-videos
export MACH_URL=https://127.0.0.1:15092   # or https://212.8.248.187:5092
export MACH_USER=sus MACH_PASS=max
export MACH_LINUX_VM=chrome-e2e-vm MACH_WINDOWS_VM=win10-msedge
node render-cards.mjs
rm -rf raw/seg01-login raw/seg02-dashboard-console raw/seg03-cinema-wall
node seg01-login.mjs
node seg02-dashboard-console.mjs
node seg03-cinema-wall.mjs
# calibrate extract_clip offsets in build.sh from mark() → video-relative times
./build.sh
cp out/machina-wow-reel*.mp4 ~/Desktop/
```

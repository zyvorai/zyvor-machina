#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Upload Hardware Studio reel to YouTube (public) and wire into ../hypersdk-web.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

VIDEO="${1:-out/machina-hardware-wow-reel.mp4}"
SECRETS="${YT_CLIENT_SECRETS:-$HOME/Desktop/Zeus-OS-Demo-Videos/client_secret_369624289033-fphugk292q2ff61cb25aqetljgus4aoi.apps.googleusercontent.com.json}"
TOKEN="${YT_TOKEN:-$HOME/Desktop/Zeus-OS-Demo-Videos/.youtube-upload/token.json}"
PY="${YT_PYTHON:-$HOME/Desktop/zyvor-demo-videos/.venv/bin/python}"
WEB_ROOT="${HYPERSDK_WEB:-$HOME/tt/hypersdk-web}"
TITLE='Machina — Libvirt Hardware Studio (NIC · video · USB/PCI · firmware)'
DESC='Machina Hardware Studio on a live KVM host — open any guest and edit day-2 hardware without virsh XML spelunking.

Recorded against lab 212.8.248.187 after the hw-feats wave:
• Hardware summary (CPU, firmware, video, disk bus, NIC, guest agent)
• Display & Access / Storage / Network editors
• Live NIC model switch (virtio ↔ e1000e)
• USB & PCI passthrough inventory
• Firmware & Security (BIOS/UEFI, TPM)
• Video model switch (virtio ↔ qxl) via define_xml fallback

→ https://zyvor.dev/machina
→ Trial: https://zyvor.dev/contact?intent=trial&product=machina

#Machina #KVM #libvirt #Hardware #Zyvor #virtualization #virtio'

[[ -f "$VIDEO" ]] || { echo "missing $VIDEO — run ./build-hw.sh first"; exit 2; }
[[ -x "$PY" ]] || PY=python3

"$PY" upload-wow-reel.py \
  --video "$VIDEO" \
  --client-secrets "$SECRETS" \
  --token "$TOKEN" \
  --privacy public \
  --state out/youtube-state.json \
  --state-key machina-hardware-wow-reel \
  --title "$TITLE" \
  --description "$DESC"

ID=$(python3 - <<'PY'
import json
from pathlib import Path
st=json.loads(Path("out/youtube-state.json").read_text())
print(st["machina-hardware-wow-reel"]["id"])
PY
)
URL="https://youtu.be/${ID}"
echo "YouTube: $URL"

mkdir -p "$HOME/Desktop/Machina-Demo-Videos/machina-hardware"
cat > "$HOME/Desktop/Machina-Demo-Videos/machina-hardware/youtube-upload.json" <<EOF
{
  "id": "$ID",
  "url": "$URL",
  "title": "$TITLE",
  "file": "$(cd "$(dirname "$VIDEO")" && pwd)/$(basename "$VIDEO")",
  "privacy": "public",
  "constant": "MACHINA_HARDWARE_STUDIO_DEMO_VIDEO"
}
EOF

DEMO_TS="$WEB_ROOT/src/data/product-demo-videos.ts"
if [[ -f "$DEMO_TS" ]] && ! grep -q "MACHINA_HARDWARE_STUDIO_DEMO_VIDEO" "$DEMO_TS"; then
  python3 - <<PY
from pathlib import Path
p = Path("$DEMO_TS")
text = p.read_text()
block = '''
/** Machina — Hardware Studio live reel (Aug 2026), lab 212.8.248.187. */
export const MACHINA_HARDWARE_STUDIO_DEMO_VIDEO: ProductDemoVideo = {
  youtubeId: '$ID',
  title: '$TITLE',
  watchUrl: 'https://www.youtube.com/watch?v=$ID',
  description:
    'Libvirt Hardware Studio on a real KVM guest: summary → display/storage/network editors → live NIC model switch → USB/PCI inventory → firmware → video model (virtio↔qxl).',
};

'''
anchor = "export const MACHINA_WOW_REEL_DEMO_VIDEO"
if anchor not in text:
    raise SystemExit(f"anchor missing in {p}")
text = text.replace(anchor, block + anchor, 1)
old = """const MACHINA_DEMO_VIDEOS_ALL: ProductDemoVideo[] = [
  MACHINA_WOW_REEL_DEMO_VIDEO,"""
new = """const MACHINA_DEMO_VIDEOS_ALL: ProductDemoVideo[] = [
  MACHINA_WOW_REEL_DEMO_VIDEO,
  MACHINA_HARDWARE_STUDIO_DEMO_VIDEO,"""
if old not in text:
    raise SystemExit("MACHINA_DEMO_VIDEOS_ALL list anchor missing")
text = text.replace(old, new, 1)
p.write_text(text)
print(f"Wired {p}")
PY
  echo "Next: cd $WEB_ROOT && git add src/data/product-demo-videos.ts && git commit && git push"
else
  echo "hypersdk-web already has MACHINA_HARDWARE_STUDIO_DEMO_VIDEO or path missing — paste id $ID manually"
fi

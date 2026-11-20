// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { chromium } from "playwright";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const outDir = join(__dirname, "png");

function titleHtml(kicker, line1, line2) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  html,body{margin:0;padding:0;width:1920px;height:1080px;background:#070b14;
    background-image:radial-gradient(ellipse 60% 50% at 20% 0%, rgba(37,99,235,0.22), transparent 55%),
                      radial-gradient(ellipse 50% 45% at 85% 100%, rgba(245,158,11,0.14), transparent 55%);
    font-family:-apple-system,'SF Pro Text',Segoe UI,sans-serif;display:flex;align-items:center;justify-content:center;}
  .wrap{text-align:center;padding:0 200px;}
  .kicker{font-family:ui-monospace,'SF Mono',monospace;color:#38bdf8;letter-spacing:0.3em;font-size:24px;font-weight:700;margin-bottom:33px;text-transform:uppercase;}
  h1{color:#f4f7fc;font-size:60px;font-weight:800;letter-spacing:-0.02em;margin:0 0 27px;line-height:1.15;}
  p{color:#93a4bd;font-size:30px;font-weight:400;margin:0;line-height:1.5;max-width:1280px;}
  .rule{width:96px;height:5px;background:linear-gradient(90deg,#2563eb,#f59e0b);margin:39px auto 0;border-radius:5px;}
  </style></head><body>
  <div class="wrap">
    ${kicker ? `<div class="kicker">${kicker}</div>` : ""}
    <h1>${line1}</h1>
    ${line2 ? `<p>${line2}</p>` : ""}
    <div class="rule"></div>
  </div>
  </body></html>`;
}

function captionHtml(text) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  html,body{margin:0;padding:0;width:1920px;height:220px;background:transparent;
    font-family:-apple-system,'SF Pro Text',Segoe UI,sans-serif;display:flex;align-items:center;justify-content:center;}
  .bar{width:1770px;background:rgba(7,11,20,0.90);border:1px solid rgba(245,158,11,0.35);border-radius:21px;
    padding:27px 45px;display:flex;align-items:center;gap:21px;box-shadow:0 18px 45px rgba(0,0,0,0.45);}
  .dot{width:14px;height:14px;border-radius:50%;background:#f59e0b;flex:none;box-shadow:0 0 15px #f59e0b;}
  .text{color:#f1f5f9;font-size:30px;font-weight:500;line-height:1.4;}
  </style></head><body>
  <div class="bar"><div class="dot"></div><div class="text">${text}</div></div>
  </body></html>`;
}

const cards = [
  { file: "g-linux-title", kicker: "MACHINA", line1: "Linux Golden Image → New VM", line2: "One Packer-style qcow2. Thin-clone as many identical workers as you need." },
  { file: "g-win-title", kicker: "MACHINA", line1: "Windows Golden Image → New VM", line2: "hyper2kvm Win10 golden. Clone from saved template — same Create VM flow." },
  { file: "g-linux-outro", kicker: "", line1: "Linux workers from one golden.", line2: "Create VM → Clone from golden image → saved template" },
  { file: "g-win-outro", kicker: "", line1: "Windows workers from one golden.", line2: "Shut off the golden briefly, thin-clone, start both." },
];

const captions = [
  { file: "cap-g-disks", text: "Disk images on the hypervisor — keep one canonical Linux golden qcow2." },
  { file: "cap-g-create", text: "Create VM → Clone from golden image — saved template or direct qcow2 backing." },
  { file: "cap-g-tmpl-linux", text: "Pick linux-ubuntu-golden — thin clone (qemu-img backing → small overlay)." },
  { file: "cap-g-spawn-linux", text: "Name the worker and create — virt-install clones from the golden in seconds." },
  { file: "cap-g-win-show", text: "Windows golden win10-msedge — the image you clone from, not the clone itself." },
  { file: "cap-g-tmpl-win", text: "windows-win10-golden template points at the Win10 qcow2 on the host." },
  { file: "cap-g-spawn-win", text: "Create from golden — new Windows guest with a thin overlay disk." },
];

const browser = await chromium.launch({ channel: "chrome" });
for (const t of cards) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  await page.setContent(titleHtml(t.kicker, t.line1, t.line2));
  await page.screenshot({ path: `${outDir}/${t.file}.png` });
  await page.close();
  console.log("title:", t.file);
}
for (const c of captions) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 220 } });
  await page.setContent(captionHtml(c.text));
  await page.screenshot({ path: `${outDir}/${c.file}.png`, omitBackground: true });
  await page.close();
  console.log("caption:", c.file);
}
await browser.close();

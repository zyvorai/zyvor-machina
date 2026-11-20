#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Title/caption cards for Machina Hardware feats reel.
 */
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { mkdirSync } from 'node:fs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const outDir = join(__dirname, 'png-hw');
mkdirSync(outDir, { recursive: true });

function titleHtml(kicker, line1, line2) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  html,body{margin:0;padding:0;width:1920px;height:1080px;background:#070b14;
    background-image:radial-gradient(ellipse 60% 50% at 20% 0%, rgba(37,99,235,0.22), transparent 55%),
                      radial-gradient(ellipse 50% 45% at 85% 100%, rgba(56,189,248,0.14), transparent 55%);
    font-family:-apple-system,'SF Pro Text',Segoe UI,sans-serif;display:flex;align-items:center;justify-content:center;}
  .wrap{text-align:center;padding:0 200px;}
  .kicker{font-family:ui-monospace,'SF Mono',monospace;color:#38bdf8;letter-spacing:0.3em;font-size:24px;font-weight:700;margin-bottom:33px;text-transform:uppercase;}
  h1{color:#f4f7fc;font-size:58px;font-weight:800;letter-spacing:-0.02em;margin:0 0 27px;line-height:1.15;}
  p{color:#93a4bd;font-size:28px;font-weight:400;margin:0;line-height:1.5;max-width:1280px;}
  .rule{width:96px;height:5px;background:linear-gradient(90deg,#2563eb,#38bdf8);margin:39px auto 0;border-radius:5px;}
  </style></head><body>
  <div class="wrap">
    ${kicker ? `<div class="kicker">${kicker}</div>` : ''}
    <h1>${line1}</h1>
    ${line2 ? `<p>${line2}</p>` : ''}
    <div class="rule"></div>
  </div>
  </body></html>`;
}

function captionHtml(text) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  html,body{margin:0;padding:0;width:1920px;height:220px;background:transparent;
    font-family:-apple-system,'SF Pro Text',Segoe UI,sans-serif;display:flex;align-items:center;justify-content:center;}
  .bar{width:1770px;background:rgba(7,11,20,0.90);border:1px solid rgba(56,189,248,0.28);border-radius:21px;
    padding:27px 45px;display:flex;align-items:center;gap:21px;box-shadow:0 18px 45px rgba(0,0,0,0.45);}
  .dot{width:14px;height:14px;border-radius:50%;background:#38bdf8;flex:none;box-shadow:0 0 15px #38bdf8;}
  .text{color:#f1f5f9;font-size:28px;font-weight:500;line-height:1.4;}
  </style></head><body>
  <div class="bar"><div class="dot"></div><div class="text">${text}</div></div>
  </body></html>`;
}

const cards = [
  {
    file: 'hw00-title',
    kicker: 'MACHINA · HARDWARE',
    line1: 'Libvirt Hardware Studio, Live',
    line2: 'video · disk bus · NIC model · USB/PCI · CD-ROM · firmware · virtiofs',
  },
  { file: 'hw01-login', kicker: '', line1: 'Sign In to Platform' },
  { file: 'hw02-summary', kicker: '', line1: 'Hardware Summary on the VM' },
  { file: 'hw03-display', kicker: '', line1: 'Display & Console Graphics' },
  { file: 'hw04-storage', kicker: '', line1: 'Disks, Bus & CD-ROM' },
  { file: 'hw05-network', kicker: '', line1: 'Live NIC Model Switch' },
  { file: 'hw06-hostdev', kicker: '', line1: 'USB & PCI Passthrough' },
  { file: 'hw07-firmware', kicker: '', line1: 'Firmware, TPM & Security' },
  {
    file: 'hw08-outro',
    kicker: '',
    line1: 'Day-2 hardware without XML spelunking.',
    line2: 'zyvor.dev/machina',
  },
];

const captions = [
  { file: 'cap-hw-login', text: 'One login — open any KVM guest and edit its hardware from the control plane.' },
  { file: 'cap-hw-summary', text: 'CPU, memory, firmware, video, disk bus, NIC, guest agent — structured, not raw virsh dumpxml.' },
  { file: 'cap-hw-display', text: 'Display & Access — VNC/SPICE graphics for Cinema and Studio consoles.' },
  { file: 'cap-hw-storage', text: 'Storage — disks, bus types, CD-ROM insert/eject, without leaving the VM page.' },
  { file: 'cap-hw-nic', text: 'Live NIC model switch — virtio ↔ e1000e via nic.tune, applied on the running guest.' },
  { file: 'cap-hw-hostdev', text: 'Host Devices — browse hypervisor USB/PCI and attach passthrough when the host has free devices.' },
  { file: 'cap-hw-firmware', text: 'Firmware & Security — BIOS/UEFI flip, TPM, RNG. Restart-gated where libvirt requires it.' },
];

const browser = await chromium.launch({ channel: 'chrome' });
for (const t of cards) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  await page.setContent(titleHtml(t.kicker, t.line1, t.line2));
  await page.screenshot({ path: `${outDir}/${t.file}.png` });
  await page.close();
  console.log('title:', t.file);
}
for (const c of captions) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 220 } });
  await page.setContent(captionHtml(c.text));
  await page.screenshot({ path: `${outDir}/${c.file}.png`, omitBackground: true });
  await page.close();
  console.log('caption:', c.file);
}
await browser.close();

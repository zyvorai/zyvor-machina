#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Record Machina Hardware feats UX via classic VM Devices tab
 * (Platform hardware drawer + live NIC model + video model + USB/PCI).
 *
 * Env: MACH_URL MACH_USER MACH_PASS MACH_VM_NAME
 */
import { openLoggedIn, closeAndSave, waitReady, BASE, LINUX_VM } from './lib.mjs';

const VM_NAME = process.env.MACH_VM_NAME || LINUX_VM || 'chrome-e2e-vm';
const t0 = Date.now();
const mark = (label) => console.log(`t=${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

async function dismissNoise(page) {
  for (const sel of [
    'button:has-text("Dismiss")',
    '[aria-label="Close"]',
    'button:has-text("Close")',
  ]) {
    const b = page.locator(sel).first();
    if (await b.count()) await b.click({ timeout: 800 }).catch(() => {});
  }
}

async function waitForHwCard(page) {
  const hwCard = page.getByTestId('classic-platform-hardware');
  for (let i = 0; i < 45; i++) {
    if (await hwCard.count()) {
      try {
        await hwCard.waitFor({ state: 'visible', timeout: 2000 });
        return hwCard;
      } catch {
        /* keep waiting */
      }
    }
    if (i % 5 === 0) mark(`waiting-devices-${i}`);
    await page.waitForTimeout(2000);
  }
  throw new Error('classic-platform-hardware never appeared');
}

async function clickSection(page, label) {
  const scope = page.locator('[role="dialog"], aside').last();
  const btn = scope.getByRole('button', { name: new RegExp(`^${label}`, 'i') }).first();
  if (await btn.count()) {
    await btn.scrollIntoViewIfNeeded();
    await btn.click();
  } else {
    await page.getByRole('button', { name: new RegExp(`^${label}`, 'i') }).first().click({ timeout: 12000 });
  }
  await page.waitForTimeout(1200);
}

const { browser, context, page } = await openLoggedIn('raw/seg-hw-feats');
await waitReady(page);
mark('post-login');

await page.goto(`${BASE}/vms/${encodeURIComponent(VM_NAME)}?tab=devices`, {
  waitUntil: 'domcontentloaded',
  timeout: 60000,
});
mark('vm-devices-nav');

const hwCard = await waitForHwCard(page);
await hwCard.scrollIntoViewIfNeeded();
await page.waitForTimeout(2500);
await dismissNoise(page);
mark('hardware-summary');

await page.getByTestId('classic-vm-hardware-edit').click({ timeout: 15000 });
await page.waitForTimeout(2500);
mark('hardware-drawer');

const drawer = page.getByTestId('vm-hardware-drawer');
await drawer.waitFor({ timeout: 20000 });
// Scroll the drawer body so the Edit Hardware CTA (below summary rows) is in view.
await drawer.locator('.overflow-y-auto, .flex-1').first().evaluate((el) => {
  el.scrollTop = el.scrollHeight;
}).catch(() => {});
await page.waitForTimeout(500);
const editInDrawer = drawer.getByTestId('vm-hardware-edit');
await editInDrawer.waitFor({ timeout: 20000 });
await editInDrawer.scrollIntoViewIfNeeded();
await editInDrawer.click({ force: true });
await page.waitForTimeout(2000);
mark('edit-open');

await clickSection(page, 'Display & Access');
await page.waitForTimeout(2500);
mark('display');

await clickSection(page, 'Storage');
await page.waitForTimeout(3000);
mark('storage');

await clickSection(page, 'Network');
await page.waitForTimeout(2000);
mark('network');

const editNic = page.getByRole('button', { name: /^Edit$/i }).first();
if (await editNic.count()) {
  await editNic.click();
  await page.waitForTimeout(800);
  const modelSel = page.locator('select').filter({ has: page.locator('option[value="e1000e"]') }).first();
  if (await modelSel.count()) {
    await modelSel.selectOption('e1000e');
    await page.waitForTimeout(600);
    const apply = page.getByRole('button', { name: /^Apply$/i }).first();
    if (await apply.count()) {
      await apply.click();
      await page.waitForTimeout(3500);
      mark('nic-e1000e');
    }
  }
  const editNic2 = page.getByRole('button', { name: /^Edit$/i }).first();
  if (await editNic2.count()) {
    await editNic2.click().catch(() => {});
    await page.waitForTimeout(600);
    const modelSel2 = page.locator('select').filter({ has: page.locator('option[value="virtio"]') }).first();
    if (await modelSel2.count()) {
      await modelSel2.selectOption('virtio');
      const apply2 = page.getByRole('button', { name: /^Apply$/i }).first();
      if (await apply2.count()) {
        await apply2.click();
        await page.waitForTimeout(3500);
        mark('nic-virtio');
      }
    }
  }
} else {
  mark('nic-edit-missing');
}

await clickSection(page, 'Host Devices');
await page.waitForTimeout(3500);
mark('hostdev');

await clickSection(page, 'Firmware & Security');
await page.waitForTimeout(3000);
mark('firmware');

// Close editors so classic Video model dialog is visible
await page.keyboard.press('Escape').catch(() => {});
await page.waitForTimeout(600);
await page.keyboard.press('Escape').catch(() => {});
await page.waitForTimeout(800);

const videoBtn = page.getByRole('button', { name: /Video model/i }).first();
if (await videoBtn.count()) {
  await videoBtn.scrollIntoViewIfNeeded();
  await videoBtn.click({ force: true });
  await page.waitForTimeout(2000);
  mark('video-dialog');
  const dlg = page.locator('[role="dialog"]').filter({ hasText: /video/i }).last();
  const sel = dlg.locator('select').first();
  if (await sel.count()) {
    await sel.selectOption('qxl').catch(() => sel.selectOption({ label: /qxl/i }).catch(() => {}));
    await page.waitForTimeout(500);
    const applyVid = dlg.getByRole('button', { name: /^(Apply|Set|Save|Change|OK)/i }).first();
    if (await applyVid.count()) {
      await applyVid.click({ force: true });
      await page.waitForTimeout(3000);
      mark('video-qxl');
      await videoBtn.click({ force: true }).catch(() => {});
      await page.waitForTimeout(1000);
      const dlg2 = page.locator('[role="dialog"]').filter({ hasText: /video/i }).last();
      const sel2 = dlg2.locator('select').first();
      if (await sel2.count()) {
        await sel2.selectOption('virtio').catch(() => sel2.selectOption({ label: /virtio/i }).catch(() => {}));
        const apply2 = dlg2.getByRole('button', { name: /^(Apply|Set|Save|Change|OK)/i }).first();
        if (await apply2.count()) {
          await apply2.click({ force: true });
          await page.waitForTimeout(3000);
          mark('video-virtio');
        }
      }
    } else {
      mark('video-apply-missing');
      await page.keyboard.press('Escape').catch(() => {});
    }
  } else {
    mark('video-options-visible');
    await page.keyboard.press('Escape').catch(() => {});
  }
} else {
  mark('video-btn-missing');
}

const usbHead = page.getByText(/USB Devices/i).first();
if (await usbHead.count()) {
  await usbHead.scrollIntoViewIfNeeded();
  await page.waitForTimeout(2500);
  mark('usb-list');
}

await page.waitForTimeout(1500);
mark('done');
await closeAndSave(browser, context);
console.log('OK raw/seg-hw-feats');

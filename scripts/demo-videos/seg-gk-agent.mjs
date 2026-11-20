#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Record GuestKit agent live UX: health + TRIM + network apply + service start/stop.
 * Env: MACH_URL MACH_USER MACH_PASS MACH_VM_ID (platform UUID)
 */
import { openLoggedIn, closeAndSave, waitReady, BASE } from './lib.mjs';

const VM_ID = process.env.MACH_VM_ID || '3b2803c9-68e9-4235-b0f8-ef46a42c7a80';
const t0 = Date.now();
const mark = (label) => console.log(`t=${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

async function dismissNoise(page) {
  // Close toast / error banners that steal focus in the reel
  for (const sel of [
    'button:has-text("Dismiss")',
    '[aria-label="Close"]',
    'button:has-text("Close")',
  ]) {
    const b = page.locator(sel).first();
    if (await b.count()) await b.click({ timeout: 1000 }).catch(() => {});
  }
}

const { browser, context, page } = await openLoggedIn('raw/seg-gk-agent');
await waitReady(page);
mark('post-login');

await page.goto(`${BASE}/platform/vms/${VM_ID}?tab=guestHealth`, {
  waitUntil: 'domcontentloaded',
  timeout: 60000,
});
await page.waitForTimeout(3000);
await dismissNoise(page);

const healthTab = page.getByText(/^Guest health$/);
if (await healthTab.count()) await healthTab.first().click().catch(() => {});

// Initial load often sticks on "Testing guest agent…" — force refresh until actions appear.
for (let i = 0; i < 8; i++) {
  if (await page.getByRole('button', { name: /TRIM filesystems|Apply IP/i }).count()) break;
  const refresh = page.getByRole('button', { name: /Refresh guest info|^Refresh$/i });
  if (await refresh.count()) {
    await refresh.first().click().catch(() => {});
    mark(`health-refresh-${i}`);
  }
  await page.waitForTimeout(3500);
}
await page.getByRole('button', { name: /TRIM filesystems/i }).first().waitFor({ timeout: 60000 });
await page.waitForTimeout(2500);
mark('guest-health');

const trimBtn = page.getByRole('button', { name: /TRIM filesystems/i }).first();
await trimBtn.scrollIntoViewIfNeeded();
await trimBtn.click();
await page.waitForTimeout(4500);
mark('trim-clicked');

await page.getByText(/Guest network/i).first().scrollIntoViewIfNeeded();
await page.waitForTimeout(1200);
mark('network-visible');

const iface = page.locator('input[placeholder="enp1s0"]').first();
const cidr = page.locator('input[placeholder="192.168.122.50/24"]').first();
const gw = page.locator('input[placeholder="192.168.122.1"]').first();
await iface.fill('enp1s0');
await cidr.fill('192.168.122.56/24');
await gw.fill('192.168.122.1');
await page.waitForTimeout(1000);
await page.getByRole('button', { name: /Apply network|Apply IP \+ gateway/i }).first().click();
await page.waitForTimeout(8000);
mark('network-applied');

await page.goto(`${BASE}/platform/vms/${VM_ID}?tab=guestServices`, {
  waitUntil: 'domcontentloaded',
  timeout: 60000,
});
await page.waitForTimeout(2000);
const svcTab = page.getByText(/^Guest services$/);
if (await svcTab.count()) await svcTab.first().click().catch(() => {});

for (let i = 0; i < 8; i++) {
  if (await page.getByText(/machina-demo/i).count()) break;
  const refresh = page.getByRole('button', { name: /^Refresh$/i });
  if (await refresh.count()) await refresh.last().click().catch(() => {});
  mark(`services-refresh-${i}`);
  await page.waitForTimeout(3000);
}
await page.getByText(/machina-demo/i).first().waitFor({ timeout: 60000 });
await page.waitForTimeout(2500);
mark('guest-services');

const row = page.locator('div').filter({ hasText: /^machina-demo$/ }).first();
const demo = (await row.count())
  ? row
  : page.locator('div,li').filter({ hasText: /machina-demo/i }).first();
await demo.scrollIntoViewIfNeeded();

async function clickEnabled(label) {
  const btn = page.getByRole('button', { name: new RegExp(`^${label}$`, 'i') }).filter({ hasNot: page.locator('[disabled]') });
  // Prefer button in the demo row when present
  const scoped = demo.getByRole('button', { name: new RegExp(`^${label}$`, 'i') });
  const target = (await scoped.count()) ? scoped.first() : page.getByRole('button', { name: new RegExp(`^${label}$`, 'i') }).first();
  await target.waitFor({ state: 'visible', timeout: 30000 });
  for (let i = 0; i < 40; i++) {
    if (await target.isEnabled()) break;
    await page.waitForTimeout(500);
  }
  await target.click({ timeout: 15000 });
  await page.waitForTimeout(5500);
  mark(`service-${label.toLowerCase()}`);
}

await clickEnabled('Stop');
await clickEnabled('Start');

await page.waitForTimeout(2500);
mark('end');
await closeAndSave(browser, context);
console.log('seg-gk-agent done');

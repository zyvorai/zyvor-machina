#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Create a sprite with network_egress=true via the real dashboard (not a
 * bypass — exercises the daemon's actual auth + create flow), for a
 * host-side DHCP-lease check. Driven by scripts/sprite-verify-egress.sh;
 * prints SPRITE_ID=<id> on success for that script to parse.
 *
 * Env: MACH_URL MACH_USER MACH_PASS MACH_SPRITE_GOLDEN_IMAGE MACH_SPRITE_BACKEND
 */
import { openLoggedIn, closeAndSave, waitReady, BASE } from './lib.mjs';

const GOLDEN_IMAGE = process.env.MACH_SPRITE_GOLDEN_IMAGE || 'ubuntu-test';
const BACKEND = process.env.MACH_SPRITE_BACKEND || 'libvirt';

const { browser, context, page } = await openLoggedIn('raw/_e2e-egress-check');
await waitReady(page);
await page.goto(`${BASE}/sprites`, { waitUntil: 'domcontentloaded', timeout: 30000 });
await waitReady(page);
await page.waitForTimeout(1500);

await page.getByRole('button', { name: /New Sprite/i }).click();
await page.waitForTimeout(1000);
const dialog = page.locator('[role="dialog"]').last();
const goldenSelect = dialog.locator('#sprite-golden-image');
await goldenSelect.waitFor({ timeout: 15000 });
const available = await goldenSelect.locator('option').allTextContents();
if (!available.includes(GOLDEN_IMAGE)) {
  console.error(`FAIL: golden image '${GOLDEN_IMAGE}' not in the picker (available: ${available.join(', ') || '(none)'})`);
  await closeAndSave(browser, context);
  process.exit(1);
}
await goldenSelect.selectOption(GOLDEN_IMAGE);
await dialog.locator('#sprite-ttl').selectOption('900').catch(() => {});
await dialog.getByRole('button', { name: BACKEND === 'cloudhypervisor' ? /Cloud Hypervisor/i : /^Libvirt$/i }).click();
await dialog.getByRole('checkbox').check();
await page.waitForTimeout(300);
await dialog.getByRole('button', { name: /^Create$/ }).click();
await dialog.waitFor({ state: 'detached', timeout: 30000 });
await page.waitForTimeout(1500);

const toastText = await page.locator("text=/Sprite .* booting/").first().textContent().catch(() => null);
const match = toastText?.match(/Sprite '([a-f0-9-]+)' booting/);
if (!match) {
  console.error(`FAIL: no "Sprite '<id>' booting" toast seen (got: ${toastText ?? 'nothing'})`);
  await closeAndSave(browser, context);
  process.exit(1);
}
console.log(`SPRITE_ID=${match[1]}`);

await closeAndSave(browser, context);

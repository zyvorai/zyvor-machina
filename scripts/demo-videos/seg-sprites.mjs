#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Record the Sprites UX: instant disposable sandbox VMs, on both the
 * libvirt and Cloud Hypervisor backends.
 *
 * Env: MACH_URL MACH_USER MACH_PASS MACH_SPRITE_GOLDEN_IMAGE
 */
import { openLoggedIn, closeAndSave, waitReady, BASE } from './lib.mjs';

const GOLDEN_IMAGE = process.env.MACH_SPRITE_GOLDEN_IMAGE || 'ubuntu-test';
const t0 = Date.now();
const mark = (label) => console.log(`t=${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

async function createSprite(page, { backend }) {
  await page.getByRole('button', { name: /New Sprite/i }).click();
  await page.waitForTimeout(1000);
  mark(`modal-open-${backend}`);

  const dialog = page.locator('[role="dialog"]').last();
  const goldenSelect = dialog.locator('#sprite-golden-image');
  await goldenSelect.waitFor({ timeout: 15000 });
  await goldenSelect.selectOption(GOLDEN_IMAGE).catch(() => {});
  await page.waitForTimeout(600);
  mark(`golden-image-selected-${backend}`);

  await dialog.getByRole('button', { name: backend === 'cloudhypervisor' ? /Cloud Hypervisor/i : /^Libvirt$/i }).click();
  await page.waitForTimeout(500);
  mark(`backend-selected-${backend}`);

  await dialog.locator('#sprite-ttl').selectOption('1800').catch(() => {});
  await page.waitForTimeout(400);

  await dialog.getByRole('button', { name: /^Create$/ }).click();
  mark(`create-clicked-${backend}`);
  // Cloud Hypervisor materializes a full disk copy (its qcow2 backend
  // rejects backing-file overlays), which takes noticeably longer than
  // libvirt's instant COW clone — wait for the modal to actually close
  // rather than a fixed timeout that's only right for one backend.
  await dialog.waitFor({ state: 'detached', timeout: 45000 });
  mark(`sprite-live-${backend}`);
}

const { browser, context, page } = await openLoggedIn('raw/seg-sprites');
await waitReady(page);
mark('post-login');

await page.goto(`${BASE}/sprites`, { waitUntil: 'domcontentloaded', timeout: 30000 });
await waitReady(page);
await page.waitForTimeout(1500);
mark('sprites-page-empty');

await createSprite(page, { backend: 'libvirt' });
await page.waitForTimeout(1000);
mark('libvirt-sprite-listed');

await createSprite(page, { backend: 'cloudhypervisor' });
await page.waitForTimeout(500);
mark('both-sprites-listed');

// Hold on the populated list so the backend badges / vsock CID / expiry
// countdown are readable in the recording.
await page.waitForTimeout(4000);
mark('list-hold');

// Delete one sprite to show teardown.
const deleteBtn = page.getByLabel('Delete').first();
await deleteBtn.click();
await page.waitForTimeout(800);
mark('delete-confirm-open');
await page.locator('[role="dialog"]').getByRole('button', { name: /^Delete$/ }).click();
await page.waitForTimeout(2000);
mark('sprite-deleted');

await page.waitForTimeout(1500);
mark('done');
await closeAndSave(browser, context);
console.log('OK raw/seg-sprites');

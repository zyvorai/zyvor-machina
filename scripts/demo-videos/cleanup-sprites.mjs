#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** One-off: delete every sprite currently listed, so seg-sprites.mjs starts from an empty list. */
import { openLoggedIn, closeAndSave, waitReady, BASE } from './lib.mjs';

const { browser, context, page } = await openLoggedIn('raw/_cleanup-sprites');
await waitReady(page);
await page.goto(`${BASE}/sprites`, { waitUntil: 'domcontentloaded', timeout: 30000 });
await waitReady(page);
await page.waitForTimeout(1500);

let guard = 0;
while (guard++ < 10) {
  const deleteBtn = page.getByLabel('Delete').first();
  if (!(await deleteBtn.count())) break;
  await deleteBtn.click();
  await page.waitForTimeout(500);
  await page.locator('[role="dialog"]').getByRole('button', { name: /^Delete$/ }).click();
  await page.waitForTimeout(1500);
}
console.log(`cleanup done after ${guard - 1} deletes`);
await closeAndSave(browser, context);

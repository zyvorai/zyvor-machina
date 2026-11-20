#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Record the animated terminal-egress-demo.html as a video. */
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const htmlPath = join(__dirname, 'terminal-egress-demo.html');

const browser = await chromium.launch({ channel: 'chrome' });
const context = await browser.newContext({
  viewport: { width: 1920, height: 1080 },
  recordVideo: { dir: 'raw/seg-terminal-egress', size: { width: 1920, height: 1080 } },
});
const page = await context.newPage();
await page.goto(`file://${htmlPath}`, { waitUntil: 'load' });
await page.waitForFunction(() => window.__demoDone === true, { timeout: 60000 });
await page.close();
await context.close();
await browser.close();
console.log('OK raw/seg-terminal-egress');

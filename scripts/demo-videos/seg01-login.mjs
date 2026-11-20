// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { chromium } from "playwright";
import { BASE, USER, PASS, waitReady } from "./lib.mjs";

const t0 = Date.now();
const mark = (label) => console.log(`t=${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

const browser = await chromium.launch({ channel: "chrome" });
const context = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  ignoreHTTPSErrors: true,
  recordVideo: { dir: "raw/seg01-login", size: { width: 1440, height: 900 } },
});
const page = await context.newPage();

await page.goto(BASE, { waitUntil: "domcontentloaded", timeout: 30000 });
mark("login-page-ready");
await page.waitForTimeout(1500);
await page.locator("#login-username").click();
await page.locator("#login-username").type(USER, { delay: 100 });
await page.waitForTimeout(300);
await page.locator("#login-password").click();
await page.locator("#login-password").type(PASS, { delay: 100 });
await page.waitForTimeout(500);
await page.click('button:has-text("Sign in")');
mark("clicked-sign-in");
await page.waitForTimeout(5000);
for (let i = 0; i < 5; i++) {
  const onClassic = await page.getByText("Back to Platform").count();
  if (onClassic > 0) {
    await page.getByText("Back to Platform").first().click();
    await page.waitForTimeout(3000);
  } else break;
}
await waitReady(page);
mark("dashboard-ready");
await page.waitForTimeout(2000);
mark("hold-done");

await page.close();
await context.close();
await browser.close();
console.log("seg01-login done");

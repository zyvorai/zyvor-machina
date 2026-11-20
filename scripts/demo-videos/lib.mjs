// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { chromium } from "playwright";

/** Lab defaults — override with MACH_URL / MACH_USER / MACH_PASS. Prefer SSH tunnel. */
export const BASE = process.env.MACH_URL || "https://127.0.0.1:15092";
export const USER = process.env.MACH_USER || "sus";
export const PASS = process.env.MACH_PASS || "max";
export const LINUX_VM = process.env.MACH_LINUX_VM || "chrome-e2e-vm";
export const WINDOWS_VM = process.env.MACH_WINDOWS_VM || "win10-msedge";

export async function openLoggedIn(videoDir, { skipLogin = true } = {}) {
  const browser = await chromium.launch({ channel: "chrome" });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    ignoreHTTPSErrors: true,
    recordVideo: { dir: videoDir, size: { width: 1440, height: 900 } },
  });
  const page = await context.newPage();
  if (skipLogin) {
    await page.goto(BASE, { waitUntil: "domcontentloaded", timeout: 30000 });
    await page.waitForTimeout(2000);
    await page.fill("#login-username", USER);
    await page.fill("#login-password", PASS);
    await page.click('button:has-text("Sign in")');
    await page.waitForTimeout(5000);
    for (let i = 0; i < 5; i++) {
      const onClassic = await page.getByText("Back to Platform").count();
      if (onClassic > 0) {
        await page.getByText("Back to Platform").first().click();
        await page.waitForTimeout(3000);
      } else break;
    }
  }
  return { browser, context, page };
}

export async function waitReady(page, timeout = 15000) {
  await page
    .waitForFunction(
      () =>
        !/scanning fleet|loading/i.test(document.body.innerText) &&
        document.body.innerText.trim().length > 100,
      { timeout }
    )
    .catch(() => {});
}

export async function goPlatform(page) {
  const url = page.url();
  if (!/\/platform/.test(url)) {
    await page.goto(`${BASE.replace(/\/$/, "")}/platform`, {
      waitUntil: "domcontentloaded",
      timeout: 30000,
    }).catch(() => {});
    await page.waitForTimeout(2000);
    for (let i = 0; i < 3; i++) {
      const onClassic = await page.getByText("Back to Platform").count();
      if (onClassic > 0) {
        await page.getByText("Back to Platform").first().click();
        await page.waitForTimeout(2500);
      } else break;
    }
  }
  await waitReady(page);
}

/** Click the first visible match for a VM name (classic list, finder, mission cards). */
export async function openVmByName(page, name) {
  const loc = page.getByText(name, { exact: false }).first();
  await loc.click({ timeout: 12000 });
  await page.waitForTimeout(2500);
}

export async function wakeConsoleCanvas(page) {
  const canvas = page.locator("canvas").first();
  if (await canvas.count().then((c) => c > 0).catch(() => false)) {
    const box = await canvas.boundingBox().catch(() => null);
    if (box) {
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
      await page.waitForTimeout(400);
      await page.keyboard.press("Enter").catch(() => {});
      return;
    }
  }
  await page.mouse.click(720, 450);
}

export async function closeAndSave(browser, context) {
  const page = context.pages()[0];
  await page.close();
  await context.close();
  await browser.close();
}

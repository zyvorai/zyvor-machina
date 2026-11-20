// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {
  openLoggedIn,
  closeAndSave,
  waitReady,
  goPlatform,
  wakeConsoleCanvas,
  BASE,
} from "./lib.mjs";

const t0 = Date.now();
const mark = (label) => console.log(`t=${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

const LINUX_ID = process.env.MACH_LINUX_VM_ID || "3b2803c9-68e9-4235-b0f8-ef46a42c7a80";
const WINDOWS_ID = process.env.MACH_WINDOWS_VM_ID || "90843de5-a79a-4a31-8e7f-bf139a504603";
const root = BASE.replace(/\/$/, "");

const { browser, context, page } = await openLoggedIn("raw/seg03-cinema-wall");
await goPlatform(page);
mark("post-login-ready");

async function openCinema(vmId, label) {
  await page.goto(`${root}/platform/vms/${vmId}/consolehub?mode=cinema`, {
    waitUntil: "domcontentloaded",
    timeout: 45000,
  });
  await page.waitForTimeout(4000);
  mark(`${label}-cinema-nav`);
  // Wait for noVNC canvas to appear (guest display)
  for (let i = 0; i < 20; i++) {
    const n = await page.locator("canvas").count().catch(() => 0);
    if (n > 0) break;
    await page.waitForTimeout(500);
  }
  await wakeConsoleCanvas(page);
  await page.waitForTimeout(4500);
  mark(`${label}-cinema-hold`);
}

await openCinema(LINUX_ID, "linux");
await openCinema(WINDOWS_ID, "windows");

await page.goto(`${root}/platform/mission-control/live`, {
  waitUntil: "domcontentloaded",
  timeout: 30000,
});
await waitReady(page);
mark("wall-ready");
await page.waitForTimeout(3500);
mark("wall-hold-done");

await closeAndSave(browser, context);
console.log("seg03-cinema-wall done");

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { openLoggedIn, closeAndSave, waitReady, goPlatform, LINUX_VM } from "./lib.mjs";

const t0 = Date.now();
const mark = (label) => console.log(`t=${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

const { browser, context, page } = await openLoggedIn("raw/seg02-dashboard-console");
await goPlatform(page);
mark("post-login-ready");

// Platform home IS Mission Control / Command Center (not /platform/mission-control)
await page.mouse.move(700, 280);
await page.waitForTimeout(2800);
mark("hero-hold-done");
await page.mouse.wheel(0, 420);
await page.waitForTimeout(2200);
mark("launchpad-hold-done");
await page.mouse.wheel(0, 280);
await page.waitForTimeout(1600);
mark("hosts-hold-done");

// Machine Finder / VMs gallery
await page.getByText("Machine Finder", { exact: false }).first().click({ timeout: 8000 }).catch(async () => {
  await page.goto(page.url().replace(/\/platform.*/, "/platform/vms"), {
    waitUntil: "domcontentloaded",
    timeout: 30000,
  });
});
await waitReady(page);
await page.waitForTimeout(2000);
mark("finder-ready");

await page.getByText(LINUX_VM, { exact: false }).first().click({ timeout: 12000 }).catch(() => {});
await page.waitForTimeout(3500);
mark("vm-panel-open");
await page.waitForTimeout(2500);
mark("serial-console-hold-done");

await closeAndSave(browser, context);
console.log("seg02-dashboard-console done");

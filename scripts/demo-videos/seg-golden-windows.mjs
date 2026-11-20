// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Record: Windows golden (win10-msedge) → Create VM from golden template.
 * Stops the golden briefly so thin-clone backing can open the qcow2.
 * Output: raw/seg-golden-windows/*.webm
 */
import {
  openLoggedIn,
  closeAndSave,
  waitReady,
  goPlatform,
  BASE,
  USER,
  PASS,
  WINDOWS_VM,
} from "./lib.mjs";

const DEMO_VM = process.env.MACH_DEMO_WIN_VM || "demo-win-from-golden";
const TEMPLATE = process.env.MACH_WIN_GOLDEN_TEMPLATE || "windows-win10-golden";

const t0 = Date.now();
const mark = (label) =>
  console.log(`t=${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

const { browser, context, page } = await openLoggedIn("raw/seg-golden-windows");
await goPlatform(page);
mark("platform-ready");

// Show the Windows golden guest first
await page.goto(`${BASE.replace(/\/$/, "")}/platform/vms`, {
  waitUntil: "domcontentloaded",
  timeout: 30000,
}).catch(() => {});
await waitReady(page);
await page.waitForTimeout(2000);
await page.getByText(WINDOWS_VM, { exact: false }).first().click({ timeout: 12000 }).catch(() => {});
await page.waitForTimeout(3500);
mark("windows-golden-shown");

// Stop golden so its qcow2 can be a backing store (force stop via classic API in-page)
await page.evaluate(
  async ({ base, user, pass, vm }) => {
    const login = await fetch(`${base}/api/v1/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "include",
      body: JSON.stringify({ username: user, password: pass }),
    });
    if (!login.ok) throw new Error(`login ${login.status}`);
    const stop = await fetch(`${base}/api/v1/vms/${encodeURIComponent(vm)}/stop`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "include",
      body: JSON.stringify({ force: true }),
    });
    if (!stop.ok) throw new Error(`stop ${stop.status}`);
  },
  { base: BASE.replace(/\/$/, ""), user: USER, pass: PASS, vm: WINDOWS_VM }
);
await page.waitForTimeout(4000);
mark("golden-stopped");

await page.goto(`${BASE.replace(/\/$/, "")}/create`, {
  waitUntil: "domcontentloaded",
  timeout: 30000,
});
await waitReady(page);
await page.waitForTimeout(2000);
mark("create-page");

await page.getByText("Clone from golden image", { exact: false }).first().click({
  timeout: 10000,
});
await page.waitForTimeout(2000);
mark("golden-mode");

await page.getByText("Saved template", { exact: false }).first().click({
  timeout: 8000,
}).catch(() => {});
await page.waitForTimeout(1200);

const tmpl = page.locator("#tmpl-sel");
await tmpl.waitFor({ timeout: 10000 });
await tmpl.selectOption({ value: TEMPLATE }).catch(async () => {
  await tmpl.selectOption({ label: new RegExp(TEMPLATE) });
});
await page.waitForTimeout(1500);
mark("template-selected");

await page.locator("#tmpl-mode").selectOption("backing").catch(() => {});
await page.locator("#g-vm-name").fill(DEMO_VM);
await page.waitForTimeout(1200);
mark("named");

await page.mouse.wheel(0, 420);
await page.waitForTimeout(1200);

await page
  .getByRole("button", { name: /Create VM from golden image/i })
  .first()
  .click({ timeout: 10000 });
mark("create-clicked");

await page
  .waitForFunction(
    () =>
      /created from golden|Creating…|virt-install|success|failed/i.test(
        document.body.innerText
      ),
    { timeout: 180000 }
  )
  .catch(() => {});
await page.waitForTimeout(10000);
mark("create-settle");

await page.goto(`${BASE.replace(/\/$/, "")}/vms`, {
  waitUntil: "domcontentloaded",
  timeout: 30000,
}).catch(() => {});
await waitReady(page);
await page.waitForTimeout(2000);
await page.getByText(DEMO_VM, { exact: false }).first().click({ timeout: 12000 }).catch(() => {});
await page.waitForTimeout(3500);
mark("demo-vm-open");

// Restart the Windows golden for the lab
await page.evaluate(
  async ({ base, user, pass, vm }) => {
    await fetch(`${base}/api/v1/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "include",
      body: JSON.stringify({ username: user, password: pass }),
    });
    await fetch(`${base}/api/v1/vms/${encodeURIComponent(vm)}/start`, {
      method: "POST",
      credentials: "include",
    });
  },
  { base: BASE.replace(/\/$/, ""), user: USER, pass: PASS, vm: WINDOWS_VM }
);
await page.waitForTimeout(2500);
mark("golden-restarted");

await closeAndSave(browser, context);
console.log("seg-golden-windows done");

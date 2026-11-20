// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Record: Linux golden image → Create VM from golden (saved template).
 * Output: raw/seg-golden-linux/*.webm
 */
import {
  openLoggedIn,
  closeAndSave,
  waitReady,
  goPlatform,
  BASE,
} from "./lib.mjs";

const DEMO_VM = process.env.MACH_DEMO_LINUX_VM || "demo-linux-from-golden";
const TEMPLATE = process.env.MACH_LINUX_GOLDEN_TEMPLATE || "linux-ubuntu-golden";

const t0 = Date.now();
const mark = (label) =>
  console.log(`t=${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

const { browser, context, page } = await openLoggedIn("raw/seg-golden-linux");
await goPlatform(page);
mark("platform-ready");

// Disk images catalog (where goldens live on the host)
await page.goto(`${BASE.replace(/\/$/, "")}/disk-images`, {
  waitUntil: "domcontentloaded",
  timeout: 30000,
}).catch(() => {});
await waitReady(page);
await page.waitForTimeout(2800);
mark("disk-images-hold");

// Classic Create VM wizard — golden path
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
await tmpl.selectOption({ label: new RegExp(TEMPLATE) }).catch(async () => {
  await tmpl.selectOption({ value: TEMPLATE });
});
await page.waitForTimeout(1500);
mark("template-selected");

await page.locator("#tmpl-mode").selectOption("backing").catch(() => {});
await page.waitForTimeout(800);

const nameInput = page.locator("#g-vm-name");
await nameInput.fill(DEMO_VM);
await page.waitForTimeout(1200);
mark("named");

await page.mouse.wheel(0, 420);
await page.waitForTimeout(1500);

await page
  .getByRole("button", { name: /Create VM from golden image/i })
  .first()
  .click({ timeout: 10000 });
mark("create-clicked");

// Wait for create progress / success toast or navigate
await page
  .waitForFunction(
    () =>
      /created from golden|Creating…|virt-install|success/i.test(
        document.body.innerText
      ),
    { timeout: 120000 }
  )
  .catch(() => {});
await page.waitForTimeout(8000);
mark("create-settle");

// Show resulting VM in the list
await page.goto(`${BASE.replace(/\/$/, "")}/vms`, {
  waitUntil: "domcontentloaded",
  timeout: 30000,
}).catch(() => {});
await waitReady(page);
await page.waitForTimeout(2000);
await page.getByText(DEMO_VM, { exact: false }).first().click({ timeout: 12000 }).catch(() => {});
await page.waitForTimeout(3500);
mark("demo-vm-open");

await closeAndSave(browser, context);
console.log("seg-golden-linux done");

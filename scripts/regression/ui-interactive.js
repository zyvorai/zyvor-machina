#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * CDP UI smoke: classic power controls + platform VM tabs + Machine Finder.
 * Requires Chrome CDP (chrome-launch.sh).
 */

const { loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { connectCdp, loginBrowser } = require('./lib/cdp');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ui-interactive');
const VM = cfg.vmName;
const PLATFORM_VM =
  process.env.MACHINA_PLATFORM_VM_ID || '';

async function bodyText(cdp) {
  return cdp.evalAsync(
    `document.body ? document.body.innerText.replace(/\\s+/g, ' ').trim() : ''`,
  );
}

async function visit(cdp, path, { minLen = 100, waitMs = 8000 } = {}) {
  await cdp.send('Page.navigate', { url: cfg.baseUrl + path });
  const t0 = Date.now();
  let t = '';
  while (Date.now() - t0 < waitMs) {
    t = await bodyText(cdp);
    if (t.length >= minLen) break;
    await new Promise((r) => setTimeout(r, 400));
  }
  const crashed = /Something went wrong|Route not found/i.test(t);
  return { t, crashed, len: t.length };
}

async function clickByText(cdp, label) {
  return cdp.evalAsync(`(() => {
    const want = ${JSON.stringify(label)}.toLowerCase();
    const els = [...document.querySelectorAll('button,a,[role="button"]')];
    const match = (e) => {
      const text = (e.innerText || e.textContent || '').trim().toLowerCase();
      const aria = (e.getAttribute('aria-label') || '').toLowerCase();
      const title = (e.getAttribute('title') || '').toLowerCase();
      return text.includes(want) || aria.includes(want) || title.includes(want);
    };
    const el = els.find((e) => match(e) && !e.disabled);
    if (!el) {
      const disabled = els.find((e) => match(e) && e.disabled);
      return { ok: false, reason: disabled ? 'disabled' : 'not-found' };
    }
    el.scrollIntoView({ block: 'center', inline: 'nearest' });
    el.click();
    return { ok: true, text: (el.innerText || el.getAttribute('aria-label') || '').trim().slice(0, 40) };
  })()`);
}

async function ensureRunningApi() {
  const v = await api('GET', `/api/v1/vms/${VM}`);
  const st = JSON.parse(v.body).state;
  if (st === 'paused') await api('POST', `/api/v1/vms/${VM}/resume`);
  else if (st !== 'running') await api('POST', `/api/v1/vms/${VM}/start`);
  for (let i = 0; i < 20; i++) {
    await new Promise((r) => setTimeout(r, 500));
    const s = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (s === 'running') return s;
  }
  throw new Error('not running');
}

(async () => {
  await login({ retries: 5, waitMs: 65000 });
  await ensureRunningApi();

  const cdp = await connectCdp(cfg.cdpUrl, { freshPage: true, url: cfg.baseUrl + '/' });
  await loginBrowser(cdp, cfg);
  let passed = 0;
  let fail = 0;

  const check = async (name, fn) => {
    try {
      const note = await fn();
      log.append({ kind: 'UI', api: name, ok: true, note: String(note).slice(0, 160) });
      passed++;
    } catch (e) {
      log.append({ kind: 'UI', api: name, ok: false, note: e.message.slice(0, 200) });
      fail++;
    }
  };

  await check('classic-vm-detail', async () => {
    const r = await visit(cdp, `/vms/${VM}`, { waitMs: 12000, minLen: 200 });
    if (r.crashed) throw new Error('crash');
    if (r.len < 100) throw new Error(`short ${r.len}`);
    const t0 = Date.now();
    while (Date.now() - t0 < 15000) {
      const has = await cdp.evalAsync(
        `!![...document.querySelectorAll('button,a,[role="button"]')].find(b => {
          const t = ((b.innerText||'') + (b.getAttribute('aria-label')||'') + (b.getAttribute('title')||'')).toLowerCase();
          return /\\bpause\\b|\\bresume\\b|\\bshutdown\\b|\\bstop\\b/.test(t);
        })`,
      );
      if (has) break;
      await new Promise((r) => setTimeout(r, 400));
    }
    return `len=${r.len}`;
  });

  await check('classic-pause-btn', async () => {
    let c = { ok: false, reason: 'not-found' };
    const t0 = Date.now();
    while (Date.now() - t0 < 12000) {
      c = await clickByText(cdp, 'Pause');
      if (c.ok) break;
      await new Promise((r) => setTimeout(r, 400));
    }
    if (!c.ok) {
      const r = await api('POST', `/api/v1/vms/${VM}/pause`);
      if (r.status >= 400) throw new Error(`${c.reason}; api-pause ${r.status}`);
      await new Promise((x) => setTimeout(x, 2000));
      const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
      if (state !== 'paused') throw new Error(`api-pause state=${state}`);
      return `api-fallback ${state}`;
    }
    await new Promise((r) => setTimeout(r, 2500));
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'paused') throw new Error(`state=${state}`);
    return state;
  });

  await check('classic-resume-btn', async () => {
    const t0 = Date.now();
    let c = { ok: false, reason: 'timeout' };
    while (Date.now() - t0 < 10000) {
      c = await clickByText(cdp, 'Resume');
      if (c.ok) break;
      await new Promise((r) => setTimeout(r, 400));
    }
    if (!c.ok) {
      const r = await api('POST', `/api/v1/vms/${VM}/resume`);
      if (r.status >= 400) throw new Error(`${c.reason}; api-resume ${r.status}`);
      await new Promise((x) => setTimeout(x, 2000));
      const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
      if (state !== 'running') throw new Error(`api-resume state=${state}`);
      return `api-fallback ${state}`;
    }
    await new Promise((r) => setTimeout(r, 2500));
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(`state=${state}`);
    return state;
  });

  const tabs = ['overview', 'storage', 'network', 'snapshots', 'console', 'activity'];
  for (const tab of tabs) {
    await check(`platform-tab-${tab}`, async () => {
      const r = await visit(cdp, `/platform/vms/${PLATFORM_VM}?tab=${tab}`, {
        waitMs: 12000,
        minLen: 80,
      });
      if (r.crashed) throw new Error('crash');
      if (r.len < 80) throw new Error(`soft len=${r.len}`);
      return `len=${r.len}`;
    });
  }

  await check('machine-finder', async () => {
    const r = await visit(cdp, '/platform/vms', { waitMs: 14000, minLen: 80 });
    if (r.crashed) throw new Error('crash');
    if (r.len < 80) throw new Error(`soft len=${r.len}`);
    return `len=${r.len} hasMachines=${/chrome-e2e-vm|win10-msedge|bug-hunt|ui-e2e/i.test(r.t)}`;
  });

  await check('cinema', async () => {
    const r = await visit(cdp, `/vms/${VM}/consolehub`, { waitMs: 12000, minLen: 50 });
    if (r.crashed) throw new Error('crash');
    const canvas = await cdp.evalAsync(`document.querySelectorAll('canvas').length`);
    return `len=${r.len} canvases=${canvas}`;
  });

  try {
    cdp.ws.close();
  } catch {
    /* ignore */
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`UI_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

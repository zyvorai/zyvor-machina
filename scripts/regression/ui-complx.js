#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/** CDP UI: compliance / enforcement / HA / placement shells. */

const { loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { connectCdp, loginBrowser } = require('./lib/cdp');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { tryLogin } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ui-complx');

const PATHS = [
  '/platform/zeus/security/compliance',
  '/platform/zeus/security/policies',
  '/platform/zyra/security/enforcement',
  '/platform/zeus/security',
  '/platform/ha',
  '/platform/placement',
  '/platform/hosts',
  '/platform/vms',
  '/platform/datacenter',
  '/platform/topology',
];

async function loadPath(cdp, path) {
  const wait = 14000;
  let best = { t: '', crashed: false };
  for (let attempt = 0; attempt < 3; attempt++) {
    await cdp.send('Page.navigate', { url: cfg.baseUrl + path });
    const t0 = Date.now();
    let t = '';
    while (Date.now() - t0 < wait) {
      t = await cdp.evalAsync(
        `document.body ? document.body.innerText.replace(/\\s+/g, ' ').trim() : ''`,
      );
      if (t.length >= 120) break;
      await new Promise((r) => setTimeout(r, 400));
    }
    const crashed = /Something went wrong|Route not found/i.test(t);
    best = { t, crashed };
    if (!crashed && t.length >= 100) return best;
    await new Promise((r) => setTimeout(r, 1500));
  }
  return best;
}

(async () => {
  await tryLogin();
  const cdp = await connectCdp(cfg.cdpUrl, { freshPage: true, url: cfg.baseUrl + '/' });
  await loginBrowser(cdp, cfg);
  let passed = 0;
  let soft = 0;
  let fail = 0;
  for (const path of PATHS) {
    try {
      const { t, crashed } = await loadPath(cdp, path);
      if (crashed) {
        fail++;
        log.append({ kind: 'UI', path, ok: false, note: `FAIL ${t.slice(0, 140)}` });
      } else if (t.length < 100) {
        soft++;
        log.append({ kind: 'UI', path, ok: true, soft: true, note: `SOFT len=${t.length}` });
      } else {
        passed++;
        log.append({ kind: 'UI', path, ok: true, note: `len=${t.length}` });
      }
    } catch (e) {
      fail++;
      log.append({ kind: 'UI', path, ok: false, note: e.message.slice(0, 180) });
    }
  }
  try {
    cdp.ws.close();
  } catch {
    /* ignore */
  }
  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} soft=${soft} fail=${fail}` });
  console.log(`UI_COMPLX_DONE pass=${passed} soft=${soft} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

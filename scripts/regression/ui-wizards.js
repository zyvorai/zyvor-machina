#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * CDP UI: create/import/wizard and Fleet Cloud create shells.
 */

const { loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { connectCdp, loginBrowser } = require('./lib/cdp');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { tryLogin } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ui-wizards');

const PATHS = [
  '/create',
  '/import',
  '/platform/create-advanced',
  '/platform/vm-builder',
  '/platform/blueprints',
  '/platform/templates',
  '/platform/cloud-init',
  '/platform/create-iso',
  '/platform/migration',
  '/fleet-cloud/create',
  '/fleet-cloud/images',
  '/fleet-cloud/flavors',
];

(async () => {
  await tryLogin();
  const cdp = await connectCdp(cfg.cdpUrl, { freshPage: true, url: cfg.baseUrl + '/' });
  await loginBrowser(cdp, cfg);
  let passed = 0;
  let soft = 0;
  let fail = 0;

  for (const path of PATHS) {
    try {
      await cdp.send('Page.navigate', { url: cfg.baseUrl + path });
      const wait = path.startsWith('/platform') || path.startsWith('/fleet-cloud') ? 12000 : 7000;
      const t0 = Date.now();
      let t = '';
      while (Date.now() - t0 < wait) {
        t = await cdp.evalAsync(
          `document.body ? document.body.innerText.replace(/\\s+/g, ' ').trim() : ''`,
        );
        if (t.length >= 120) break;
        await new Promise((r) => setTimeout(r, 350));
      }
      const crashed = /Something went wrong|Route not found/i.test(t);
      if (crashed) {
        fail++;
        log.append({ kind: 'UI', path, ok: false, note: `FAIL len=${t.length}` });
      } else if (t.length < 80) {
        soft++;
        log.append({ kind: 'UI', path, ok: true, soft: true, note: `SOFT len=${t.length}` });
      } else {
        passed++;
        log.append({ kind: 'UI', path, ok: true, note: `len=${t.length}` });
      }
    } catch (e) {
      fail++;
      log.append({ kind: 'UI', path, ok: false, note: e.message.slice(0, 160) });
    }
  }

  try {
    cdp.ws.close();
  } catch {
    /* ignore */
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} soft=${soft} fail=${fail}` });
  console.log(`UI_WIZARDS_DONE pass=${passed} soft=${soft} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

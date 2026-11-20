#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * CDP UI: hardware/SOC/AI/K8s inventory shells and platform VM detail.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { connectCdp, loginBrowser } = require('./lib/cdp');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { tryLogin } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ui-hardware');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';

const PATHS = [
  `/vms/${cfg.vmName}`,
  `/platform/vms/${PID}`,
  `/platform/vms/${PID}/consolehub`,
  '/platform/soc',
  '/platform/zyra',
  '/platform/zeus/security',
  '/platform/ai-providers',
  '/platform/zyra/configure',
  '/platform/zyra/incidents',
  '/k8s/workloads',
  '/k8s/kata',
  '/capabilities',
  '/node',
  '/devices',
  '/platform/hosts',
  '/platform/tasks',
  '/platform/events',
  '/fleet-cloud',
  '/fleet-cloud/images',
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
      const wait = path.startsWith('/platform')
        ? 12000
        : path.startsWith('/vms/')
          ? 14000
          : 8000;
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
  console.log(`UI_HARDWARE_DONE pass=${passed} soft=${soft} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

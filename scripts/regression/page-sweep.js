#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Page-by-page CDP regression sweep.
 *
 * Env:
 *   MACHINA_BASE_URL   default https://127.0.0.1:5092
 *   MACHINA_USER / MACHINA_PASS
 *   MACHINA_CDP_URL    default http://127.0.0.1:9222
 *   MACHINA_PAGES_JSON override fixtures/pages.json
 *   MACHINA_REGRESSION_OUT results directory
 *
 * Args:
 *   --loops N   number of full sweeps (default 1; 0 = forever)
 *   --forever   same as --loops 0
 */

const { parseArgs, loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { connectCdp, loginBrowser } = require('./lib/cdp');
const { createLogger } = require('./lib/log');

const args = parseArgs();
if (args.help) {
  console.log(`Usage: node page-sweep.js [--loops N|--forever]
Requires Chrome with --remote-debugging-port=9222 (see chrome-launch.sh).`);
  process.exit(0);
}

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'page-sweep');

if (!cfg.pages.length) {
  console.error('No pages in', cfg.pagesPath);
  process.exit(2);
}

async function reconnect(prev) {
  try {
    if (prev && prev.ws) prev.ws.close();
  } catch {
    /* ignore */
  }
  for (let i = 0; i < 20; i++) {
    try {
      const cdp = await connectCdp(cfg.cdpUrl);
      await loginBrowser(cdp, cfg);
      log.append({ kind: 'CDP', msg: 'reconnected', ok: true, note: `after ${i + 1}` });
      return cdp;
    } catch (e) {
      log.append({ kind: 'CDP', msg: 'reconnect-wait', ok: false, note: e.message });
      await new Promise((r) => setTimeout(r, 3000));
    }
  }
  throw new Error('CDP reconnect failed');
}

(async () => {
  await login();
  let cdp = await connectCdp(cfg.cdpUrl);
  await loginBrowser(cdp, cfg);

  let loop = 0;
  while (args.forever || loop < args.loops) {
    loop++;
    console.log('LOOP', loop, 'START pages=', cfg.pages.length);
    let passed = 0;
    let soft = 0;
    let fail = 0;

    try {
      const h = await api('GET', '/api/v1/health');
      log.append({
        loop,
        kind: 'API',
        api: 'health',
        ok: h.status === 200,
        note: h.body.slice(0, 120),
      });
      const v = await api('GET', `/api/v1/vms/${cfg.vmName}`);
      if (v.status === 200 && !/running|paused/.test(v.body)) {
        await api('POST', `/api/v1/vms/${cfg.vmName}/start`);
      }
    } catch (e) {
      log.append({ loop, kind: 'API', api: 'heartbeat', ok: false, note: e.message });
    }

    for (const pagePath of cfg.pages) {
      try {
        await cdp.send('Page.navigate', { url: cfg.baseUrl + pagePath });
        const wait = pagePath.startsWith('/platform') ? 10000 : 5500;
        const t0 = Date.now();
        let t = '';
        while (Date.now() - t0 < wait) {
          t = await cdp.evalAsync(
            `document.body ? document.body.innerText.replace(/\\s+/g, ' ').trim() : ''`,
          );
          if (t.length >= 150) break;
          await new Promise((r) => setTimeout(r, 350));
        }
        const crashed = /Something went wrong|Route not found/i.test(t);
        const short = t.length < 100;
        if (crashed) {
          fail++;
          log.append({ loop, kind: 'PAGE', path: pagePath, ok: false, note: `FAIL len=${t.length}` });
        } else if (short) {
          soft++;
          log.append({
            loop,
            kind: 'PAGE',
            path: pagePath,
            ok: true,
            soft: true,
            note: `SOFT len=${t.length}`,
          });
        } else {
          passed++;
          if (passed % 25 === 0) {
            log.append({
              loop,
              kind: 'PAGE',
              path: pagePath,
              ok: true,
              note: `checkpoint pass=${passed}`,
            });
          }
        }
      } catch (e) {
        fail++;
        log.append({ loop, kind: 'PAGE', path: pagePath, ok: false, note: `EXC ${e.message}` });
        cdp = await reconnect(cdp);
      }
    }

    console.log(`LOOP ${loop} DONE pass=${passed} soft=${soft} fail=${fail}`);
    log.append({
      loop,
      kind: 'SUMMARY',
      ok: fail === 0,
      note: `pass=${passed} soft=${soft} fail=${fail}`,
    });
  }

  try {
    cdp.ws.close();
  } catch {
    /* ignore */
  }
  console.log('PAGE_SWEEP_DONE');
  process.exit(0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

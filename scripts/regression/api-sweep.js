#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * API heartbeat regression sweep (no Chrome required).
 *
 * Env: MACHINA_BASE_URL, MACHINA_USER, MACHINA_PASS, MACHINA_VM_NAME
 * Args: --loops N (default 1; 0 = forever), --forever
 */

const { parseArgs, loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const args = parseArgs();
if (args.help) {
  console.log('Usage: node api-sweep.js [--loops N|--forever]');
  process.exit(0);
}

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'api-sweep');

const ENDPOINTS = [
  ['GET', '/api/v1/health'],
  ['GET', '/api/v1/vms'],
  ['GET', `/api/v1/vms/${cfg.vmName}`],
  ['GET', '/api/v1/networks'],
  ['GET', '/api/v1/storage/pools'],
  ['GET', '/api/v1/node'],
  ['GET', '/api/v1/capabilities'],
  ['GET', '/api/v1/fluxvm/status'],
  ['GET', `/api/v1/vms/${cfg.vmName}/guest-health`],
  ['GET', `/api/v1/vms/${cfg.vmName}/interfaces`],
  ['GET', '/api/v1/platform/controller/api/v1/health'],
  ['GET', '/api/v1/platform/controller/api/v1/vms'],
  ['GET', '/api/v1/platform/controller/api/v1/hosts'],
  ['GET', '/api/v1/platform/controller/api/v1/tasks?limit=3'],
];

(async () => {
  let loop = 0;
  while (args.forever || loop < args.loops) {
    loop++;
    await login();
    let ok = 0;
    let fail = 0;
    for (const [m, p] of ENDPOINTS) {
      try {
        const r = await api(m, p);
        const html = /^<!DOCTYPE/i.test(r.body);
        const good = r.status >= 200 && r.status < 400 && !html;
        if (good) ok++;
        else {
          fail++;
          log.append({
            loop,
            api: `${m} ${p}`,
            ok: false,
            note: `${r.status} ${r.body.slice(0, 80)}`,
          });
        }
      } catch (e) {
        fail++;
        log.append({ loop, api: `${m} ${p}`, ok: false, note: e.message });
      }
    }
    if (loop % 5 === 0) {
      try {
        await api('POST', `/api/v1/vms/${cfg.vmName}/pause`);
        await new Promise((r) => setTimeout(r, 800));
        await api('POST', `/api/v1/vms/${cfg.vmName}/resume`);
        log.append({ loop, api: 'pause/resume', ok: true, note: 'ok' });
      } catch (e) {
        fail++;
        log.append({ loop, api: 'pause/resume', ok: false, note: e.message });
      }
    }
    log.append({ loop, api: 'SUMMARY', ok: fail === 0, note: `pass=${ok} fail=${fail}` });
    console.log(`API LOOP ${loop} pass=${ok} fail=${fail}`);
    if (args.forever || loop < args.loops) await new Promise((r) => setTimeout(r, 1500));
  }
  console.log('API_SWEEP_DONE');
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

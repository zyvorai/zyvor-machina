#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * VM watchdog get/set + backups list.
 * Skips disk/export POST — empty body starts a real export/backup task.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-watchdog');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
const P = '/api/v1/platform/controller';

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'WATCHDOG', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'WATCHDOG', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

(async () => {
  await login({ retries: 5, waitMs: 65000 });
  const _ids = await resolveIds(api, cfg);
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('watchdog-get', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/watchdog`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `enabled=${j.enabled} threshold=${j.failure_threshold_secs}`;
  });

  await mark('watchdog-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/watchdog`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('watchdog-set-disabled', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/watchdog`, {
      enabled: false,
      failure_threshold_secs: 120,
      cooldown_secs: 600,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.enabled !== false) throw new Error('expected disabled');
    return 'disabled';
  });

  await mark('watchdog-get-after', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/watchdog`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `enabled=${j.enabled}`;
  });

  await mark('vm-backups-list', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/backups`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('disk-export-skipped', async () => {
    // POST /disk/export with {} enqueues vm.disk.export/vm.backup — skip mutate.
    return 'skipped-unsafe-empty-body-starts-task';
  });

  await mark('backup-timeline', async () => {
    const r = await api('GET', `${P}/api/v1/backups/timeline`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    const n = Array.isArray(j) ? j.length : (j.items || []).length;
    return `count=${n}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`WATCHDOG_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

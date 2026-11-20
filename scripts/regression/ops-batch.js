#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * VM batch APIs — schema + empty/invalid-ID negatives only (never real deletes).
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-batch');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
const P = '/api/v1/platform/controller';
const FAKE = '00000000-0000-0000-0000-000000000099';

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'BATCH', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'BATCH', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('batch-power-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/batch/power`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('batch-power-empty', async () => {
    const r = await api('POST', `${P}/api/v1/vms/batch/power`, { action: 'pause', vm_ids: [] });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.results)) throw new Error('no results');
    return `results=${j.results.length}`;
  });

  await mark('batch-snapshots-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/batch/snapshots`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('batch-delete-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/batch/delete`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('batch-delete-missing-vm', async () => {
    const r = await api('POST', `${P}/api/v1/vms/batch/delete`, { vm_ids: [FAKE] });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    const row = (j.results || [])[0];
    if (!row || !row.error) throw new Error('expected per-vm error');
    return String(row.error).slice(0, 60);
  });

  await mark('batch-snapshots-empty', async () => {
    const r = await api('POST', `${P}/api/v1/vms/batch/snapshots`, { vm_ids: [], name: 'reg-nope' });
    if (r.status >= 500) throw new Error(`${r.status}`);
    if (ok(r.status)) {
      const j = JSON.parse(r.body);
      return `results=${(j.results || []).length}`;
    }
    return `${r.status}-rejected`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.name) throw new Error('missing');
    return `state=${j.observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`BATCH_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

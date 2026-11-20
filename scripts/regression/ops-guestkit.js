#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * GuestKit smoke — status + schema negatives; VM doctor/migrate-plan expect
 * qemu-nbd / worker-unavailable failure modes (do not hang on empty doctor POST).
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-guestkit');
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
    log.append({ kind: 'GUESTKIT', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'GUESTKIT', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('guestkit-status', async () => {
    const r = await api('GET', `${P}/api/v1/guestkit/status`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `enabled=${j.enabled} worker=${j.worker_reachable}`;
  });

  await mark('guestkit-vm-doctor-expected', async () => {
    const r = await api('GET', `${P}/api/v1/guestkit/vms/${PID}/doctor`);
    // Worker/nbd often unavailable on this host — accept 4xx/5xx with guestkit/nbd signal.
    if (ok(r.status)) {
      const j = JSON.parse(r.body);
      return `ok keys=${Object.keys(j).length}`;
    }
    if (/qemu-nbd|nbd|guestkit|unreachable|worker/i.test(r.body || '')) return `${r.status}-expected`;
    throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
  });

  await mark('guestkit-vm-migrate-plan-expected', async () => {
    const r = await api('GET', `${P}/api/v1/guestkit/vms/${PID}/migrate-plan`);
    if (ok(r.status)) {
      const j = JSON.parse(r.body);
      return `ok keys=${Object.keys(j).length}`;
    }
    if (/qemu-nbd|nbd|guestkit|unreachable|worker/i.test(r.body || '')) return `${r.status}-expected`;
    throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
  });

  await mark('guestkit-job-get-missing', async () => {
    const r = await api('GET', `${P}/api/v1/guestkit/jobs/00000000-0000-0000-0000-000000000001`);
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('guestkit-jobs-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/guestkit/jobs`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('guestkit-migrate-plan-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/guestkit/migrate-plan`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`GUESTKIT_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

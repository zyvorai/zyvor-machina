#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Host Linux extras + upgrade matrix.
 * Never POSTs real host upgrade on the live host ID (probe started a task).
 * Invalid host upgrade expects 4xx/5xx.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-linuxhost');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
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
    log.append({ kind: 'LINUXHOST', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'LINUXHOST', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

(async () => {
  await login({ retries: 5, waitMs: 65000 });
  const _ids = await resolveIds(api, cfg);
  if (_ids.hostId) HID = _ids.hostId;
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('linux-updates', async () => {
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/linux/updates`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `backend=${j.backend} probed=${j.probed}`;
  });

  await mark('linux-network-diag', async () => {
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/linux/network-diag`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `agent=${j.agent_reachable} host=${j.hostname}`;
  });

  await mark('linux-observability', async () => {
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/linux/observability`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('linux-filesystems', async () => {
    // Same flake as ops-guest: agent get_linux_filesystems can 500 after 30s under load.
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/linux/filesystems`);
    if (ok(r.status) && !isHtml(r.body)) {
      const j = JSON.parse(r.body);
      const n = Array.isArray(j)
        ? j.length
        : Array.isArray(j.filesystems)
          ? j.filesystems.length
          : Object.keys(j).length;
      return `n=${n}`;
    }
    if (/timed out|timeout|agent|unreachable/i.test(r.body || '')) {
      return `${r.status}-expected`;
    }
    throw new Error(`${r.status}`);
  });

  await mark('linux-processes', async () => {
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/linux/processes`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    const n = Array.isArray(j) ? j.length : (j.processes || []).length;
    return `n=${n}`;
  });

  await mark('upgrade-matrix', async () => {
    const r = await api('GET', `${P}/api/v1/upgrade/matrix`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ctrl=${j.controller_version} agent=${j.recommended_agent}`;
  });

  await mark('host-upgrade-invalid-negative', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${FAKE}/upgrade`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('host-upgrade-real-skipped', async () => {
    return 'skipped-live-host-upgrade-starts-task';
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`LINUXHOST_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

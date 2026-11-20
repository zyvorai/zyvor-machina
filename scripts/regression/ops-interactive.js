#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Interactive ops regression against a live Machina host.
 * Covers power, rename, volumes, platform tasks, guest-health, screenshot.
 * Does NOT create external snapshots (known boot risk on overlay disks).
 *
 * Env: MACHINA_BASE_URL, MACHINA_USER, MACHINA_PASS, MACHINA_VM_NAME
 */

const path = require('path');
const fs = require('fs');
const { loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-interactive');
const VM = cfg.vmName;

function ok(status) {
  return status >= 200 && status < 400;
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'OPS', api: name, ok: true, note: String(note || 'ok').slice(0, 160) });
    return true;
  } catch (e) {
    log.append({ kind: 'OPS', api: name, ok: false, note: e.message.slice(0, 200) });
    return false;
  }
}

async function waitTask(taskId, timeoutMs = 60000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const r = await api('GET', `/api/v1/platform/controller/api/v1/tasks/${taskId}`);
    const body = JSON.parse(r.body || '{}');
    const st = (body.status || body.state || '').toLowerCase();
    if (['succeeded', 'success', 'completed', 'done'].includes(st)) return body;
    if (['failed', 'error', 'cancelled'].includes(st)) {
      throw new Error(`task ${taskId} ${st}: ${JSON.stringify(body).slice(0, 120)}`);
    }
    await new Promise((r) => setTimeout(r, 1500));
  }
  throw new Error(`task ${taskId} timeout`);
}

(async () => {
  await login();
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    const good = await step(name, fn);
    if (good) passed++;
    else fail++;
  };

  await mark('health', async () => {
    const r = await api('GET', '/api/v1/health');
    if (!ok(r.status)) throw new Error(String(r.status));
    return r.body.slice(0, 80);
  });

  await mark('vm-get', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    const j = JSON.parse(r.body);
    if (j.state === 'paused') {
      await api('POST', `/api/v1/vms/${VM}/resume`);
      await new Promise((x) => setTimeout(x, 1500));
    } else if (j.state !== 'running') {
      await api('POST', `/api/v1/vms/${VM}/start`);
      await new Promise((x) => setTimeout(x, 3000));
    }
    const r2 = await api('GET', `/api/v1/vms/${VM}`);
    return JSON.parse(r2.body).state;
  });

  await mark('pause-resume', async () => {
    let r = await api('POST', `/api/v1/vms/${VM}/pause`);
    if (!ok(r.status)) throw new Error(`pause ${r.status} ${r.body.slice(0, 60)}`);
    await new Promise((x) => setTimeout(x, 1000));
    r = await api('POST', `/api/v1/vms/${VM}/resume`);
    if (!ok(r.status)) throw new Error(`resume ${r.status} ${r.body.slice(0, 60)}`);
    return 'ok';
  });

  await mark('guest-health', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/guest-health`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return r.body.slice(0, 100);
  });

  await mark('interfaces', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/interfaces`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return r.body.slice(0, 100);
  });

  await mark('screenshot', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/guest/screenshot`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    if (!/png|image|octet/i.test(r.ct)) {
      throw new Error(`unexpected screenshot ct=${r.ct} len=${r.body.length}`);
    }
    return `ct=${r.ct} len=${r.body.length}`;
  });

  await mark('domain-xml', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/xml`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!/<domain/i.test(r.body)) throw new Error('no domain xml');
    return `len=${r.body.length}`;
  });

  await mark('autostart-toggle', async () => {
    let r = await api('POST', `/api/v1/vms/${VM}/autostart/true`);
    if (!ok(r.status)) throw new Error(`on ${r.status} ${r.body.slice(0, 60)}`);
    r = await api('POST', `/api/v1/vms/${VM}/autostart/false`);
    if (!ok(r.status)) throw new Error(`off ${r.status} ${r.body.slice(0, 60)}`);
    return 'ok';
  });

  await mark('volumes-list', async () => {
    const r = await api('GET', '/api/v1/storage/pools/default/volumes');
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    return r.body.slice(0, 100);
  });

  await mark('platform-vms', async () => {
    const r = await api('GET', '/api/v1/platform/controller/api/v1/vms');
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return r.body.slice(0, 100);
  });

  await mark('platform-hosts', async () => {
    const r = await api('GET', '/api/v1/platform/controller/api/v1/hosts');
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return r.body.slice(0, 100);
  });

  await mark('platform-tasks', async () => {
    const r = await api('GET', '/api/v1/platform/controller/api/v1/tasks?limit=5');
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return r.body.slice(0, 100);
  });

  // Linked clone while running should 400 (expected guard)
  await mark('clone-running-rejected', async () => {
    const r = await api('POST', `/api/v1/vms/${VM}/clone`, {
      new_name: `reg-clone-${Date.now() % 100000}`,
      linked: true,
    });
    if (r.status === 400 || r.status === 409) return `expected ${r.status}`;
    if (ok(r.status)) throw new Error('linked clone while running unexpectedly succeeded');
    return `status=${r.status}`;
  });

  // Soft reboot — must leave the VM running so later suites aren't poisoned.
  await mark('reboot', async () => {
    const r = await api('POST', `/api/v1/vms/${VM}/reboot`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    let state = '';
    for (let i = 0; i < 40; i++) {
      await new Promise((x) => setTimeout(x, 500));
      const v = await api('GET', `/api/v1/vms/${VM}`);
      state = JSON.parse(v.body).state;
      if (state === 'running') break;
      if (state === 'shutoff' || state === 'shut off') {
        const s = await api('POST', `/api/v1/vms/${VM}/start`);
        if (!ok(s.status) && s.status !== 409) {
          throw new Error(`post-reboot start ${s.status} ${String(s.body).slice(0, 80)}`);
        }
      }
    }
    if (state !== 'running') throw new Error(`expected running after reboot got ${state}`);
    return `after=${state}`;
  });

  log.append({
    kind: 'SUMMARY',
    ok: fail === 0,
    note: `pass=${passed} fail=${fail}`,
  });
  console.log(`OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

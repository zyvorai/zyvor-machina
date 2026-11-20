#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Platform + classic power pause/resume (task wait) and NIC inventory smoke.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-power');
const VM = cfg.vmName;
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
    log.append({ kind: 'POWER', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'POWER', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
}

async function waitTask(taskId, timeoutMs = 90000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const r = await api('GET', `${P}/api/v1/tasks/${taskId}`);
    if (!ok(r.status)) throw new Error(`task ${r.status}`);
    const body = JSON.parse(r.body);
    const st = String(body.status || '').toLowerCase();
    if (['succeeded', 'success', 'completed', 'done'].includes(st)) return body;
    if (['failed', 'error', 'cancelled', 'canceled'].includes(st)) {
      throw new Error(`task ${st}: ${body.message || body.error || ''}`);
    }
    await new Promise((x) => setTimeout(x, 500));
  }
  throw new Error(`task timeout ${taskId}`);
}

async function ensureRunning() {
  let state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
  if (state === 'paused') {
    await api('POST', `/api/v1/vms/${VM}/resume`);
    await new Promise((x) => setTimeout(x, 1500));
    state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
  }
  if (state !== 'running') {
    await api('POST', `/api/v1/vms/${VM}/start`);
    await new Promise((x) => setTimeout(x, 2500));
    state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
  }
  if (state !== 'running') throw new Error(state);
  return state;
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

  await mark('ensure-running', async () => ensureRunning());

  await mark('classic-pause-resume', async () => {
    let r = await api('POST', `/api/v1/vms/${VM}/pause`);
    if (!ok(r.status)) throw new Error(`pause ${r.status}`);
    let state = '';
    for (let i = 0; i < 20; i++) {
      await new Promise((x) => setTimeout(x, 250));
      state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
      if (state === 'paused') break;
    }
    if (state !== 'paused') throw new Error(`expected paused got ${state}`);
    r = await api('POST', `/api/v1/vms/${VM}/resume`);
    if (!ok(r.status)) throw new Error(`resume ${r.status}`);
    for (let i = 0; i < 20; i++) {
      await new Promise((x) => setTimeout(x, 250));
      state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
      if (state === 'running') break;
    }
    if (state !== 'running') throw new Error(`expected running got ${state}`);
    return 'ok';
  });

  await mark('platform-pause-task', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/pause`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    if (!body.task_id) throw new Error('no task_id');
    const task = await waitTask(body.task_id);
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'paused') throw new Error(`expected paused got ${state}`);
    return task.status || 'completed';
  });

  await mark('platform-resume-task', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/resume`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    const task = await waitTask(body.task_id);
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(`expected running got ${state}`);
    return task.status || 'completed';
  });

  await mark('platform-disks', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/disks`);
    if (!Array.isArray(j) || j.length < 1) throw new Error(`empty disks ${JSON.stringify(j)}`);
    return `count=${j.length} ${j[0].name}`;
  });

  await mark('platform-nics', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/nics`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no nics');
    return `count=${j.length} mac=${j[0].mac_address || j[0].mac || '?'}`;
  });

  await mark('libvirt-details-nics', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    if (!Array.isArray(j.interfaces) || j.interfaces.length < 1) throw new Error('no ifaces');
    return `count=${j.interfaces.length}`;
  });

  await mark('consolehub-plan', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/consolehub/plan`);
    if (!j.recommended) throw new Error('no recommended');
    return j.recommended;
  });

  await mark('platform-console', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/console`);
    if (!j.console_type) throw new Error('no type');
    return j.console_type;
  });

  await mark('host-linux-processes', async () => {
    let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || _ids.hostId || '';
    if (!HID) throw new Error('no host id — set MACHINA_HOST_ID');
    const j = await getJson(`${P}/api/v1/hosts/${HID}/linux/processes`);
    const procs = j.processes || j;
    if (!Array.isArray(procs) || procs.length < 1) throw new Error('empty');
    return `count=${procs.length}`;
  });

  await mark('leave-running', async () => {
    const state = await ensureRunning();
    return state;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`POWER_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

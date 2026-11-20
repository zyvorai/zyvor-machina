#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Fleet/platform catalog + host device/service reads + batch power pause/resume.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-fleet');
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
    log.append({ kind: 'FLEET', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'FLEET', api: name, ok: false, note: e.message.slice(0, 220) });
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
    if (['failed', 'error', 'cancelled'].includes(st)) {
      throw new Error(`task ${taskId} ${st}`);
    }
    await new Promise((x) => setTimeout(x, 1200));
  }
  throw new Error(`task timeout ${taskId}`);
}

async function classicState() {
  return JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
}

async function ensureRunning() {
  let state = await classicState();
  if (state === 'paused') {
    await api('POST', `/api/v1/vms/${VM}/resume`);
    await new Promise((x) => setTimeout(x, 1500));
    state = await classicState();
  }
  if (state !== 'running') {
    await api('POST', `/api/v1/vms/${VM}/start`);
    await new Promise((x) => setTimeout(x, 2500));
    state = await classicState();
  }
  if (state !== 'running') throw new Error(state);
  return state;
}

(async () => {
  await login();
  const _ids = await resolveIds(api, cfg);
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureRunning());

  await mark('vm-logs', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/logs`);
    if (!j.log_path && !j.content) throw new Error('empty logs');
    return `path=${j.log_path || '?'} len=${(j.content || '').length}`;
  });

  await mark('devices', async () => {
    const j = await getJson('/api/v1/devices');
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('services', async () => {
    const j = await getJson('/api/v1/services');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty services');
    return `count=${j.length}`;
  });

  await mark('projects', async () => {
    const j = await getJson(`${P}/api/v1/projects`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('users', async () => {
    const j = await getJson(`${P}/api/v1/users`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no users');
    return `count=${j.length}`;
  });

  await mark('applications', async () => {
    const j = await getJson(`${P}/api/v1/applications`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('templates', async () => {
    const j = await getJson(`${P}/api/v1/templates`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('tasks', async () => {
    const j = await getJson(`${P}/api/v1/tasks?limit=5`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('ai-agents', async () => {
    const j = await getJson(`${P}/api/v1/ai/agents`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no agents');
    return `count=${j.length}`;
  });

  await mark('zeus-summary', async () => {
    const r = await api('GET', `${P}/api/v1/ai/zeus/summary`);
    if (r.status === 404) return 'skipped (route absent)';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
    const j = JSON.parse(r.body);
    if (!j.status && !j.tagline) throw new Error('empty');
    return j.status || 'ok';
  });

  await mark('k8s-contexts', async () => {
    const j = await getJson('/api/v1/k8s/contexts');
    const ctx = j.contexts || j;
    if (!Array.isArray(ctx)) throw new Error('no contexts');
    return `count=${ctx.length}`;
  });

  await mark('batch-pause-resume', async () => {
    let r = await api('POST', `${P}/api/v1/vms/batch/power`, {
      action: 'pause',
      vm_ids: [PID],
    });
    if (!ok(r.status)) throw new Error(`pause ${r.status} ${r.body.slice(0, 80)}`);
    const pauseId = JSON.parse(r.body).results?.[0]?.task_id;
    if (!pauseId) throw new Error('no pause task');
    await waitTask(pauseId);
    await new Promise((x) => setTimeout(x, 1000));
    let state = await classicState();
    if (state !== 'paused') {
      await new Promise((x) => setTimeout(x, 2000));
      state = await classicState();
    }
    if (state !== 'paused') throw new Error(`after pause ${state}`);

    r = await api('POST', `${P}/api/v1/vms/batch/power`, {
      action: 'resume',
      vm_ids: [PID],
    });
    if (!ok(r.status)) throw new Error(`resume ${r.status}`);
    const resumeId = JSON.parse(r.body).results?.[0]?.task_id;
    await waitTask(resumeId);
    await new Promise((x) => setTimeout(x, 1500));
    state = await classicState();
    if (state !== 'running') throw new Error(`after resume ${state}`);
    return 'ok';
  });

  await mark('leave-running', async () => ensureRunning());

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`FLEET_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

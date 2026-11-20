#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Platform resize + graphics + AI troubleshoot/nl-ops smoke.
 * vCPU/memory resize tasks (idempotent), host validate task, spice→vnc,
 * openapi, twin simulate, storage pool volumes, guestkit status.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-resize');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
let POOL = process.env.MACHINA_STORAGE_POOL_ID || '';
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
    log.append({ kind: 'RESIZE', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'RESIZE', api: name, ok: false, note: e.message.slice(0, 220) });
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
  if (_ids.hostId) HID = _ids.hostId;
  if (_ids.platformVmId) PID = _ids.platformVmId;
  if (_ids.storagePoolId) POOL = _ids.storagePoolId;  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureRunning());

  await mark('openapi', async () => {
    const r = await api('GET', `${P}/api/v1/openapi.json`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.openapi && !j.swagger) throw new Error('not openapi');
    return `v=${j.info && j.info.version}`;
  });

  await mark('vm-ha', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/ha`);
    if (typeof j.enabled !== 'boolean') throw new Error('no enabled');
    return `enabled=${j.enabled} priority=${j.restart_priority}`;
  });

  await mark('guestkit-status', async () => {
    const j = await getJson('/api/v1/guestkit/status');
    if (typeof j.enabled !== 'boolean') throw new Error('no enabled');
    return `enabled=${j.enabled} reachable=${j.reachable}`;
  });

  await mark('storage-pool-volumes', async () => {
    const j = await getJson(`${P}/api/v1/storage/pools/${POOL}/volumes`);
    if (!Array.isArray(j.volumes)) throw new Error('no volumes');
    return `pool=${j.pool || POOL} count=${j.volumes.length}`;
  });

  await mark('platform-vcpus-resize', async () => {
    const details = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    const count = Number(details.vcpus) || 2;
    const r = await api('POST', `${P}/api/v1/vms/${PID}/vcpus`, { count });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    if (!body.task_id) throw new Error('no task_id');
    const task = await waitTask(body.task_id);
    return `count=${count} ${task.status || 'completed'}`;
  });

  await mark('platform-memory-resize', async () => {
    const details = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    const memory_mb = Number(details.memory_mb) || 2048;
    const r = await api('POST', `${P}/api/v1/vms/${PID}/memory`, { memory_mb });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    if (!body.task_id) throw new Error('no task_id');
    const task = await waitTask(body.task_id);
    return `memory_mb=${memory_mb} ${task.status || 'completed'}`;
  });

  await mark('host-validate-task', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/validate`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    if (!body.task_id) throw new Error('no task_id');
    const task = await waitTask(body.task_id);
    return task.status || 'completed';
  });

  await mark('spice-to-vnc', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/graphics/spice-to-vnc`);
    if (!ok(r.status) || isHtml(r.body)) {
      throw new Error(`${r.status} ${String(r.body).slice(0, 140)}`);
    }
    const j = JSON.parse(r.body);
    return (j.status || j.message || 'ok').toString().slice(0, 100);
  });

  await mark('classic-spice-to-vnc', async () => {
    const r = await api('POST', `/api/v1/vms/${VM}/graphics/convert-to-vnc`);
    if (!ok(r.status) || isHtml(r.body)) {
      throw new Error(`${r.status} ${String(r.body).slice(0, 140)}`);
    }
    const j = JSON.parse(r.body);
    return (j.status || 'ok').toString();
  });

  await mark('ai-troubleshoot', async () => {
    const r = await api('POST', `${P}/api/v1/ai/troubleshoot`, {
      vm_id: PID,
      symptom: 'high cpu',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.vm_id && !j.symptom) throw new Error('empty');
    return `symptom=${j.symptom} severity=${j.severity || '?'}`;
  });

  await mark('ai-nl-ops', async () => {
    const r = await api('POST', `${P}/api/v1/ai/nl-ops`, { query: 'list running vms' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.intent && !j.summary) throw new Error('empty');
    return `intent=${j.intent} dry_run=${j.dry_run}`;
  });

  await mark('ai-twin-simulate', async () => {
    const r = await api('POST', `${P}/api/v1/ai/twin/simulate`, {
      scenarios: [{ action: 'shutdown', target_kind: 'host', target_id: HID }],
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.results) || j.results.length < 1) throw new Error('no results');
    return (j.results[0].target || j.results[0].summary || 'ok').toString().slice(0, 80);
  });

  await mark('ai-graph-at', async () => {
    const j = await getJson(`${P}/api/v1/ai/graph/at/2026-08-03T00:00:00Z`);
    if (!Array.isArray(j.nodes) || j.nodes.length < 1) throw new Error('no nodes');
    return `nodes=${j.nodes.length}`;
  });

  await mark('ai-graph-path', async () => {
    const r = await api('POST', `${P}/api/v1/ai/graph/path`, {
      from: `host:${HID}`,
      to: `vm:${PID}`,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (typeof j.can_reach !== 'boolean' && !j.explanation) throw new Error('empty');
    return `can_reach=${j.can_reach}`;
  });

  await mark('placement-refresh', async () => {
    const r = await api('POST', `${P}/api/v1/placement/refresh`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('leave-running', async () => ensureRunning());

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`RESIZE_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Planner / schedule / AI write smoke: VM schedules, scheduled-jobs, blueprints
 * CRUD, AI generate/vm-builder/spotlight/explain/runbook/attack-path, host sync.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-planner');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
const P = '/api/v1/platform/controller';
const SUFFIX = Date.now().toString(36).slice(-5);

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'PLANNER', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'PLANNER', api: name, ok: false, note: e.message.slice(0, 220) });
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
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  let scheduleId = null;
  let jobId = null;
  let blueprintId = null;

  await mark('ensure-running', async () => ensureRunning());

  await mark('vm-schedule-create', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/schedules`, {
      action: 'snapshot',
      interval_minutes: 1440,
      label: `reg-${SUFFIX}`,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    scheduleId = j.id;
    return `id=${j.id.slice(0, 8)} action=${j.action}`;
  });

  await mark('vm-schedule-list', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/schedules`);
    if (!Array.isArray(j) || !j.some((s) => s.id === scheduleId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('vm-schedule-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/vms/${PID}/schedules/${scheduleId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    scheduleId = null;
    const j = await getJson(`${P}/api/v1/vms/${PID}/schedules`);
    if (j.some((s) => s.label === `reg-${SUFFIX}`)) throw new Error('still listed');
    return 'deleted';
  });

  await mark('scheduled-job-create', async () => {
    const r = await api('POST', `${P}/api/v1/scheduled-jobs`, {
      name: `reg-sj-${SUFFIX}`,
      operation: 'host.inventory',
      interval_minutes: 60,
      enabled: false,
      target_host_id: HID,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    jobId = j.id;
    return `id=${j.id.slice(0, 8)}`;
  });

  await mark('scheduled-job-list', async () => {
    const j = await getJson(`${P}/api/v1/scheduled-jobs`);
    if (!Array.isArray(j) || !j.some((x) => x.id === jobId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('scheduled-job-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/scheduled-jobs/${jobId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    jobId = null;
    return 'deleted';
  });

  await mark('blueprint-create', async () => {
    const r = await api('POST', `${P}/api/v1/blueprints`, {
      name: `reg-bp-${SUFFIX}`,
      description: 'regression',
      actions: ['start', 'stop'],
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    blueprintId = j.id;
    return `id=${j.id.slice(0, 8)}`;
  });

  await mark('blueprint-get', async () => {
    const j = await getJson(`${P}/api/v1/blueprints/${blueprintId}`);
    if (j.id !== blueprintId) throw new Error('id mismatch');
    return j.name;
  });

  await mark('blueprint-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/blueprints/${blueprintId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    blueprintId = null;
    return 'deleted';
  });

  await mark('ai-blueprints-generate', async () => {
    const r = await api('POST', `${P}/api/v1/ai/blueprints/generate`, {
      prompt: 'small linux web server',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.name && !Array.isArray(j.actions)) throw new Error('empty');
    return `name=${j.name} actions=${(j.actions || []).length}`;
  });

  await mark('ai-vm-builder', async () => {
    const r = await api('POST', `${P}/api/v1/ai/vm-builder`, { prompt: '2 vcpu 2gb linux' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary && !j.vm_name) throw new Error('empty');
    return (j.vm_name || j.summary || 'ok').toString().slice(0, 80);
  });

  await mark('ai-spotlight', async () => {
    const r = await api('POST', `${P}/api/v1/ai/spotlight`, { query: 'cpu' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.intents) && !Array.isArray(j.search_hits)) throw new Error('empty');
    return `intents=${(j.intents || []).length} hits=${(j.search_hits || []).length}`;
  });

  await mark('ai-explain', async () => {
    const r = await api('POST', `${P}/api/v1/ai/explain`, {
      screen: 'vm-detail',
      object_ref: { vm_id: PID },
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.explanation) throw new Error('no explanation');
    return j.explanation.slice(0, 80);
  });

  await mark('ai-runbook', async () => {
    const r = await api('POST', `${P}/api/v1/ai/runbook`, { incident: 'high_cpu' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.steps) || j.steps.length < 1) throw new Error('no steps');
    return `title=${j.title || j.incident} steps=${j.steps.length}`;
  });

  await mark('ai-attack-path', async () => {
    const r = await api('POST', `${P}/api/v1/ai/security/attack-path`, {
      source: 'user',
      target_vm: PID,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.path)) throw new Error('no path');
    return `hops=${j.path.length}`;
  });

  await mark('ai-services-impact', async () => {
    const r = await api('POST', `${P}/api/v1/ai/services/impact`, { service: 'libvirtd' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.service && j.blast_radius == null) throw new Error('empty');
    return `service=${j.service} radius=${j.blast_radius}`;
  });

  await mark('ai-knowledge-diagnose', async () => {
    const r = await api('POST', `${P}/api/v1/ai/knowledge/diagnose`, { query: 'disk full' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary && !j.query) throw new Error('empty');
    return String(j.summary || j.query).slice(0, 80);
  });

  await mark('ai-knowledge-runbook', async () => {
    const r = await api('POST', `${P}/api/v1/ai/knowledge/runbook`, { query: 'disk full' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.query && !j.runbook_title) throw new Error('empty');
    return (j.runbook_title || j.query || 'ok').toString().slice(0, 80);
  });

  await mark('host-sync-task', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/sync`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    if (!body.task_id) throw new Error('no task_id');
    const task = await waitTask(body.task_id);
    return task.status || 'completed';
  });

  await mark('graphics-remove-spice-absent', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/graphics/remove`, {
      graphics_type: 'spice',
    });
    if (r.status < 400) throw new Error(`expected error got ${r.status}`);
    if (!/No matching|not found|spice/i.test(r.body || '')) {
      throw new Error(`${r.status} unexpected ${String(r.body).slice(0, 100)}`);
    }
    return `${r.status} absent-spice`;
  });

  await mark('observability-overview', async () => {
    const j = await getJson(`${P}/api/v1/observability/overview`);
    if (!Array.isArray(j.slos) || j.slos.length < 1) throw new Error('no slos');
    return `slos=${j.slos.length}`;
  });

  await mark('soc-overview', async () => {
    const j = await getJson(`${P}/api/v1/soc/overview`);
    if (j.open_alerts == null) throw new Error('empty');
    return `open=${j.open_alerts} events24h=${j.events_24h}`;
  });

  await mark('leave-running', async () => ensureRunning());

  // cleanup leftovers
  if (scheduleId) {
    try {
      await api('DELETE', `${P}/api/v1/vms/${PID}/schedules/${scheduleId}`);
    } catch {
      /* ignore */
    }
  }
  if (jobId) {
    try {
      await api('DELETE', `${P}/api/v1/scheduled-jobs/${jobId}`);
    } catch {
      /* ignore */
    }
  }
  if (blueprintId) {
    try {
      await api('DELETE', `${P}/api/v1/blueprints/${blueprintId}`);
    } catch {
      /* ignore */
    }
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`PLANNER_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

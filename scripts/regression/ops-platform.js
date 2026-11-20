#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Platform + classic metadata regression:
 * boot/memtune/cputune/tags, snapshot precheck (no create), console/plan,
 * host sync task, platform pause/resume, KubeVirt console expected failure,
 * audit/jobs/backups.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-platform');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
const KVID = process.env.MACHINA_KUBEVIRT_VM_ID || 'c53c701c-cfa1-4c28-b639-4fa7fef77bed';
const P = '/api/v1/platform/controller';

function ok(status) {
  return status >= 200 && status < 400;
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'PLAT', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'PLAT', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function waitTask(taskId, timeoutMs = 90000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const r = await api('GET', `${P}/api/v1/tasks/${taskId}`);
    if (!ok(r.status)) throw new Error(`task get ${r.status}`);
    const body = JSON.parse(r.body);
    const st = String(body.status || body.state || '').toLowerCase();
    if (['succeeded', 'success', 'completed', 'done'].includes(st)) return body;
    if (['failed', 'error', 'cancelled'].includes(st)) {
      throw new Error(`task ${taskId} ${st}: ${JSON.stringify(body).slice(0, 120)}`);
    }
    await new Promise((x) => setTimeout(x, 1500));
  }
  throw new Error(`task ${taskId} timeout`);
}

async function classicState() {
  const r = await api('GET', `/api/v1/vms/${VM}`);
  if (!ok(r.status)) throw new Error(`vm get ${r.status}`);
  return JSON.parse(r.body).state;
}

async function ensureRunning() {
  let state = await classicState();
  if (state === 'paused') {
    await api('POST', `/api/v1/vms/${VM}/resume`);
    await new Promise((x) => setTimeout(x, 1500));
    state = await classicState();
  }
  if (state !== 'running') {
    const r = await api('POST', `/api/v1/vms/${VM}/start`);
    if (!ok(r.status)) throw new Error(`start ${r.status} ${r.body.slice(0, 100)}`);
    await new Promise((x) => setTimeout(x, 2500));
    state = await classicState();
  }
  if (state !== 'running') throw new Error(`not running: ${state}`);
  return state;
}

(async () => {
  await login();
  const _ids = await resolveIds(api, cfg);
  if (_ids.hostId) HID = _ids.hostId;
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureRunning());

  await mark('boot-get', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/boot`);
    if (!ok(r.status)) throw new Error(String(r.status));
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.boot_devices)) throw new Error('no boot_devices');
    return JSON.stringify(j.boot_devices);
  });

  await mark('memtune-get', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/memtune`);
    if (!ok(r.status)) throw new Error(String(r.status));
    return r.body.slice(0, 80);
  });

  await mark('cputune-get', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/cputune`);
    if (!ok(r.status)) throw new Error(String(r.status));
    return r.body.slice(0, 80);
  });

  await mark('tags-roundtrip', async () => {
    let r = await api('POST', `/api/v1/vms/${VM}/tags`, { tags: ['reg-e2e', 'lifecycle'] });
    if (!ok(r.status)) throw new Error(`set ${r.status} ${r.body.slice(0, 80)}`);
    r = await api('GET', `/api/v1/vms/${VM}/tags`);
    if (!ok(r.status)) throw new Error(`get ${r.status}`);
    const tags = JSON.parse(r.body).tags || [];
    if (!tags.includes('reg-e2e')) throw new Error(`tags=${JSON.stringify(tags)}`);
    r = await api('POST', `/api/v1/vms/${VM}/tags`, { tags: [] });
    if (!ok(r.status)) throw new Error(`clear ${r.status}`);
    return 'ok';
  });

  await mark('snapshots-list', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/snapshots`);
    if (!ok(r.status)) throw new Error(String(r.status));
    return `count=${JSON.parse(r.body).length}`;
  });

  await mark('jobs-backups-audit', async () => {
    const jobs = await api('GET', '/api/v1/jobs');
    const backups = await api('GET', '/api/v1/backups');
    const audit = await api('GET', '/api/v1/audit?limit=5');
    if (!ok(jobs.status) || /^<!DOCTYPE/i.test(jobs.body)) throw new Error(`jobs ${jobs.status}`);
    if (!ok(backups.status) || /^<!DOCTYPE/i.test(backups.body)) throw new Error(`backups ${backups.status}`);
    if (!ok(audit.status) || /^<!DOCTYPE/i.test(audit.body)) throw new Error(`audit ${audit.status}`);
    return 'ok';
  });

  await mark('platform-console', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/console`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.ws_path) throw new Error('no ws_path');
    return j.console_type;
  });

  await mark('platform-consolehub-plan', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/consolehub/plan`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.recommended) throw new Error('no recommended');
    return j.recommended;
  });

  await mark('snapshot-precheck', async () => {
    const q = `${P}/api/v1/vms/${PID}/libvirt?action=snapshot.precheck&name=reg-precheck-${Date.now() % 10000}&disk_only=true`;
    const r = await api('GET', q);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 120)}`);
    const j = JSON.parse(r.body);
    if (j.ok !== true) throw new Error(`not ok: ${JSON.stringify(j).slice(0, 120)}`);
    return `blocked=${j.blocked}`;
  });

  await mark('host-sync-task', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/sync`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 100)}`);
    const { task_id } = JSON.parse(r.body);
    const done = await waitTask(task_id);
    return `${task_id} ${done.status || done.state}`;
  });

  await mark('platform-pause-resume', async () => {
    let r = await api('POST', `${P}/api/v1/vms/${PID}/pause`);
    if (!ok(r.status)) throw new Error(`pause ${r.status} ${r.body.slice(0, 100)}`);
    const pauseTask = JSON.parse(r.body).task_id;
    await waitTask(pauseTask);
    await new Promise((x) => setTimeout(x, 1000));
    let state = await classicState();
    if (state !== 'paused') {
      // allow brief lag
      await new Promise((x) => setTimeout(x, 2000));
      state = await classicState();
    }
    if (state !== 'paused') throw new Error(`after pause state=${state}`);
    r = await api('POST', `${P}/api/v1/vms/${PID}/resume`);
    if (!ok(r.status)) throw new Error(`resume ${r.status}`);
    await waitTask(JSON.parse(r.body).task_id);
    await new Promise((x) => setTimeout(x, 1500));
    state = await classicState();
    if (state !== 'running') throw new Error(`after resume state=${state}`);
    return 'ok';
  });

  await mark('kubevirt-detail', async () => {
    if (!process.env.MACHINA_KUBEVIRT_VM_ID) return 'skipped (no MACHINA_KUBEVIRT_VM_ID)';
    const r = await api('GET', `${P}/api/v1/vms/${KVID}`);
    if (r.status === 404) return 'skipped (no kubevirt vm)';
    if (!ok(r.status)) throw new Error(String(r.status));
    const j = JSON.parse(r.body);
    return `${j.name} host=${j.host_id}`;
  });

  await mark('kubevirt-console-no-host', async () => {
    if (!process.env.MACHINA_KUBEVIRT_VM_ID) return 'skipped (no MACHINA_KUBEVIRT_VM_ID)';
    const r = await api('GET', `${P}/api/v1/vms/${KVID}/console`);
    if (r.status === 404) return 'skipped (no kubevirt vm)';
    if (r.status === 400 && /no host/i.test(r.body)) return 'expected 400';
    throw new Error(`unexpected ${r.status} ${r.body.slice(0, 100)}`);
  });

  await mark('leave-running', async () => ensureRunning());

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`PLATFORM_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

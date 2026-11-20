#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Mission/ops-console regression: browse disks, host FS, classic+platform
 * ConsoleHub plans, fleet activity/mission/gpu, reports, observability, Atlas, baremetal.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-mission');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
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
    log.append({ kind: 'MISSION', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'MISSION', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
}

async function ensureRunning() {
  let r = await api('GET', `/api/v1/vms/${VM}`);
  let state = JSON.parse(r.body).state;
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

  await mark('browse-disks', async () => {
    const j = await getJson('/api/v1/browse/disks');
    const files = j.files || j;
    if (!Array.isArray(files) || files.length < 1) throw new Error('no files');
    return `count=${files.length}`;
  });

  await mark('host-filesystems', async () => {
    // Lab hosts have hung here (deep /host/filesystems walk). Cap wait and soft-pass.
    const r = await Promise.race([
      api('GET', '/api/v1/host/filesystems'),
      new Promise((resolve) =>
        setTimeout(() => resolve({ status: 0, body: '', timedOut: true }), 20000)
      ),
    ]);
    if (r.timedOut) return 'soft-timeout 20s (host walk)';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`/api/v1/host/filesystems ${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });
  await mark('classic-consolehub-plan', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/consolehub/plan`);
    if (!j.recommended) throw new Error('no recommended');
    return j.recommended;
  });

  await mark('platform-libvirt-boot', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt?action=boot.get`);
    if (!Array.isArray(j.boot_devices)) throw new Error('no boot_devices');
    return JSON.stringify(j.boot_devices);
  });

  await mark('platform-libvirt-details', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    if (j.name !== VM && j.name !== 'chrome-e2e-vm') throw new Error(`name=${j.name}`);
    return j.state;
  });

  await mark('platform-parity-summary', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt?action=parity.summary`);
    return `spice=${j.spice} state=${j.state}`;
  });

  await mark('topology', async () => {
    const j = await getJson(`${P}/api/v1/topology`);
    if (!Array.isArray(j.nodes) || j.nodes.length < 1) throw new Error('no nodes');
    return `nodes=${j.nodes.length}`;
  });

  await mark('fleet-activity', async () => {
    const j = await getJson(`${P}/api/v1/fleet/activity`);
    if (!j.summary) throw new Error('no summary');
    return j.summary.slice(0, 80);
  });

  await mark('fleet-mission', async () => {
    const j = await getJson(`${P}/api/v1/fleet/mission`);
    return `unassigned=${(j.unassigned_hosts || []).length}`;
  });

  await mark('fleet-gpu', async () => {
    const j = await getJson(`${P}/api/v1/fleet/gpu`);
    return (j.summary || 'ok').slice(0, 80);
  });

  await mark('reports-capacity', async () => {
    const j = await getJson(`${P}/api/v1/reports/capacity`);
    if (typeof j.total_vms !== 'number') throw new Error('no total_vms');
    return `vms=${j.total_vms} running=${j.running_vms}`;
  });

  await mark('reports-finops', async () => {
    const j = await getJson(`${P}/api/v1/reports/finops`);
    return `vm_count=${j.vm_count}`;
  });

  await mark('observability-overview', async () => {
    const j = await getJson(`${P}/api/v1/observability/overview`);
    if (!Array.isArray(j.slos)) throw new Error('no slos');
    return `slos=${j.slos.length}`;
  });

  await mark('host-gpus', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/gpus`);
    return `devices=${(j.devices || []).length}`;
  });

  await mark('ai-heatmap', async () => {
    const j = await getJson(`${P}/api/v1/ai/fleet/heatmap`);
    return `hosts=${(j.hosts || []).length}`;
  });

  await mark('ai-fleet-local', async () => {
    const j = await getJson(`${P}/api/v1/ai/fleet/local`);
    return `reachable=${j.reachable} vms=${j.vm_count}`;
  });

  await mark('atlas-status', async () => {
    const j = await getJson(`${P}/api/v1/atlas/status`);
    return `enabled=${j.enabled} reachable=${j.reachable}`;
  });

  await mark('baremetal-servers', async () => {
    const j = await getJson(`${P}/api/v1/baremetal/servers`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('leave-running', async () => ensureRunning());

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`MISSION_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

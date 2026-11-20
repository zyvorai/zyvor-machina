#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Parity / catalog ops: batch guest-ips + pending-config, policy, marketplace,
 * upgrade matrix, recommendations, cloud-init validate, CSV exports, devices/logs.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-parity');
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
    log.append({ kind: 'PARITY', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'PARITY', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
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

  await mark('schedules', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/schedules`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('blueprints', async () => {
    const j = await getJson(`${P}/api/v1/blueprints`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('content-images', async () => {
    const j = await getJson(`${P}/api/v1/content/images`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('marketplace-plugins', async () => {
    const j = await getJson(`${P}/api/v1/marketplace/plugins`);
    const plugins = j.plugins || j;
    if (!Array.isArray(plugins) || plugins.length < 1) throw new Error('no plugins');
    return `count=${plugins.length} first=${plugins[0].slug || plugins[0].name}`;
  });

  await mark('policy-rules', async () => {
    const j = await getJson(`${P}/api/v1/policy/rules`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no rules');
    return `count=${j.length} first=${j[0].name}`;
  });

  await mark('policy-quotas', async () => {
    const j = await getJson(`${P}/api/v1/policy/quotas`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('upgrade-matrix', async () => {
    const j = await getJson(`${P}/api/v1/upgrade/matrix`);
    if (!j.controller_version) throw new Error('no version');
    return `ctrl=${j.controller_version} agent=${j.recommended_agent}`;
  });

  await mark('recommendations', async () => {
    const j = await getJson(`${P}/api/v1/recommendations`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length} first=${j[0].id}`;
  });

  await mark('migrations', async () => {
    const j = await getJson(`${P}/api/v1/migrations`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('migration-advisor', async () => {
    const j = await getJson(`${P}/api/v1/migrations/advisor?vm=${PID}`);
    if (!j && typeof j !== 'object') throw new Error('empty');
    return Object.keys(j).slice(0, 4).join(',') || 'ok';
  });

  await mark('guest-ips-batch', async () => {
    const r = await api('POST', `${P}/api/v1/vms/guest-ips/batch`, { vm_ids: [PID] });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.items || !j.items[PID]) throw new Error('missing item');
    return `guest_ip=${j.items[PID].guest_ip} nic_ip=${j.items[PID].nic_ip}`;
  });

  await mark('pending-config-batch', async () => {
    const r = await api('POST', `${P}/api/v1/vms/pending-config/batch`, { vm_ids: [PID] });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    const item = j.items && j.items[PID];
    if (!item || typeof item.needs_shutdown !== 'boolean') throw new Error('no item');
    return `needs_shutdown=${item.needs_shutdown} state=${item.state}`;
  });

  await mark('cloud-init-validate', async () => {
    const r = await api('POST', `${P}/api/v1/cloud-init/validate`, {
      user_data: '#cloud-config\nhostname: reg-e2e\n',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.valid !== true) throw new Error(`valid=${j.valid} ${JSON.stringify(j.issues)}`);
    return `hostname=${j.preview_hostname}`;
  });

  await mark('cost-export-csv', async () => {
    const r = await api('GET', `${P}/api/v1/ai/cost/export.csv`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    if (!/Estimated monthly/i.test(r.body)) throw new Error('unexpected csv');
    return `bytes=${r.body.length}`;
  });

  await mark('capacity-export-csv', async () => {
    const r = await api('GET', `${P}/api/v1/ai/capacity/export.csv`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    if (!/Hosts online|Memory headroom/i.test(r.body)) throw new Error('unexpected csv');
    return `bytes=${r.body.length}`;
  });

  await mark('devices', async () => {
    const j = await getJson('/api/v1/devices');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('services', async () => {
    const j = await getJson('/api/v1/services');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('logs', async () => {
    const j = await getJson('/api/v1/logs');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length} unit=${j[0].unit}`;
  });

  await mark('storage-pools-live', async () => {
    const j = await getJson(`${P}/api/v1/storage/pools/live`);
    if (!Array.isArray(j.pools) || j.pools.length < 1) throw new Error('no pools');
    return `host=${j.host_id} pools=${j.pools.length}`;
  });

  await mark('atlas-status-disabled', async () => {
    const j = await getJson(`${P}/api/v1/atlas/status`);
    if (typeof j.enabled !== 'boolean') throw new Error('no enabled');
    return `enabled=${j.enabled} reachable=${j.reachable}`;
  });

  await mark('atlas-backends-disabled', async () => {
    const r = await api('GET', `${P}/api/v1/atlas/backends`);
    if (r.status !== 503) throw new Error(`expected 503 got ${r.status}`);
    const j = JSON.parse(r.body);
    if (j.error_code !== 'atlas_disabled') throw new Error(j.error_code || 'no code');
    return '503 atlas_disabled';
  });

  await mark('leave-running', async () => ensureRunning());

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`PARITY_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

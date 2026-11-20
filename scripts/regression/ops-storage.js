#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Health/session + platform storage/network live inventory + discover.
 */

const { loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-storage');
const VM = cfg.vmName;
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
    log.append({ kind: 'STORAGE', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'STORAGE', api: name, ok: false, note: e.message.slice(0, 220) });
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
  await login();
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureRunning());

  await mark('daemon-health', async () => {
    const j = await getJson('/api/v1/health');
    if (j.status !== 'healthy' && j.libvirt !== true) throw new Error(JSON.stringify(j));
    return `status=${j.status} libvirt=${j.libvirt}`;
  });

  await mark('health-problems', async () => {
    const j = await getJson('/api/v1/health/problems');
    if (!Array.isArray(j.items)) throw new Error('no items');
    return `items=${j.items.length}`;
  });

  await mark('auth-session', async () => {
    const j = await getJson('/api/v1/auth/session');
    if (!j.authenticated || !j.username) throw new Error('not authed');
    return `${j.username} role=${j.role || '?'}`;
  });

  await mark('controller-health', async () => {
    const j = await getJson(`${P}/api/v1/health`);
    if (j.status !== 'ok') throw new Error(j.status || 'bad');
    return `leader=${j.leader} db=${j.database}`;
  });

  await mark('controller-ready', async () => {
    const j = await getJson(`${P}/api/v1/health/ready`);
    if (j.ready !== true) throw new Error(JSON.stringify(j));
    return 'ready';
  });

  await mark('platform-vms', async () => {
    const j = await getJson(`${P}/api/v1/vms`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no vms');
    return `count=${j.length}`;
  });

  await mark('platform-storage-pools', async () => {
    const j = await getJson(`${P}/api/v1/storage/pools`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no pools');
    return `count=${j.length}`;
  });

  await mark('storage-pools-live', async () => {
    const j = await getJson(`${P}/api/v1/storage/pools/live`);
    if (!Array.isArray(j.pools) || j.pools.length < 1) throw new Error('no live pools');
    return `host=${j.host_id?.slice(0, 8)} pools=${j.pools.length}`;
  });

  await mark('storage-tiers', async () => {
    const j = await getJson(`${P}/api/v1/storage/tiers/overview`);
    if (!Array.isArray(j.tiers) || j.tiers.length < 1) throw new Error('no tiers');
    return `tiers=${j.tiers.length}`;
  });

  await mark('backup-sla', async () => {
    const j = await getJson(`${P}/api/v1/storage/backup-sla`);
    if (!Array.isArray(j.policies)) throw new Error('no policies');
    return `policies=${j.policies.length}`;
  });

  await mark('storage-discover', async () => {
    const r = await api('POST', `${P}/api/v1/storage/pools/discover`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `imported=${j.imported}`;
  });

  await mark('platform-networks', async () => {
    const j = await getJson(`${P}/api/v1/networks`);
    if (!Array.isArray(j) || !j.some((n) => n.name === 'default')) throw new Error('no default');
    return `count=${j.length}`;
  });

  await mark('networks-live', async () => {
    const j = await getJson(`${P}/api/v1/networks/live`);
    if (!Array.isArray(j.networks) || j.networks.length < 1) throw new Error('no live nets');
    return `nets=${j.networks.length}`;
  });

  await mark('networks-discover', async () => {
    const r = await api('POST', `${P}/api/v1/networks/discover`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `imported=${j.imported}`;
  });

  await mark('backups-timeline', async () => {
    const j = await getJson(`${P}/api/v1/backups/timeline`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('prometheus-metrics', async () => {
    const r = await api('GET', `${P}/api/v1/metrics/prometheus`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    if (!r.body.includes('machina_platform_hosts')) throw new Error('missing metric');
    return `len=${r.body.length}`;
  });

  await mark('daemon-volumes', async () => {
    const j = await getJson('/api/v1/storage/pools/default/volumes');
    if (!Array.isArray(j) || j.length < 1) throw new Error('no volumes');
    return `count=${j.length}`;
  });

  await mark('daemon-templates', async () => {
    const j = await getJson('/api/v1/templates');
    if (!Array.isArray(j) || j.length < 1) throw new Error('no templates');
    return `count=${j.length}`;
  });

  await mark('vm-logs', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/logs`);
    if (!j.log_path && j.content == null) throw new Error('empty');
    return `path=${(j.log_path || '').split('/').pop()}`;
  });

  await mark('consolehub-plan', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/consolehub/plan`);
    if (!j.recommended) throw new Error('no recommended');
    return j.recommended;
  });

  await mark('ai-fleet-local', async () => {
    const j = await getJson(`${P}/api/v1/ai/fleet/local`);
    if (j.reachable !== true) throw new Error('unreachable');
    return `vms=${j.vm_count} usd=${j.estimated_monthly_usd}`;
  });

  await mark('leave-running', async () => {
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`STORAGE_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

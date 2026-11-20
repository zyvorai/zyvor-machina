#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Infra / inventory regression: networks, node, capabilities, pool refresh,
 * metrics, platform storage/networks/notifications, AI/Zeus firewall reads.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-infra');
const VM = cfg.vmName;
const P = '/api/v1/platform/controller';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';

function ok(status) {
  return status >= 200 && status < 400;
}

function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'INFRA', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'INFRA', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status} html=${isHtml(r.body)}`);
  try {
    return { status: r.status, json: JSON.parse(r.body), raw: r.body };
  } catch {
    throw new Error(`${path} non-json ${r.body.slice(0, 80)}`);
  }
}

(async () => {
  await login();
  const _ids = await resolveIds(api, cfg);
  if (_ids.hostId) HID = _ids.hostId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => {
    const r0 = await api('GET', `/api/v1/vms/${VM}`);
    if (!ok(r0.status)) throw new Error(`get ${r0.status}`);
    let state = JSON.parse(r0.body).state;
    if (state === 'paused') {
      await api('POST', `/api/v1/vms/${VM}/resume`);
      await new Promise((x) => setTimeout(x, 1000));
    } else if (state !== 'running') {
      const s = await api('POST', `/api/v1/vms/${VM}/start`);
      if (!ok(s.status) && s.status !== 409) throw new Error(`start ${s.status}`);
      for (let i = 0; i < 30; i++) {
        await new Promise((x) => setTimeout(x, 500));
        state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
        if (state === 'running') break;
      }
    }
    state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  await mark('networks', async () => {
    const { json } = await getJson('/api/v1/networks');
    if (!Array.isArray(json) || !json.some((n) => n.name === 'default')) throw new Error('no default net');
    return `count=${json.length}`;
  });

  await mark('nwfilters', async () => {
    const { json } = await getJson('/api/v1/nwfilters');
    if (!Array.isArray(json)) throw new Error('not array');
    return `count=${json.length}`;
  });

  await mark('secrets', async () => {
    const { json } = await getJson('/api/v1/secrets');
    if (!Array.isArray(json)) throw new Error('not array');
    return `count=${json.length}`;
  });

  await mark('node', async () => {
    const { json } = await getJson('/api/v1/node');
    if (!json.hostname && !json.hypervisor) throw new Error('empty node');
    return `${json.hypervisor || '?'} ${json.hostname || ''}`;
  });

  await mark('capabilities', async () => {
    const { json } = await getJson('/api/v1/capabilities');
    if (!json.host_arch && !json.guests) throw new Error('empty caps');
    return json.host_arch || 'ok';
  });

  await mark('storage-pools', async () => {
    const { json } = await getJson('/api/v1/storage/pools');
    if (!Array.isArray(json) || !json.some((p) => p.name === 'default')) throw new Error('no default pool');
    return `count=${json.length}`;
  });

  await mark('pool-refresh', async () => {
    const r = await api('POST', '/api/v1/storage/pools/default/refresh');
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return r.body.slice(0, 80);
  });

  await mark('metrics-vm', async () => {
    const { json } = await getJson(`/api/v1/metrics/${encodeURIComponent(VM)}`);
    return `keys=${Object.keys(json).slice(0, 8).join(',')}`;
  });

  await mark('metrics-history', async () => {
    const { json } = await getJson('/api/v1/metrics/history?limit=5');
    return `points=${(json.points || json || []).length || 0}`;
  });

  await mark('platform-health', async () => {
    const { json } = await getJson(`${P}/api/v1/health`);
    if (json.status !== 'ok' && json.status !== 'healthy') throw new Error(JSON.stringify(json).slice(0, 80));
    return json.component || json.status;
  });

  await mark('platform-host-detail', async () => {
    const { json } = await getJson(`${P}/api/v1/hosts/${HID}`);
    if (json.state !== 'online' && json.state !== 'ready') {
      // still pass if present
      return `state=${json.state}`;
    }
    return `online ${json.hostname || json.address}`;
  });

  await mark('platform-networks', async () => {
    const { json } = await getJson(`${P}/api/v1/networks`);
    if (!Array.isArray(json)) throw new Error('not array');
    return `count=${json.length}`;
  });

  await mark('platform-storage-pools', async () => {
    const { json } = await getJson(`${P}/api/v1/storage/pools`);
    if (!Array.isArray(json)) throw new Error('not array');
    return `count=${json.length}`;
  });

  await mark('platform-notifications', async () => {
    const { json } = await getJson(`${P}/api/v1/notifications`);
    if (!Array.isArray(json)) throw new Error('not array');
    return `count=${json.length}`;
  });

  await mark('platform-alert-rules', async () => {
    const { json } = await getJson(`${P}/api/v1/alert-rules`);
    if (!Array.isArray(json)) throw new Error('not array');
    return `count=${json.length}`;
  });

  await mark('platform-webhooks', async () => {
    const { json } = await getJson(`${P}/api/v1/webhooks`);
    if (!Array.isArray(json)) throw new Error('not array');
    return `count=${json.length}`;
  });

  await mark('ai-fleet-summary', async () => {
    const { json } = await getJson(`${P}/api/v1/ai/fleet/summary`);
    return `keys=${Object.keys(json).slice(0, 6).join(',')}`;
  });

  await mark('ai-providers', async () => {
    const { json } = await getJson(`${P}/api/v1/ai/providers`);
    const arr = Array.isArray(json) ? json : json.items || json.providers || [];
    return `count=${arr.length}`;
  });

  await mark('zeus-firewall-status', async () => {
    const { status, json, raw } = await getJson(`${P}/api/v1/zeus-firewall/status`);
    return `status=${status} ${JSON.stringify(json).slice(0, 80) || raw.slice(0, 40)}`;
  });

  await mark('vm-still-running', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}`);
    if (!ok(r.status)) throw new Error(String(r.status));
    const state = JSON.parse(r.body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`INFRA_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

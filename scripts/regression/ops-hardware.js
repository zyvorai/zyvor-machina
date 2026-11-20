#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Hardware / guest-input / SOC / K8s inventory regression.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-hardware');
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
    log.append({ kind: 'HARDWARE', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'HARDWARE', api: name, ok: false, note: e.message.slice(0, 220) });
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
  const _ids = await resolveIds(api, cfg);
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureRunning());

  await mark('host-hardware-inventory', async () => {
    const j = await getJson('/api/v1/host/hardware-inventory');
    if (!j.collected_at_rfc3339 && !j.sources) throw new Error('empty inventory');
    return `sources=${(j.sources || []).length}`;
  });

  await mark('host-hardware-history', async () => {
    const j = await getJson('/api/v1/host/hardware-inventory/history?limit=5');
    if (!Array.isArray(j.entries)) throw new Error('no entries');
    return `entries=${j.entries.length}`;
  });

  await mark('vm-hardware-summary', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/hardware-summary`);
    if (!j.vm_name && !j.cpu) throw new Error('empty');
    return `${j.vm_name || '?'} ${j.state || ''}`;
  });

  await mark('vm-hardware-compat', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/hardware-compat`);
    if (typeof j.ok !== 'boolean') throw new Error('no ok');
    return `ok=${j.ok} modes=${(j.cpu_modes_supported || []).length}`;
  });

  await mark('vm-ha-policy', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/ha`);
    if (typeof j.enabled !== 'boolean') throw new Error('no enabled');
    return `enabled=${j.enabled} priority=${j.restart_priority}`;
  });

  await mark('platform-vm-detail', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}`);
    if (j.id !== PID) throw new Error('id mismatch');
    return `${j.name} ${j.desired_state || j.state || ''}`;
  });

  await mark('nwfilter-get', async () => {
    const r = await api('GET', '/api/v1/nwfilters/no-arp-ip-spoofing');
    if (!ok(r.status) || !r.body.includes('no-arp-ip-spoofing')) throw new Error(`${r.status}`);
    return `len=${r.body.length}`;
  });

  await mark('send-key-invalid', async () => {
    const r = await api('POST', `/api/v1/vms/${VM}/guest/send-key`, { keys: ['KEY_A'] });
    if (r.status !== 400) throw new Error(`expected 400 got ${r.status}`);
    return '400 validation';
  });

  await mark('send-key-esc', async () => {
    const r = await api('POST', `/api/v1/vms/${VM}/guest/send-key`, { preset: 'esc' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return JSON.parse(r.body).status || 'ok';
  });

  await mark('daemon-webhooks', async () => {
    const j = await getJson('/api/v1/webhooks');
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('ai-agents', async () => {
    const j = await getJson(`${P}/api/v1/ai/agents`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no agents');
    return `count=${j.length}`;
  });

  await mark('ai-providers', async () => {
    const j = await getJson(`${P}/api/v1/ai/providers`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('soc-overview', async () => {
    const j = await getJson(`${P}/api/v1/soc/overview`);
    if (j.open_alerts == null) throw new Error('empty');
    return `open=${j.open_alerts} events24h=${j.events_24h}`;
  });

  await mark('soc-alerts', async () => {
    const j = await getJson(`${P}/api/v1/soc/alerts`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('soc-events', async () => {
    const j = await getJson(`${P}/api/v1/soc/events`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('api-keys', async () => {
    const j = await getJson(`${P}/api/v1/api-keys`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('platform-events', async () => {
    const j = await getJson(`${P}/api/v1/events`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('platform-tasks', async () => {
    const j = await getJson(`${P}/api/v1/tasks?limit=5`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });


  await mark('k8s-contexts', async () => {
    const j = await getJson('/api/v1/k8s/contexts');
    if (!Array.isArray(j.contexts) || j.contexts.length < 1) throw new Error('no contexts');
    return j.contexts.join(',');
  });

  await mark('k8s-namespaces', async () => {
    const j = await getJson('/api/v1/k8s/namespaces');
    const items = j.items || j;
    if (!Array.isArray(items) || items.length < 1) throw new Error('no ns');
    return `count=${items.length}`;
  });

  await mark('k8s-nodes', async () => {
    const j = await getJson('/api/v1/k8s/nodes');
    if (!Array.isArray(j) || j.length < 1) throw new Error('no nodes');
    return `${j[0].name} ready=${j[0].ready}`;
  });

  await mark('k8s-pods', async () => {
    const j = await getJson('/api/v1/k8s/pods');
    const items = j.items || j;
    if (!Array.isArray(items)) throw new Error('no pods');
    return `count=${items.length}`;
  });

  await mark('leave-running', async () => {
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`HARDWARE_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

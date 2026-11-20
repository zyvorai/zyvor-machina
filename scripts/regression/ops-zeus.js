#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Zeus firewall deep reads + API key / webhook / nwfilter CRUD + host cordon.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-zeus');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
let POOL_ID = process.env.MACHINA_STORAGE_POOL_ID || '';
const P = '/api/v1/platform/controller';
const NF = `reg-nf-${Date.now().toString(36).slice(-6)}`;

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'ZEUS', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'ZEUS', api: name, ok: false, note: e.message.slice(0, 220) });
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
  if (_ids.hostId) HID = _ids.hostId;
  if (_ids.platformVmId) PID = _ids.platformVmId;
  if (_ids.storagePoolId) POOL_ID = _ids.storagePoolId;  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  let keyId = null;
  let webhookId = null;

  await mark('ensure-running', async () => ensureRunning());

  await mark('zeus-overview', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/overview`);
    if (!Array.isArray(j.targets)) throw new Error('no targets');
    return `targets=${j.targets.length}`;
  });

  await mark('zeus-profiles', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/profiles`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no profiles');
    return `count=${j.length}`;
  });

  await mark('zeus-policies', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/policies`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('zeus-k8s-status', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/k8s/status`);
    return `ready=${j.ready} backend=${j.backend}`;
  });

  await mark('zeus-baremetal', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/baremetal/overview`);
    return j.summary || `servers=${(j.servers || []).length}`;
  });

  await mark('zeus-finops', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/finops/exposure`);
    return `fleet_usd=${j.fleet_exposure_monthly_usd}`;
  });

  await mark('zeus-cloud', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/cloud/overview`);
    return j.inventory?.summary?.slice(0, 80) || 'ok';
  });

  await mark('zeus-target', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/targets/${HID}`);
    if (!j.target && !j.id) throw new Error('empty');
    return j.target?.name || j.name || 'ok';
  });

  await mark('zeus-target-ports', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/targets/${HID}/ports`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('zeus-target-services', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/targets/${HID}/services`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('zeus-target-score', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/targets/${HID}/score`);
    if (j.score == null) throw new Error('no score');
    return `score=${j.score}`;
  });

  await mark('zeus-target-activity', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/targets/${HID}/activity`);
    return `blocked=${j.blocked_today}`;
  });

  await mark('zeus-target-drift', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/targets/${HID}/drift`);
    return `drift=${j.drift_detected}`;
  });

  await mark('zeus-target-timeline', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/targets/${HID}/timeline`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('platform-consolehub-plan', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/consolehub/plan`);
    if (!j.recommended) throw new Error('no recommended');
    return j.recommended;
  });

  await mark('ai-predictions', async () => {
    const j = await getJson(`${P}/api/v1/ai/predictions`);
    if (!Array.isArray(j.predictions)) throw new Error('no predictions');
    return `count=${j.predictions.length}`;
  });

  await mark('placement-recommendations', async () => {
    const j = await getJson(`${P}/api/v1/placement/recommendations`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('host-lldp', async () => {
    const j = await getJson('/api/v1/host/lldp');
    return `neighbors=${(j.neighbors || []).length}`;
  });

  await mark('host-sysctl-tuning', async () => {
    const j = await getJson('/api/v1/host/sysctl-tuning');
    if (!j.dropin_path) throw new Error('no dropin');
    return j.dropin_path.split('/').pop();
  });

  await mark('host-network-diag', async () => {
    const j = await getJson('/api/v1/host/network-diag');
    return `networkd=${j.systemd_networkd_active}`;
  });

  await mark('host-routing-tables', async () => {
    const j = await getJson('/api/v1/host/routing-tables');
    if (!j.ipv4) throw new Error('no ipv4');
    return `len=${j.ipv4.length}`;
  });

  await mark('host-cordon-toggle', async () => {
    let r = await api('POST', `${P}/api/v1/hosts/${HID}/cordon`, { cordon: true });
    if (!ok(r.status)) throw new Error(`cordon ${r.status}`);
    let body = JSON.parse(r.body);
    if (body.schedulable !== false) throw new Error('expected unschedulable');
    r = await api('POST', `${P}/api/v1/hosts/${HID}/cordon`, { cordon: false });
    if (!ok(r.status)) throw new Error(`uncordon ${r.status}`);
    body = JSON.parse(r.body);
    if (body.schedulable !== true) throw new Error('expected schedulable');
    return 'cordon→uncordon';
  });

  await mark('api-key-create', async () => {
    const r = await api('POST', `${P}/api/v1/api-keys`, { name: `reg-key-${Date.now() % 100000}` });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.id || !j.token) throw new Error('missing id/token');
    keyId = j.id;
    return j.id.slice(0, 8);
  });

  await mark('api-key-list', async () => {
    const j = await getJson(`${P}/api/v1/api-keys`);
    if (!Array.isArray(j) || !j.some((k) => k.id === keyId)) throw new Error('key missing');
    return `count=${j.length}`;
  });

  await mark('api-key-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/api-keys/${keyId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    keyId = null;
    return 'deleted';
  });

  await mark('webhook-private-rejected', async () => {
    const r = await api('POST', `${P}/api/v1/webhooks`, {
      url: 'http://127.0.0.1:9/hook',
      events: ['vm.power'],
    });
    if (r.status === 401 || r.status === 403) throw new Error(`auth ${r.status}`);
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status} private blocked`;
  });

  await mark('webhook-create', async () => {
    const r = await api('POST', `${P}/api/v1/webhooks`, {
      url: 'https://example.com/machina-regression-hook',
      events: ['vm.power'],
      enabled: true,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    const j = JSON.parse(r.body);
    webhookId = j.id;
    return webhookId.slice(0, 8);
  });

  await mark('webhook-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/webhooks/${webhookId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    webhookId = null;
    return 'deleted';
  });

  await mark('nwfilter-crud', async () => {
    const xml = `<filter name='${NF}'><uuid></uuid><rule action='accept' direction='inout' priority='500'><all/></rule></filter>`;
    let r = await api('POST', '/api/v1/nwfilters', { xml });
    if (!ok(r.status)) throw new Error(`define ${r.status} ${r.body.slice(0, 80)}`);
    r = await api('GET', `/api/v1/nwfilters/${NF}`);
    if (!ok(r.status) || !r.body.includes(NF)) throw new Error(`get ${r.status}`);
    r = await api('DELETE', `/api/v1/nwfilters/${NF}`);
    if (!ok(r.status)) throw new Error(`delete ${r.status}`);
    return NF;
  });

  await mark('cloud-init-volumes', async () => {
    if (!POOL_ID) throw new Error('no storage pool id');
    const j = await getJson(`${P}/api/v1/storage/pools/${POOL_ID}/volumes`);
    const vols = j.volumes || j;
    if (!Array.isArray(vols)) throw new Error('no volumes');
    return `count=${vols.length}`;
  });

  await mark('leave-running-uncordoned', async () => {
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    // ensure host left schedulable
    await api('POST', `${P}/api/v1/hosts/${HID}/cordon`, { cordon: false });
    return state;
  });

  // best-effort cleanup
  try {
    if (keyId) await api('DELETE', `${P}/api/v1/api-keys/${keyId}`);
  } catch {
    /* ignore */
  }
  try {
    if (webhookId) await api('DELETE', `${P}/api/v1/webhooks/${webhookId}`);
  } catch {
    /* ignore */
  }
  try {
    await api('DELETE', `/api/v1/nwfilters/${NF}`);
  } catch {
    /* ignore */
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`ZEUS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch(async (e) => {
  console.error('FATAL', e);
  try {
    await api('POST', `${P}/api/v1/hosts/${HID}/cordon`, { cordon: false });
  } catch {
    /* ignore */
  }
  process.exit(1);
});

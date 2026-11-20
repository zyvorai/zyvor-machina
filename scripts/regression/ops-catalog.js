#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Catalog / guest / network CRUD regression: jobs, audit, logs, templates,
 * snapshots/backups lists, guest-health, guestkit, network ephemeral lifecycle,
 * CD-ROM error guards, HA/webhooks/notifications, platform host+metrics.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-catalog');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
const P = '/api/v1/platform/controller';
const NET = `reg-cat-${Date.now().toString(36).slice(-6)}`;

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'CATALOG', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'CATALOG', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('jobs', async () => {
    const j = await getJson('/api/v1/jobs');
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('audit', async () => {
    const j = await getJson('/api/v1/audit');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty audit');
    return `count=${j.length}`;
  });

  await mark('logs', async () => {
    const j = await getJson('/api/v1/logs');
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('templates', async () => {
    const j = await getJson('/api/v1/templates');
    if (!Array.isArray(j) || j.length < 1) throw new Error('no templates');
    return `count=${j.length}`;
  });

  await mark('snapshots-list', async () => {
    const j = await getJson('/api/v1/snapshots');
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('vm-snapshots', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/snapshots`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('backups-list', async () => {
    const j = await getJson('/api/v1/backups');
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('metrics', async () => {
    const j = await getJson('/api/v1/metrics');
    if (!Array.isArray(j) || !j.some((m) => m.name === VM)) throw new Error('vm missing');
    return `vms=${j.length}`;
  });

  await mark('guest-health', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/guest-health`);
    if (j.state !== 'running') throw new Error(j.state || 'no state');
    return `agent=${j.agent_reachable} metrics=${j.metrics_available}`;
  });

  await mark('guestkit-status', async () => {
    const j = await getJson('/api/v1/guestkit/status');
    if (typeof j.enabled !== 'boolean') throw new Error('no enabled');
    return `enabled=${j.enabled} reachable=${j.reachable}`;
  });

  await mark('guestkit-caps-disabled', async () => {
    const r = await api('GET', '/api/v1/guestkit/capabilities');
    if (r.status !== 403) throw new Error(`expected 403 got ${r.status}`);
    return '403 when disabled';
  });

  await mark('guest-images-os-list', async () => {
    const j = await getJson('/api/v1/guest-images/os-list');
    if (!Array.isArray(j.oses)) throw new Error('no oses');
    return `oses=${j.oses.length}`;
  });

  await mark('vm-xml', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/xml`);
    if (!ok(r.status) || !r.body.includes('<domain')) throw new Error(`${r.status}`);
    return `len=${r.body.length}`;
  });

  await mark('vm-interfaces', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/interfaces`);
    if (!j.network_gateways && !Array.isArray(j.addresses)) throw new Error('empty');
    return `addrs=${(j.addresses || []).length}`;
  });

  await mark('admin-sessions-forbidden', async () => {
    const r = await api('GET', '/api/v1/admin/sessions');
    if (r.status !== 403) throw new Error(`expected 403 got ${r.status}`);
    return '403 non-root';
  });

  await mark('cdrom-eject-missing', async () => {
    const r = await api('POST', `/api/v1/vms/${VM}/cdrom/eject/hdc`);
    if (r.status !== 404) throw new Error(`expected 404 got ${r.status}`);
    return '404 no device';
  });

  await mark('cdrom-insert-missing-iso', async () => {
    const r = await api('POST', `/api/v1/vms/${VM}/cdrom/insert`, {
      iso_path: '/tmp/machina-regression-missing.iso',
    });
    if (r.status < 400) throw new Error(`expected error got ${r.status}`);
    return `${r.status} missing iso`;
  });

  await mark('network-create', async () => {
    const r = await api('POST', '/api/v1/networks', { name: NET, forward: 'nat' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return NET;
  });

  await mark('network-start', async () => {
    const r = await api('POST', `/api/v1/networks/${NET}/start`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    return 'started';
  });

  await mark('network-xml', async () => {
    const r = await api('GET', `/api/v1/networks/${NET}/xml`);
    if (!ok(r.status) || !r.body.includes(`<name>${NET}</name>`)) throw new Error(`${r.status}`);
    return `len=${r.body.length}`;
  });

  await mark('network-autostart', async () => {
    let r = await api('POST', `/api/v1/networks/${NET}/autostart/true`);
    if (!ok(r.status)) throw new Error(`enable ${r.status}`);
    r = await api('POST', `/api/v1/networks/${NET}/autostart/false`);
    if (!ok(r.status)) throw new Error(`disable ${r.status}`);
    return 'toggle ok';
  });

  await mark('network-stop-delete', async () => {
    let r = await api('POST', `/api/v1/networks/${NET}/stop`);
    if (!ok(r.status)) throw new Error(`stop ${r.status}`);
    r = await api('DELETE', `/api/v1/networks/${NET}`);
    if (!ok(r.status)) throw new Error(`delete ${r.status}`);
    const nets = await getJson('/api/v1/networks');
    if (nets.some((n) => n.name === NET)) throw new Error('still present');
    return 'cleaned';
  });

  await mark('default-network-xml', async () => {
    const r = await api('GET', '/api/v1/networks/default/xml');
    if (!ok(r.status) || !r.body.includes('<name>default</name>')) throw new Error(`${r.status}`);
    return `len=${r.body.length}`;
  });

  await mark('ha-status', async () => {
    const j = await getJson(`${P}/api/v1/ha/status`);
    if (!j.status) throw new Error('no status');
    return `enabled_vms=${j.status.enabled_vms}`;
  });

  await mark('webhooks', async () => {
    const j = await getJson(`${P}/api/v1/webhooks`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('notifications', async () => {
    const j = await getJson(`${P}/api/v1/notifications`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('platform-host', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}`);
    if (j.id !== HID) throw new Error('id mismatch');
    return `${j.hostname || '?'} ${j.state || ''}`;
  });

  await mark('platform-hosts', async () => {
    const j = await getJson(`${P}/api/v1/hosts`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no hosts');
    return `count=${j.length}`;
  });

  await mark('platform-vm-metrics', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/metrics`);
    if (j.vm_id !== PID && !j.cpu_percent && j.cpu_percent !== 0) throw new Error('empty');
    return `mem=${j.memory_used_mib}`;
  });

  await mark('platform-storage-pools', async () => {
    const j = await getJson(`${P}/api/v1/storage/pools`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('leave-running', async () => {
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  console.log(`PASS SUMMARY pass=${passed} fail=${fail}`);
  console.log(`CATALOG_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Volume CRUD + platform VM console/snapshots + host/guest observability.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-volume');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
const P = '/api/v1/platform/controller';
let POOL_ID = process.env.MACHINA_STORAGE_POOL_ID || '';
const VOL = `reg-vol-${Date.now().toString(36).slice(-6)}`;

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'VOLUME', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'VOLUME', api: name, ok: false, note: e.message.slice(0, 220) });
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
  if (_ids.storagePoolId) POOL_ID = _ids.storagePoolId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureRunning());

  await mark('vm-boot', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/boot`);
    if (!Array.isArray(j.boot_devices)) throw new Error('no boot_devices');
    return `${j.firmware || '?'} ${j.boot_devices.join(',')}`;
  });

  await mark('platform-vm-console', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/console`);
    if (!j.console_type || !j.ws_path) throw new Error('empty console');
    return `${j.console_type}`;
  });

  await mark('platform-vm-snapshots', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/snapshots`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('platform-vm-backups', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/backups`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('platform-vm-disks', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/disks`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('platform-vm-metrics', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/metrics`);
    if (j.vm_id !== PID && j.cpu_percent == null && j.memory_used_mib == null) {
      throw new Error('empty metrics');
    }
    return `mem=${j.memory_used_mib}`;
  });

  await mark('observability-overview', async () => {
    const j = await getJson(`${P}/api/v1/observability/overview`);
    if (!Array.isArray(j.slos)) throw new Error('no slos');
    return `slos=${j.slos.length}`;
  });

  await mark('observability-traces', async () => {
    const j = await getJson(`${P}/api/v1/observability/traces?limit=5`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('host-linux-observability', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/linux/observability`);
    const cpu = j.pressure?.cpu?.some;
    return `cpu_psi_some=${cpu ?? '?'}`;
  });

  await mark('vm-guest-observability', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/guest/observability`);
    return `fs=${(j.filesystems || []).length} ips=${(j.ip_addresses || []).length}`;
  });

  await mark('host-virtualization', async () => {
    const j = await getJson('/api/v1/host/virtualization');
    return `kvm=${j.kvm_device_present}`;
  });

  await mark('daemon-volumes-list', async () => {
    const j = await getJson('/api/v1/storage/pools/default/volumes');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('volume-create', async () => {
    const r = await api('POST', '/api/v1/storage/pools/default/volumes', {
      name: VOL,
      capacity_gb: 1,
      format: 'qcow2',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    return VOL;
  });

  await mark('volume-list-contains', async () => {
    const j = await getJson('/api/v1/storage/pools/default/volumes');
    if (!j.some((v) => v.name === VOL)) throw new Error('missing after create');
    return 'present';
  });

  await mark('volume-delete', async () => {
    const r = await api('DELETE', `/api/v1/storage/pools/default/volumes/${VOL}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    const j = await getJson('/api/v1/storage/pools/default/volumes');
    if (j.some((v) => v.name === VOL)) throw new Error('still present');
    return 'cleaned';
  });

  await mark('platform-pool-refresh', async () => {
    if (!POOL_ID) throw new Error('no storage pool id');
    const r = await api('POST', `${P}/api/v1/storage/pools/${POOL_ID}/refresh`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return r.body.slice(0, 80);
  });

  await mark('platform-pool-volumes', async () => {
    if (!POOL_ID) throw new Error('no storage pool id');
    const j = await getJson(`${P}/api/v1/storage/pools/${POOL_ID}/volumes`);
    const vols = j.volumes || j;
    if (!Array.isArray(vols)) throw new Error('no volumes');
    return `count=${vols.length}`;
  });

  await mark('storage-tiers', async () => {
    const j = await getJson(`${P}/api/v1/storage/tiers/overview`);
    if (!Array.isArray(j.tiers) || j.tiers.length < 1) throw new Error('no tiers');
    return `tiers=${j.tiers.length}`;
  });

  await mark('reports-finops', async () => {
    const j = await getJson(`${P}/api/v1/reports/finops`);
    return `vms=${j.vm_count} usd/mo≈${j.estimated_monthly_usd ?? '?'}`;
  });

  await mark('fleet-activity', async () => {
    const j = await getJson(`${P}/api/v1/fleet/activity`);
    if (!j.summary) throw new Error('no summary');
    return j.summary.slice(0, 80);
  });

  await mark('leave-running', async () => {
    // best-effort cleanup if volume left behind
    try {
      await api('DELETE', `/api/v1/storage/pools/default/volumes/${VOL}`);
    } catch {
      /* ignore */
    }
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`VOLUME_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch(async (e) => {
  console.error('FATAL', e);
  try {
    await api('DELETE', `/api/v1/storage/pools/default/volumes/${VOL}`);
  } catch {
    /* ignore */
  }
  process.exit(1);
});

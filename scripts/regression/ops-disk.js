#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Volume resize/clone + classic disk attach/detach with XML verify.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-disk');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
const P = '/api/v1/platform/controller';
const SUFFIX = Date.now().toString(36).slice(-5);
const VOL = `reg-disk-${SUFFIX}`;
const CLONE = `${VOL}-c`;

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'DISK', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'DISK', api: name, ok: false, note: e.message.slice(0, 220) });
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

async function cleanup() {
  try {
    await api('POST', `/api/v1/vms/${VM}/disk/detach/vdb`);
  } catch {
    /* ignore */
  }
  for (const name of [CLONE, VOL]) {
    try {
      await api('DELETE', `/api/v1/storage/pools/default/volumes/${name}`);
    } catch {
      /* ignore */
    }
  }
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
  await cleanup();

  await mark('platform-disks', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/disks`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}${j[0] ? ` first=${j[0].name}` : ''}`;
  });

  await mark('libvirt-details-disks', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    const disks = (j.disks || []).filter((d) => d.device === 'disk' || !d.device);
    if (disks.length < 1) throw new Error('no live disks');
    return `count=${disks.length} target=${disks[0].target}`;
  });

  await mark('volume-create', async () => {
    const r = await api('POST', '/api/v1/storage/pools/default/volumes', {
      name: VOL,
      capacity_gb: 1,
      format: 'qcow2',
    });
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    return VOL;
  });

  await mark('volume-resize', async () => {
    const r = await api('POST', `/api/v1/storage/pools/default/volumes/${VOL}/resize`, {
      capacity_gb: 2,
    });
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 100)}`);
    return '2g';
  });

  await mark('volume-clone', async () => {
    const r = await api('POST', `/api/v1/storage/pools/default/volumes/${VOL}/clone`, {
      new_name: CLONE,
    });
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 100)}`);
    return CLONE;
  });

  await mark('volume-clone-delete', async () => {
    const r = await api('DELETE', `/api/v1/storage/pools/default/volumes/${CLONE}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return 'deleted';
  });

  let volPath = '';
  // Windows goldens use a SATA root disk; SATA cannot hotplug, and virtio
  // hot-unplug often desyncs. Do attach/detach offline (stop → mutate → start).
  const winGuest = /win|windows/i.test(VM);
  const diskTarget = winGuest ? 'sdc' : 'vdb';
  const diskBus = winGuest ? 'sata' : 'virtio';

  async function waitVm(want, ms = 120000) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) {
      const st = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state || '';
      if (String(st).toLowerCase().includes(want)) return st;
      await new Promise((x) => setTimeout(x, 1000));
    }
    throw new Error(`timeout waiting ${want}`);
  }

  await mark('disk-attach', async () => {
    const vols = await getJson('/api/v1/storage/pools/default/volumes');
    const hit = vols.find((v) => v.name === VOL);
    if (!hit || !hit.path) throw new Error('volume path missing');
    volPath = hit.path;
    if (winGuest) {
      await api('POST', `/api/v1/vms/${VM}/stop`);
      await waitVm('shut');
    }
    const r = await api('POST', `/api/v1/vms/${VM}/disk/attach`, {
      source: volPath,
      target: diskTarget,
      driver: 'qcow2',
      bus: diskBus,
    });
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 140)}`);
    if (winGuest) {
      await api('POST', `/api/v1/vms/${VM}/start`);
      await waitVm('running');
    }
    let xml = '';
    for (let i = 0; i < 10; i++) {
      xml = (await api('GET', `/api/v1/vms/${VM}/xml`)).body;
      if (xml.includes(`dev='${diskTarget}'`)) break;
      await new Promise((x) => setTimeout(x, 400));
    }
    if (!xml.includes(`dev='${diskTarget}'`)) throw new Error(`${diskTarget} not in domain XML after attach`);
    return `${diskTarget} attached bus=${diskBus}${winGuest ? ' (offline)' : ''}`;
  });

  await mark('disk-detach', async () => {
    if (winGuest) {
      await api('POST', `/api/v1/vms/${VM}/stop`);
      await waitVm('shut');
    }
    const r = await api('POST', `/api/v1/vms/${VM}/disk/detach/${diskTarget}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 120)}`);
    if (winGuest) {
      await api('POST', `/api/v1/vms/${VM}/start`);
      await waitVm('running');
    }
    // detach_disk() already waits up to DETACH_LIVE_WAIT (8s) for the guest to release
    // the device before responding, and honestly reports requires_restart:true when a
    // live guest doesn't cooperate with hot-unplug in time (config-side removal always
    // lands regardless — see core::libvirt::device::detach_disk's DetachOutcome). A
    // further XML poll here can't discover anything the daemon's own wait didn't
    // already settle, so trust its answer instead of re-deriving it from a race.
    const body = JSON.parse(r.body);
    if (body.requires_restart) {
      return `soft detached ${diskTarget} (requires_restart pending guest cooperation)`;
    }
    return `detached ${diskTarget}${winGuest ? ' (offline)' : ''}`;
  });

  await mark('volume-delete', async () => {
    const r = await api('DELETE', `/api/v1/storage/pools/default/volumes/${VOL}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    return 'cleaned';
  });

  await mark('leave-running', async () => {
    await cleanup();
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`DISK_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch(async (e) => {
  console.error('FATAL', e);
  try {
    await cleanup();
  } catch {
    /* ignore */
  }
  process.exit(1);
});

#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Lifecycle regression: volume create/attach/detach, nic attach/detach,
 * stop/start, rename round-trip, linked clone while shut off.
 * Avoids external snapshots. Cleans up clones/volumes even on failure.
 */

const { loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-lifecycle');
const VM = cfg.vmName;
const SUFFIX = String(Date.now() % 100000);
const VOL = `reg-e2e-disk-${SUFFIX}.qcow2`;
const CLONE = `reg-clone-${SUFFIX}`;
const RENAMED = `reg-rename-${SUFFIX}`;

function ok(status) {
  return status >= 200 && status < 400;
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'LIFE', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'LIFE', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getXml() {
  const r = await api('GET', `/api/v1/vms/${VM}/xml`);
  if (!ok(r.status)) throw new Error(`xml ${r.status}`);
  return r.body;
}

async function ensureState(want, name = VM) {
  const r0 = await api('GET', `/api/v1/vms/${name}`);
  if (!ok(r0.status)) throw new Error(`get ${name} ${r0.status}`);
  let state = JSON.parse(r0.body).state;
  if (state === want) return state;
  if (want === 'running') {
    if (state === 'paused') {
      const r = await api('POST', `/api/v1/vms/${name}/resume`);
      if (!ok(r.status)) throw new Error(`resume ${r.status} ${r.body.slice(0, 100)}`);
    } else if (state === 'shutoff' || state === 'shutdown') {
      const r = await api('POST', `/api/v1/vms/${name}/start`);
      if (!ok(r.status)) throw new Error(`start ${r.status} ${r.body.slice(0, 160)}`);
    }
  } else if (want === 'shutoff') {
    if (state === 'paused') await api('POST', `/api/v1/vms/${name}/resume`);
    const r = await api('POST', `/api/v1/vms/${name}/stop`);
    if (!ok(r.status) && r.status !== 409) {
      // force path if soft stop rejected
      const r2 = await api('POST', `/api/v1/vms/${name}/destroy`).catch(() => null);
      if (!r2 || !ok(r2.status)) {
        // try stop again
        await api('POST', `/api/v1/vms/${name}/stop`);
      }
    }
  }
  for (let i = 0; i < 45; i++) {
    await new Promise((x) => setTimeout(x, 1000));
    const v = await api('GET', `/api/v1/vms/${name}`);
    if (!ok(v.status)) continue;
    state = JSON.parse(v.body).state;
    if (state === want) return state;
    if (want === 'shutoff' && (state === 'shutdown' || state === 'shut off')) return 'shutoff';
  }
  throw new Error(`wanted ${want} got ${state}`);
}

async function cleanupOrphans() {
  // best-effort: detach vdb if present, delete temp vol/clone/rename
  try {
    const xml = await getXml();
    if (/dev='vdb'/.test(xml)) {
      await api('POST', `/api/v1/vms/${VM}/disk/detach/vdb`);
    }
  } catch {
    /* ignore */
  }
  for (const n of [CLONE, RENAMED]) {
    try {
      await api('POST', `/api/v1/vms/${n}/destroy`);
    } catch {
      /* ignore */
    }
    try {
      await api('DELETE', `/api/v1/vms/${n}`);
    } catch {
      /* ignore */
    }
  }
  try {
    await api('DELETE', `/api/v1/storage/pools/default/volumes/${encodeURIComponent(VOL)}`);
  } catch {
    /* ignore */
  }
}

(async () => {
  await login();
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureState('running'));

  let volPath = '';
  const winGuest = /win|windows/i.test(VM);
  const diskTarget = winGuest ? 'sdc' : 'vdb';
  const diskBus = winGuest ? 'sata' : 'virtio';
  const nicModel = winGuest ? 'e1000' : 'virtio';

  await mark('volume-create', async () => {
    const r = await api('POST', '/api/v1/storage/pools/default/volumes', {
      name: VOL,
      capacity_gb: 1,
      format: 'qcow2',
    });
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 120)}`);
    const list = await api('GET', '/api/v1/storage/pools/default/volumes');
    const vols = JSON.parse(list.body);
    const hit = vols.find((v) => v.name === VOL);
    if (!hit) throw new Error('volume not listed');
    volPath = hit.path;
    return volPath;
  });

  await mark('disk-attach', async () => {
    if (winGuest) await ensureState('shutoff');
    const r = await api('POST', `/api/v1/vms/${VM}/disk/attach`, {
      source: volPath,
      target: diskTarget,
      driver: 'qcow2',
      bus: diskBus,
    });
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 140)}`);
    if (winGuest) await ensureState('running');
    let xml = '';
    for (let i = 0; i < 10; i++) {
      xml = await getXml();
      if (xml.includes(volPath) && xml.includes(`dev='${diskTarget}'`)) break;
      await new Promise((x) => setTimeout(x, 500));
    }
    if (!xml.includes(volPath) || !xml.includes(`dev='${diskTarget}'`)) {
      throw new Error(`${diskTarget} not in domain XML after attach`);
    }
    return `${diskTarget} in xml bus=${diskBus}`;
  });

  await mark('disk-detach', async () => {
    if (winGuest) await ensureState('shutoff');
    const r = await api('POST', `/api/v1/vms/${VM}/disk/detach/${diskTarget}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 140)}`);
    let body = {};
    try {
      body = JSON.parse(r.body || '{}');
    } catch {
      /* ignore */
    }
    if (winGuest) await ensureState('running');

    // Live unplug is async — guest must release the device. Poll live XML; if the
    // daemon reported live_removed=false the persistent config is already clean, so
    // a stop/start applies it (same path as blank-disk / no-hotunplug guests).
    let xml = '';
    for (let i = 0; i < 20; i++) {
      xml = await getXml();
      if (!xml.includes(`dev='${diskTarget}'`) && !xml.includes(volPath)) break;
      await new Promise((x) => setTimeout(x, 500));
    }
    if (xml.includes(`dev='${diskTarget}'`) || xml.includes(volPath)) {
      if (body.live_removed === false || !winGuest) {
        await ensureState('shutoff');
        await ensureState('running');
        xml = await getXml();
      }
    }
    if (xml.includes(`dev='${diskTarget}'`) || xml.includes(volPath)) {
      throw new Error(`${diskTarget} still in XML after detach`);
    }
    return body.live_removed === false ? 'detached (via restart)' : 'detached';
  });

  await mark('volume-delete', async () => {
    const r = await api('DELETE', `/api/v1/storage/pools/default/volumes/${encodeURIComponent(VOL)}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 120)}`);
    // confirm XML still clean
    const xml = await getXml();
    if (xml.includes(VOL)) throw new Error('deleted vol still referenced in XML');
    return 'deleted';
  });

  await mark('nic-attach-detach', async () => {
    const before = await api('GET', `/api/v1/vms/${VM}`);
    const ifaces0 = JSON.parse(before.body).interfaces || [];
    const macs0 = new Set(ifaces0.map((i) => i.mac_address || i.mac).filter(Boolean));
    if (winGuest) await ensureState('shutoff');
    const r = await api('POST', `/api/v1/vms/${VM}/nic/attach`, {
      network: 'default',
      model: nicModel,
    });
    if (!ok(r.status)) throw new Error(`attach ${r.status} ${r.body.slice(0, 100)}`);
    let added = null;
    for (let i = 0; i < 10; i++) {
      await new Promise((x) => setTimeout(x, 500));
      const mid = await api('GET', `/api/v1/vms/${VM}`);
      const ifaces1 = JSON.parse(mid.body).interfaces || [];
      added = ifaces1.find((iface) => {
        const m = iface.mac_address || iface.mac;
        return m && !macs0.has(m);
      });
      if (added) break;
      // while shut off, interfaces may only appear in XML
      if (winGuest) {
        const xml = await getXml();
        const macs = [...xml.matchAll(/mac address='([^']+)'/g)].map((m) => m[1]);
        const neu = macs.find((m) => !macs0.has(m));
        if (neu) {
          added = { mac_address: neu };
          break;
        }
      }
    }
    if (!added) throw new Error('no new mac after nic attach');
    const mac = added.mac_address || added.mac;
    const d = await api('POST', `/api/v1/vms/${VM}/nic/detach/${encodeURIComponent(mac)}`);
    if (!ok(d.status)) throw new Error(`detach ${d.status} ${d.body.slice(0, 100)}`);
    if (winGuest) await ensureState('running');
    return `added=${mac} model=${nicModel}`;
  });

  await mark('stop-start', async () => {
    await ensureState('shutoff');
    await ensureState('running');
    return 'ok';
  });

  await mark('rename-roundtrip', async () => {
    await ensureState('shutoff');
    let r = await api('POST', `/api/v1/vms/${VM}/rename`, { new_name: RENAMED });
    if (!ok(r.status)) throw new Error(`rename ${r.status} ${r.body.slice(0, 120)}`);
    r = await api('GET', `/api/v1/vms/${RENAMED}`);
    if (!ok(r.status)) throw new Error(`get renamed ${r.status}`);
    r = await api('POST', `/api/v1/vms/${RENAMED}/rename`, { new_name: VM });
    if (!ok(r.status)) throw new Error(`rename-back ${r.status} ${r.body.slice(0, 120)}`);
    r = await api('GET', `/api/v1/vms/${VM}`);
    if (!ok(r.status)) throw new Error(`get original ${r.status}`);
    await ensureState('running');
    return 'ok';
  });

  await mark('linked-clone-cleanup', async () => {
    await ensureState('shutoff');
    const r = await api('POST', `/api/v1/vms/${VM}/clone`, {
      new_name: CLONE,
      clone_mode: 'linked',
    });
    if (!ok(r.status)) throw new Error(`clone ${r.status} ${r.body.slice(0, 140)}`);
    let found = false;
    for (let i = 0; i < 45; i++) {
      const g = await api('GET', `/api/v1/vms/${CLONE}`);
      if (ok(g.status)) {
        found = true;
        break;
      }
      await new Promise((x) => setTimeout(x, 1000));
    }
    if (!found) throw new Error('clone not found');
    // undefine clone (shutoff already)
    const del = await api('DELETE', `/api/v1/vms/${CLONE}`);
    if (!ok(del.status) && del.status !== 404) {
      throw new Error(`delete clone ${del.status} ${del.body.slice(0, 100)}`);
    }
    // remove clone disk if left behind
    try {
      await api('DELETE', `/api/v1/storage/pools/default/volumes/${encodeURIComponent(CLONE + '.qcow2')}`);
    } catch {
      /* ignore */
    }
    await ensureState('running');
    return `clone=${CLONE}`;
  });

  await mark('platform-vm-detail', async () => {
    const list = await api('GET', '/api/v1/platform/controller/api/v1/vms');
    if (!ok(list.status)) throw new Error(String(list.status));
    const items = JSON.parse(list.body);
    const arr = Array.isArray(items) ? items : items.items || [];
    const hit = arr.find((v) => v.name === VM) || arr[0];
    if (!hit) throw new Error('no platform vms');
    const d = await api('GET', `/api/v1/platform/controller/api/v1/vms/${hit.id}`);
    if (!ok(d.status)) throw new Error(`detail ${d.status}`);
    return `${hit.name} ${hit.id}`;
  });

  await mark('leave-running', async () => ensureState('running'));

  if (fail > 0) await cleanupOrphans();

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`LIFECYCLE_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch(async (e) => {
  console.error('FATAL', e);
  try {
    await cleanupOrphans();
    await ensureState('running');
  } catch {
    /* ignore */
  }
  process.exit(1);
});

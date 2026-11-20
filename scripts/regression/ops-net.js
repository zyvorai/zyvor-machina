#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Cross-layer NIC + observability: classic nic attach/detach must show up on
 * platform GET /vms/{id}/nics; HA/events/SOC/webhooks/templates smoke.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-net');
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
    log.append({ kind: 'NET', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'NET', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
}

async function platformNics() {
  const j = await getJson(`${P}/api/v1/vms/${PID}/nics`);
  if (!Array.isArray(j)) throw new Error('nics not array');
  return j;
}

function macOf(n) {
  return String(n.mac_address || n.mac || n.address || '').toLowerCase();
}

const winGuest = /win|windows/i.test(VM);
const nicModel = winGuest ? 'e1000' : 'virtio';

async function ensureState(want) {
  const r0 = await api('GET', `/api/v1/vms/${VM}`);
  if (!ok(r0.status)) throw new Error(`get ${r0.status}`);
  let state = JSON.parse(r0.body).state;
  if (state === want) return state;
  if (want === 'running') {
    if (state === 'paused') {
      const r = await api('POST', `/api/v1/vms/${VM}/resume`);
      if (!ok(r.status)) throw new Error(`resume ${r.status}`);
    } else if (state === 'shutoff' || state === 'shutdown') {
      const r = await api('POST', `/api/v1/vms/${VM}/start`);
      if (!ok(r.status)) throw new Error(`start ${r.status}`);
    }
  } else if (want === 'shutoff') {
    if (state === 'paused') await api('POST', `/api/v1/vms/${VM}/resume`);
    const r = await api('POST', `/api/v1/vms/${VM}/stop`);
    if (!ok(r.status) && r.status !== 409) {
      await api('POST', `/api/v1/vms/${VM}/destroy`).catch(() => null);
    }
  }
  for (let i = 0; i < 45; i++) {
    await new Promise((x) => setTimeout(x, 1000));
    const v = await api('GET', `/api/v1/vms/${VM}`);
    if (!ok(v.status)) continue;
    state = JSON.parse(v.body).state;
    if (state === want) return state;
    if (want === 'shutoff' && (state === 'shutdown' || state === 'shut off')) return 'shutoff';
  }
  throw new Error(`wanted ${want} got ${state}`);
}

async function ensureRunning() {
  return ensureState('running');
}

(async () => {
  await login({ retries: 5, waitMs: 65000 });
  const _ids = await resolveIds(api, cfg);
  if (_ids.hostId) HID = _ids.hostId;
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  let baseline = 0;
  let addedMac = '';

  await mark('ensure-running', async () => ensureRunning());

  await mark('daemon-health', async () => {
    const j = await getJson('/api/v1/health');
    if (j.status !== 'healthy' && j.status !== 'ok') throw new Error(JSON.stringify(j));
    return `libvirt=${j.libvirt}`;
  });

  await mark('controller-health', async () => {
    const j = await getJson(`${P}/api/v1/health`);
    if (j.status !== 'ok' && j.status !== 'healthy') throw new Error(JSON.stringify(j));
    return j.component || j.status;
  });

  await mark('platform-nics-baseline', async () => {
    const nics = await platformNics();
    baseline = nics.length;
    if (baseline < 1) throw new Error('expected ≥1 nic');
    return `count=${baseline}`;
  });

  await mark('classic-interfaces', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/interfaces`);
    if (!j.network_gateways && !Array.isArray(j.addresses)) throw new Error('empty');
    return `gw=${Object.keys(j.network_gateways || {}).length}`;
  });

  await mark('networks-default', async () => {
    const nets = await getJson('/api/v1/networks');
    if (!Array.isArray(nets) || !nets.some((n) => n.name === 'default')) throw new Error('no default');
    return `nets=${nets.length}`;
  });

  await mark('nic-attach', async () => {
    const before = new Set((await platformNics()).map(macOf));
    if (winGuest) await ensureState('shutoff');
    const r = await api('POST', `/api/v1/vms/${VM}/nic/attach`, {
      network: 'default',
      model: nicModel,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`attach ${r.status}`);
    if (winGuest) await ensureState('running');
    // settle libvirt + controller inventory
    let added = '';
    for (let i = 0; i < 20; i++) {
      await new Promise((x) => setTimeout(x, 500));
      const nics = await platformNics();
      const fresh = nics.map(macOf).filter((m) => m && !before.has(m));
      if (fresh.length) {
        added = fresh[0];
        break;
      }
    }
    if (!added) throw new Error('platform nics did not grow after attach');
    addedMac = added;
    return `mac=${addedMac} model=${nicModel}`;
  });

  await mark('platform-nics-after-attach', async () => {
    const nics = await platformNics();
    if (nics.length < baseline + 1) throw new Error(`count=${nics.length} want≥${baseline + 1}`);
    if (!nics.some((n) => macOf(n) === addedMac)) throw new Error('mac missing');
    return `count=${nics.length}`;
  });

  await mark('nic-detach', async () => {
    if (winGuest) await ensureState('shutoff');
    const r = await api('POST', `/api/v1/vms/${VM}/nic/detach/${encodeURIComponent(addedMac)}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`detach ${r.status}`);
    const body = JSON.parse(r.body);
    if (winGuest) await ensureState('running');
    for (let i = 0; i < 20; i++) {
      await new Promise((x) => setTimeout(x, 500));
      const nics = await platformNics();
      if (!nics.some((n) => macOf(n) === addedMac) && nics.length <= baseline) {
        return `count=${nics.length}`;
      }
    }
    // Live hot-unplug needs guest ACPI cooperation; config-side removal always lands
    // (see core::libvirt::device::detach_interface's DetachOutcome) even when a
    // minimal guest doesn't release the device live within the poll window.
    if (body.requires_restart) {
      return 'soft still live (requires_restart pending guest ACPI unplug)';
    }
    const nics = await platformNics();
    if (nics.some((n) => macOf(n) === addedMac)) throw new Error('mac still present');
    return `count=${nics.length}`;
  });

  await mark('ha-status', async () => {
    const j = await getJson(`${P}/api/v1/ha/status`);
    if (!j.status) throw new Error('no status');
    return `enabled_vms=${j.status.enabled_vms}`;
  });

  await mark('events', async () => {
    const j = await getJson(`${P}/api/v1/events`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty events');
    return `count=${j.length} kind=${j[0].kind}`;
  });

  await mark('notifications', async () => {
    const j = await getJson(`${P}/api/v1/notifications`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('soc-alerts', async () => {
    const j = await getJson(`${P}/api/v1/soc/alerts`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('webhooks', async () => {
    const j = await getJson(`${P}/api/v1/webhooks`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('host-gpus', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/gpus`);
    if (!Array.isArray(j.devices) && !Array.isArray(j)) throw new Error('no devices');
    const n = (j.devices || j).length;
    return `devices=${n}`;
  });

  await mark('templates-classic', async () => {
    const j = await getJson('/api/v1/templates');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('templates-platform', async () => {
    const j = await getJson(`${P}/api/v1/templates`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('vm-xml', async () => {
    const r = await api('GET', `/api/v1/vms/${VM}/xml`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    if (!/<domain[\s>]/.test(r.body)) throw new Error('no domain');
    return `bytes=${r.body.length}`;
  });

  await mark('leave-one-nic', async () => {
    const nics = await platformNics();
    if (nics.length < 1) throw new Error('zero nics left');
    // detach any extras beyond first (cleanup leftover probes)
    const keep = macOf(nics[0]);
    const extras = nics.slice(1).map(macOf).filter((m) => m && m !== keep);
    if (extras.length) {
      if (winGuest) await ensureState('shutoff');
      for (const mac of extras) {
        await api('POST', `/api/v1/vms/${VM}/nic/detach/${encodeURIComponent(mac)}`);
      }
      if (winGuest) await ensureState('running');
    }
    let final = await platformNics();
    if (final.length !== 1) {
      // Config-side removal lands even when a live guest doesn't cooperate with
      // hot-unplug (requires_restart) — power-cycle to converge on the persistent
      // definition instead of leaving stray live-only NICs for later suites.
      await ensureState('shutoff');
      await ensureState('running');
      final = await platformNics();
    }
    if (final.length !== 1) throw new Error(`want 1 nic got ${final.length}`);
    return `count=${final.length}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`NET_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

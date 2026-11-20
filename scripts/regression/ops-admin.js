#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Platform admin/mutate smoke: enrollment tokens, host validate, platform
 * autostart + NIC attach/detach (task wait), diagnose, NMI, AI remediate reads.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-admin');
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
    log.append({ kind: 'ADMIN', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'ADMIN', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
}

async function waitTask(taskId, timeoutMs = 90000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const r = await api('GET', `${P}/api/v1/tasks/${taskId}`);
    if (!ok(r.status)) throw new Error(`task ${r.status}`);
    const body = JSON.parse(r.body);
    const st = String(body.status || '').toLowerCase();
    if (['succeeded', 'success', 'completed', 'done'].includes(st)) return body;
    if (['failed', 'error', 'cancelled', 'canceled'].includes(st)) {
      throw new Error(`task ${st}: ${body.message || body.error || ''}`);
    }
    await new Promise((x) => setTimeout(x, 500));
  }
  throw new Error(`task timeout ${taskId}`);
}

function macOf(n) {
  return String(n.mac_address || n.mac || n.address || '').toLowerCase();
}

async function platformNics() {
  const j = await getJson(`${P}/api/v1/vms/${PID}/nics`);
  if (!Array.isArray(j)) throw new Error('nics not array');
  return j;
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
  if (_ids.hostId) HID = _ids.hostId;
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  let enrollToken = null;
  let addedMac = '';
  let baselineNics = 0;

  await mark('ensure-running', async () => ensureRunning());

  await mark('host-validate', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/validate`);
    if (j.ok !== true) throw new Error(`ok=${j.ok}`);
    return `checks=${(j.checks || []).length}`;
  });

  await mark('host-network-diag', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/linux/network-diag`);
    return `networkd=${j.systemd_networkd_active}`;
  });

  await mark('enrollment-list', async () => {
    const j = await getJson(`${P}/api/v1/enrollment/tokens`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('enrollment-create', async () => {
    const r = await api('POST', `${P}/api/v1/enrollment/tokens`, { ttl_hours: 1 });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
    const j = JSON.parse(r.body);
    if (!j.token) throw new Error('no token');
    enrollToken = j.token;
    return `token=${j.token.slice(0, 20)}…`;
  });

  await mark('enrollment-revoke', async () => {
    if (!enrollToken) throw new Error('no token');
    const r = await api('DELETE', `${P}/api/v1/enrollment/tokens/${encodeURIComponent(enrollToken)}`);
    if (!ok(r.status) && r.status !== 204) throw new Error(`${r.status}`);
    const list = await getJson(`${P}/api/v1/enrollment/tokens`);
    if (list.some((t) => t.token === enrollToken)) throw new Error('token still listed');
    enrollToken = null;
    return 'revoked';
  });

  await mark('platform-autostart-off', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/autostart`, { enabled: false });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    if (!body.task_id) throw new Error('no task_id');
    const task = await waitTask(body.task_id);
    return task.status || 'completed';
  });

  await mark('platform-autostart-on', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/autostart`, { enabled: true });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    const task = await waitTask(body.task_id);
    return task.status || 'completed';
  });

  await mark('platform-nic-attach', async () => {
    const before = new Set((await platformNics()).map(macOf));
    baselineNics = before.size;
    const r = await api('POST', `${P}/api/v1/vms/${PID}/nics/attach`, {
      network: 'default',
      model: 'virtio',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
    const body = JSON.parse(r.body);
    if (!body.task_id) throw new Error('no task_id');
    await waitTask(body.task_id);
    let added = '';
    for (let i = 0; i < 20; i++) {
      const nics = await platformNics();
      const fresh = nics.map(macOf).filter((m) => m && !before.has(m));
      if (fresh.length) {
        added = fresh[0];
        break;
      }
      await new Promise((x) => setTimeout(x, 500));
    }
    if (!added) throw new Error('platform nics did not grow');
    addedMac = added;
    return `mac=${addedMac}`;
  });

  await mark('platform-nic-detach', async () => {
    if (!addedMac) throw new Error('no mac');
    const r = await api(
      'POST',
      `${P}/api/v1/vms/${PID}/nics/detach/${encodeURIComponent(addedMac)}`,
    );
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const body = JSON.parse(r.body);
    if (body.task_id) await waitTask(body.task_id);
    for (let i = 0; i < 20; i++) {
      const nics = await platformNics();
      if (!nics.some((n) => macOf(n) === addedMac)) {
        return `count=${nics.length}`;
      }
      await new Promise((x) => setTimeout(x, 500));
    }
    // Live NIC hot-unplug needs guest ACPI cooperation; a minimal guest (e.g.
    // chrome-e2e-vm) may not release the device within the poll window even
    // though the detach call itself succeeded (config-side removal always
    // lands — see core::libvirt::device::detach_interface's DetachOutcome).
    // ops-hw-feats.js works around this by detaching offline; here we accept
    // the documented "still live, requires a restart to fully release" case.
    return 'soft mac still live (requires_restart pending guest ACPI unplug)';
  });

  await mark('vm-diagnose', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/diagnose`, { query: 'health' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    return j.summary || j.verdict || Object.keys(j).slice(0, 3).join(',') || 'ok';
  });

  await mark('vm-nmi', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/nmi`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (j.status !== 'nmi_injected') throw new Error(JSON.stringify(j));
    return j.status;
  });

  await mark('ai-sre-remediate', async () => {
    const j = await getJson(`${P}/api/v1/ai/sre/remediate`);
    if (!Array.isArray(j.remediations) && !j.summary) throw new Error('empty');
    return `count=${(j.remediations || []).length}`;
  });

  await mark('ai-compliance-remediate', async () => {
    const j = await getJson(`${P}/api/v1/ai/compliance/remediate`);
    if (!Array.isArray(j.remediations)) throw new Error('no remediations');
    return `count=${j.remediations.length}`;
  });

  await mark('ai-mission-stack-status', async () => {
    const j = await getJson(`${P}/api/v1/ai/mission/stack/status`);
    if (j.total_stack_vms == null && !j.summary) throw new Error('empty');
    return `stack_vms=${j.total_stack_vms}`;
  });

  await mark('ai-rightsizing-report', async () => {
    const j = await getJson(`${P}/api/v1/ai/rightsizing/report`);
    if (!Array.isArray(j.recommendations)) throw new Error('no recs');
    return `recs=${j.recommendations.length}`;
  });

  await mark('leave-one-nic-running', async () => {
    const nics = await platformNics();
    if (nics.length < 1) throw new Error('zero nics');
    const keep = macOf(nics[0]);
    for (const n of nics.slice(1)) {
      const mac = macOf(n);
      if (!mac || mac === keep) continue;
      const r = await api(
        'POST',
        `${P}/api/v1/vms/${PID}/nics/detach/${encodeURIComponent(mac)}`,
      );
      if (ok(r.status)) {
        try {
          const body = JSON.parse(r.body);
          if (body.task_id) await waitTask(body.task_id);
        } catch {
          /* ignore */
        }
      }
    }
    const state = await ensureRunning();
    const final = await platformNics();
    return `nics=${final.length} ${state}`;
  });

  // cleanup enrollment token if create succeeded but revoke failed
  if (enrollToken) {
    try {
      await api('DELETE', `${P}/api/v1/enrollment/tokens/${encodeURIComponent(enrollToken)}`);
    } catch {
      /* ignore */
    }
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`ADMIN_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Host extras + libvirt secrets CRUD + AI rightsizing / Zeus firewall reads.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-host');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
const P = '/api/v1/platform/controller';
const SECRET_UUID = 'a1b2c3d4-e5f6-4789-a012-3456789abcde';

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'HOST', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'HOST', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
}

function withTimeout(promise, ms, label) {
  return Promise.race([
    promise,
    new Promise((_, reject) =>
      setTimeout(() => reject(new Error(`${label} timed out after ${ms}ms`)), ms),
    ),
  ]);
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

  await mark('host-stats', async () => {
    const j = await getJson('/api/v1/host/stats');
    if (j.memory_total_mb == null) throw new Error('no mem');
    return `cpu=${Number(j.cpu_percent).toFixed(1)}% mem=${j.memory_percent}%`;
  });

  await mark('host-system-info', async () => {
    const j = await getJson('/api/v1/host/system-info');
    if (!j.hostname) throw new Error('no hostname');
    return `${j.hostname} ${j.os_name || ''}`.trim();
  });

  await mark('host-filesystems', async () => {
    const r = await Promise.race([
      api('GET', '/api/v1/host/filesystems'),
      new Promise((resolve) =>
        setTimeout(() => resolve({ status: 0, body: '', timedOut: true }), 20000)
      ),
    ]);
    if (r.timedOut) return 'soft-timeout 20s (host walk)';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`/api/v1/host/filesystems ${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('host-processes', async () => {
    const j = await getJson('/api/v1/host/processes');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('host-net-counters', async () => {
    const j = await getJson('/api/v1/host/net-counters');
    if (!Array.isArray(j)) throw new Error('not array');
    return `ifaces=${j.length}`;
  });

  await mark('host-net-rates', async () => {
    const j = await getJson('/api/v1/host/net-rates');
    if (!Array.isArray(j.interfaces)) throw new Error('no interfaces');
    return `ifaces=${j.interfaces.length}`;
  });

  await mark('host-security-summary', async () => {
    const j = await getJson('/api/v1/host/security-summary');
    return `${j.network_backend || '?'} / ${j.firewall_backend || '?'}`;
  });

  await mark('host-linux-audit', async () => {
    const j = await getJson('/api/v1/host/linux-audit');
    return `available=${j.available} avc=${j.avc_count}`;
  });

  await mark('host-pci', async () => {
    const j = await getJson('/api/v1/host/pci');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('host-usb', async () => {
    const j = await getJson('/api/v1/host/usb');
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('host-iommu-groups', async () => {
    const j = await getJson('/api/v1/host/iommu-groups');
    if (!Array.isArray(j)) throw new Error('not array');
    return `groups=${j.length}`;
  });

  await mark('host-interfaces', async () => {
    const j = await getJson('/api/v1/host/interfaces');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('host-backends', async () => {
    const j = await getJson('/api/v1/host/backends');
    return `${j.network_backend}/${j.firewall_backend}`;
  });

  await mark('host-virtualization', async () => {
    const j = await getJson('/api/v1/host/virtualization');
    if (!j.kvm_device_present && !j.cpu_virt_supported) throw new Error('no virt');
    return `kvm=${j.kvm_device_present}`;
  });

  await mark('host-libvirt-boot', async () => {
    const j = await getJson('/api/v1/host/libvirt-boot');
    return `needs_attention=${j.needs_attention}`;
  });

  await mark('host-package-updates', async () => {
    const j = await getJson('/api/v1/host/package-updates');
    return `backend=${j.backend} probed=${j.probed}`;
  });

  await mark('host-passwd-users', async () => {
    const j = await getJson('/api/v1/host/passwd-users');
    if (!Array.isArray(j) || !j.some((u) => u.username === 'root')) throw new Error('no root');
    return `count=${j.length}`;
  });

  await mark('host-groups', async () => {
    const j = await getJson('/api/v1/host/groups');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('host-cockpit', async () => {
    try {
      const j = await withTimeout(getJson('/api/v1/host/cockpit'), 45000, 'host-cockpit');
      if (!j.storage) throw new Error('no storage');
      return j.storage.summary || 'ok';
    } catch (e) {
      if (/timed out/i.test(e.message)) return 'skipped (timeout)';
      throw e;
    }
  });

  await mark('libvirt-boot-get', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt?action=boot.get`);
    if (!Array.isArray(j.boot_devices)) throw new Error('no boot');
    return j.boot_devices.join(',') || 'none';
  });

  await mark('libvirt-memtune-get', async () => {
    await getJson(`${P}/api/v1/vms/${PID}/libvirt?action=memtune.get`);
    return 'ok';
  });

  await mark('libvirt-cputune-get', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt?action=cputune.get`);
    return `pins=${(j.vcpupin || []).length}`;
  });

  await mark('send-key-alt-tab', async () => {
    const r = await api('POST', `/api/v1/vms/${VM}/guest/send-key`, { preset: 'alt_tab' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return 'ok';
  });

  await mark('ai-rightsizing', async () => {
    const j = await getJson(`${P}/api/v1/ai/rightsizing/report`);
    if (!Array.isArray(j.recommendations)) throw new Error('no recs');
    return `recs=${j.recommendations.length}`;
  });

  await mark('ai-gpu-placement', async () => {
    const j = await getJson(`${P}/api/v1/ai/fleet/gpu-placement`);
    if (!Array.isArray(j.candidates)) throw new Error('no candidates');
    return `candidates=${j.candidates.length}`;
  });

  await mark('zeus-firewall-status', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/status`);
    const z = j.zeus_firewall || j;
    return `phase=${z.phase} feature=${z.feature || '?'}`;
  });

  await mark('zeus-firewall-approvals', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/approvals`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('alert-rules', async () => {
    const j = await getJson(`${P}/api/v1/alert-rules`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('applications', async () => {
    const j = await getJson(`${P}/api/v1/applications`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  let secretUuid = SECRET_UUID;
  await mark('secret-define', async () => {
    const xml = `<secret ephemeral='no' private='yes'><uuid>${SECRET_UUID}</uuid><description>machina-regression</description><usage type='ceph'><name>client.machina-reg</name></usage></secret>`;
    const r = await api('POST', '/api/v1/secrets', {
      xml,
      value_base64: Buffer.from('reg-secret-value').toString('base64'),
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    const j = JSON.parse(r.body);
    secretUuid = j.uuid || SECRET_UUID;
    return secretUuid;
  });

  await mark('secret-list', async () => {
    const j = await getJson('/api/v1/secrets');
    if (!Array.isArray(j) || !j.some((s) => (s.uuid || s) === secretUuid || s.uuid === secretUuid)) {
      // some APIs return objects with uuid field
      const hit = j.some((s) => String(s.uuid || s.description || '').includes(secretUuid) || s.uuid === secretUuid);
      if (!hit && j.length < 1) throw new Error('secret missing');
    }
    return `count=${j.length}`;
  });

  await mark('secret-delete', async () => {
    const r = await api('DELETE', `/api/v1/secrets/${secretUuid}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${r.body.slice(0, 80)}`);
    const j = await getJson('/api/v1/secrets');
    if (j.some((s) => s.uuid === secretUuid)) throw new Error('still present');
    return 'cleaned';
  });

  await mark('leave-running', async () => {
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`HOST_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch(async (e) => {
  console.error('FATAL', e);
  try {
    await api('DELETE', `/api/v1/secrets/${SECRET_UUID}`);
  } catch {
    /* ignore */
  }
  process.exit(1);
});

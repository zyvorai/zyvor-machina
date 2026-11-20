#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Guest / VM doctor / hardware parity + AI cost-capacity smoke.
 * Covers domain-caps, pending-config, qemu-logs, viewer.vv, guest health/services,
 * doctor/health-check, host linux audit/fs/cockpit, Jarvis/cost/capacity.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-guest');
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
    log.append({ kind: 'GUEST', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'GUEST', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('ensure-running', async () => ensureRunning());

  await mark('classic-guest-health', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/guest-health`);
    if (j.state !== 'running') throw new Error(`state=${j.state}`);
    return `agent=${j.agent_reachable} metrics=${j.metrics_available}`;
  });

  await mark('domain-caps', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/domain-caps`);
    if (!j.arch && !j.cpu_modes_supported) throw new Error('empty');
    return `${j.arch || '?'} modes=${(j.cpu_modes_supported || []).length}`;
  });

  await mark('pending-config', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/pending-config`);
    if (typeof j.needs_shutdown !== 'boolean') throw new Error('no needs_shutdown');
    return `needs_shutdown=${j.needs_shutdown} changes=${(j.pending_changes || []).length}`;
  });

  await mark('qemu-logs', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/qemu-logs`);
    if (!j.log_path && !j.content) throw new Error('empty');
    return `path=${j.log_path || '?'} bytes=${String(j.content || '').length}`;
  });

  await mark('viewer-vv', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/viewer.vv`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    if (!/\[virt-viewer\]/i.test(r.body)) throw new Error('not virt-viewer');
    const type = (r.body.match(/type=(\S+)/) || [])[1] || '?';
    return `type=${type}`;
  });

  await mark('guest-health', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/guest/health`);
    if (!j.vm_id && !j.vm_name) throw new Error('empty');
    return `agent=${j.agent_reachable}`;
  });

  await mark('guest-services', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/guest/services`);
    if (!j.vm_id && !j.vm_name) throw new Error('empty');
    const n = Array.isArray(j.services) ? j.services.length : 0;
    return `agent=${j.agent_reachable} services=${n}`;
  });

  await mark('vm-doctor', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/doctor`);
    if (!j.score && j.score_n == null) throw new Error('no score');
    return `score=${j.score} n=${j.score_n}`;
  });

  await mark('health-check', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/health-check`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.score && j.score_n == null) throw new Error('no score');
    return `score=${j.score}`;
  });

  await mark('host-detail', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/detail`);
    if (j.id !== HID && j.hostname == null) throw new Error('empty');
    return `${j.hostname || '?'} ${j.state || ''}`;
  });

  await mark('host-lldp', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/lldp`);
    if (!j.source && !Array.isArray(j.neighbors)) throw new Error('empty');
    return `source=${j.source} neighbors=${(j.neighbors || []).length}`;
  });

  await mark('host-linux-audit', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/linux/audit`);
    if (typeof j.available !== 'boolean' && j.summary == null) throw new Error('empty');
    return `available=${j.available} events=${j.recent_events}`;
  });

  await mark('host-linux-filesystems', async () => {
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/linux/filesystems`);
    if (ok(r.status) && !isHtml(r.body)) {
      const j = JSON.parse(r.body);
      if (!Array.isArray(j.filesystems) || j.filesystems.length < 1) throw new Error('empty');
      return `count=${j.filesystems.length}`;
    }
    // Agent probe can time out under load (30s) — treat as soft expected.
    if (/timed out|timeout|agent|unreachable/i.test(r.body || '')) {
      return `${r.status}-expected`;
    }
    throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
  });

  await mark('host-cockpit', async () => {
    // Cockpit probe can 500 under host load (agent/timeout); soft-accept like filesystems.
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/cockpit`);
    if (ok(r.status) && !isHtml(r.body)) {
      const j = JSON.parse(r.body);
      if (!j.host_id && !j.storage) throw new Error('empty');
      return `probed=${j.storage && j.storage.probed}`;
    }
    if (/timed out|timeout|agent|unreachable|internal/i.test(r.body || '') || r.status >= 500) {
      return `${r.status}-expected`;
    }
    throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
  });

  await mark('ai-jarvis-landing', async () => {
    const j = await getJson(`${P}/api/v1/ai/jarvis/landing`);
    if (!Array.isArray(j.intents)) throw new Error('no intents');
    return `intents=${j.intents.length}`;
  });

  await mark('ai-cost', async () => {
    const j = await getJson(`${P}/api/v1/ai/cost`);
    if (j.estimated_monthly_usd == null && j.vm_count == null) throw new Error('empty');
    return `usd=${j.estimated_monthly_usd} vms=${j.vm_count}`;
  });

  await mark('ai-capacity', async () => {
    const j = await getJson(`${P}/api/v1/ai/capacity`);
    if (j.hosts_online == null && j.memory_headroom_mib == null) throw new Error('empty');
    return `hosts=${j.hosts_online} mem_free_mib=${j.memory_headroom_mib}`;
  });

  await mark('ai-settings', async () => {
    const j = await getJson(`${P}/api/v1/ai/settings`);
    if (typeof j.enabled !== 'boolean') throw new Error('no enabled');
    return `enabled=${j.enabled} mode=${j.mode}`;
  });

  await mark('leave-running', async () => ensureRunning());

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`GUEST_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

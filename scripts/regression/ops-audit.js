#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Platform audit/templates + Zeus simulate/compliance/connectivity + terminal session.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-audit');
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
    log.append({ kind: 'AUDIT', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'AUDIT', api: name, ok: false, note: e.message.slice(0, 220) });
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
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureRunning());

  await mark('platform-audit', async () => {
    const j = await getJson(`${P}/api/v1/audit?limit=5`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty audit');
    return `count=${j.length} action=${j[0].action || '?'}`;
  });

  await mark('daemon-audit', async () => {
    const j = await getJson('/api/v1/audit');
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('platform-templates', async () => {
    const j = await getJson(`${P}/api/v1/templates`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no templates');
    return `count=${j.length} first=${j[0].name || '?'}`;
  });

  await mark('platform-users', async () => {
    const j = await getJson(`${P}/api/v1/users`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no users');
    return `count=${j.length}`;
  });

  await mark('platform-projects', async () => {
    const j = await getJson(`${P}/api/v1/projects`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('tasks-completed', async () => {
    const j = await getJson(`${P}/api/v1/tasks?status=completed&limit=3`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('events-recent', async () => {
    const j = await getJson(`${P}/api/v1/events?limit=3`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `kind=${j[0].kind || '?'}`;
  });

  await mark('libvirt-details', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    if (j.name !== VM && j.state !== 'running') throw new Error(JSON.stringify(j).slice(0, 80));
    return `${j.name} vcpus=${j.vcpus}`;
  });

  await mark('parity-summary', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt?action=parity.summary`);
    return `state=${j.state} spice=${j.spice}`;
  });

  await mark('consolehub-sessions', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/consolehub/sessions`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('autostart-toggle', async () => {
    let r = await api('POST', `/api/v1/vms/${VM}/autostart/false`);
    if (!ok(r.status)) throw new Error(`false ${r.status}`);
    r = await api('POST', `/api/v1/vms/${VM}/autostart/true`);
    if (!ok(r.status)) throw new Error(`true ${r.status}`);
    return 'false→true';
  });

  await mark('terminal-session-invalid', async () => {
    const r = await api('POST', '/api/v1/terminal/sessions', {});
    if (r.status !== 400) throw new Error(`expected 400 got ${r.status}`);
    return '400 missing host';
  });

  await mark('terminal-session-create', async () => {
    const r = await api('POST', '/api/v1/terminal/sessions', {
      host: '127.0.0.1',
      cols: 80,
      rows: 24,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.session_id) throw new Error('no session_id');
    return `ttl=${j.expires_in_secs}`;
  });

  await mark('zeus-simulate', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/simulate`, {
      target_id: HID,
      profile: 'ProductionServer',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    const entries = j.diff?.entries || j.entries || [];
    return `entries=${entries.length}`;
  });

  await mark('zeus-connectivity', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/connectivity`, {
      target_id: HID,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `allows=${(j.allows || []).length} blocks=${(j.blocks || []).length}`;
  });

  await mark('zeus-compliance-cis', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/compliance/cis`);
    if (j.report !== 'cis') throw new Error('not cis');
    return `scanned=${j.machines_scanned} critical=${j.critical}`;
  });

  await mark('zeus-compliance-pci', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/compliance/pci`);
    if (j.report !== 'pci') throw new Error('not pci');
    return `scanned=${j.machines_scanned} critical=${j.critical}`;
  });

  await mark('zeus-guest-ports', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/vms/${PID}/guest-ports`);
    return `agent=${j.agent_reachable} ports=${(j.ports || []).length}`;
  });

  await mark('siem-export', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/siem/export`);
    if (!Array.isArray(j.events)) throw new Error('no events');
    return `events=${j.event_count ?? j.events.length}`;
  });

  await mark('notifications', async () => {
    const j = await getJson(`${P}/api/v1/notifications`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('leave-running', async () => {
    const state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state !== 'running') throw new Error(state);
    return state;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`AUDIT_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

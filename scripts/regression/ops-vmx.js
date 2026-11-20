#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * VM extras: metrics/timeline/topology/console/ws-token/guest-ips/migrate precheck.
 * Never POSTs /migrate (starts real tasks even for missing dest hosts).
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-vmx');
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
    log.append({ kind: 'VMX', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'VMX', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
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

  await mark('vm-metrics', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/metrics`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `cpu=${j.cpu_percent} mem=${j.memory_used_mib}`;
  });

  await mark('vm-timeline', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/timeline`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `events=${j.length}`;
  });

  await mark('vm-topology', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/topology`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `nodes=${(j.nodes || []).length}`;
  });

  await mark('vm-migrations-list', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/migrations`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('vm-console', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/console`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.console_type && !j.ws_path) throw new Error('empty');
    return `type=${j.console_type}`;
  });

  await mark('vm-ws-token', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/ws-token`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.token) throw new Error('no token');
    return 'issued';
  });

  await mark('guest-ips-batch', async () => {
    const r = await api('POST', `${P}/api/v1/vms/guest-ips/batch`, { vm_ids: [PID] });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.items || !j.items[PID]) throw new Error('missing item');
    return `guest=${j.items[PID].guest_ip} nic=${j.items[PID].nic_ip}`;
  });

  await mark('pending-config-batch', async () => {
    const r = await api('POST', `${P}/api/v1/vms/pending-config/batch`, { vm_ids: [PID] });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    const row = j.items && j.items[PID];
    if (!row) throw new Error('missing');
    return `needs_shutdown=${row.needs_shutdown} state=${row.state}`;
  });

  await mark('migrate-precheck-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/migrate/precheck`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('migrate-precheck-same-host', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/migrate/precheck`, { dest_host_id: HID });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ok=${j.ok} checks=${(j.checks || []).length}`;
  });

  await mark('migrate-post-skipped', async () => {
    // POST /migrate enqueues vm.migrate even for missing dest hosts — never call in regression.
    return 'skipped-unsafe-enqueues-task';
  });

  await mark('migrations-list', async () => {
    const r = await api('GET', `${P}/api/v1/migrations`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('migration-advisor-negative', async () => {
    const r = await api('GET', `${P}/api/v1/migrations/advisor`);
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`VMX_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

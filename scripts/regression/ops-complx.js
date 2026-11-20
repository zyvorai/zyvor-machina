#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Zeus firewall compliance + enforcement soft-paths + platform rename/clone/snapshot schema gates.
 * Avoids: real rename/clone/snapshot create, cordon live host, enforcement apply/delete.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-complx');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
const P = '/api/v1/platform/controller';
const FAKE = '00000000-0000-0000-0000-000000000001';

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'COMPLX', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'COMPLX', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('firewall-compliance-summary', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-firewall/compliance/summary`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `scanned=${j.machines_scanned} critical=${j.critical}`;
  });

  await mark('firewall-compliance-host', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-firewall/compliance/${HID}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `report=${String(j.report).slice(0, 8)} critical=${j.critical}`;
  });

  await mark('enforcement-status', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-security/enforcement/status`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    JSON.parse(r.body);
    return 'ok';
  });

  await mark('host-enforcement', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-security/hosts/${HID}/enforcement`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    JSON.parse(r.body);
    return 'ok';
  });

  await mark('enforcement-policies', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-security/enforcement/policies`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `policies=${(j.policies || []).length} mode=${j.api_mode}`;
  });

  await mark('enforcement-create-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/enforcement/policies`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('enforcement-apply-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/enforcement/policies/${FAKE}/apply`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('enforcement-document-missing-404', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-security/enforcement/policies/${FAKE}/document`);
    if (r.status !== 404 || isHtml(r.body)) throw new Error(`expected 404 got ${r.status}`);
    JSON.parse(r.body);
    return '404';
  });

  await mark('enforcement-attach-soft', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/enforcement/attach`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ok=${j.ok} note=${String(j.note || '').slice(0, 50)}`;
  });

  await mark('enforcement-sync-soft', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/enforcement/sync`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ok=${j.ok}`;
  });

  await mark('enforcement-detach-soft', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/enforcement/detach`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ok=${j.ok}`;
  });

  await mark('vm-rename-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/rename`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('vm-clone-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/clone`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('vm-snapshot-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/snapshots`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('vm-snapshots-list', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/snapshots`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('host-cordon-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${FAKE}/cordon`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('host-cordon-missing-negative', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${FAKE}/cordon`, { cordon: true });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('cluster-settings', async () => {
    const r = await api('GET', `${P}/api/v1/cluster/settings`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ha=${j.ha_enabled} drs=${j.drs_auto_migrate}`;
  });

  await mark('ha-status', async () => {
    const r = await api('GET', `${P}/api/v1/ha/status`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `enabled_vms=${j.status && j.status.enabled_vms}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `/api/v1/vms/${cfg.vmName}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.state !== 'running') throw new Error(`state=${j.state}`);
    return `state=${j.state}`;
  });

  await mark('postcheck-host-schedulable', async () => {
    const r = await api('GET', `${P}/api/v1/hosts/${HID}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.schedulable === false) throw new Error('host not schedulable');
    return `schedulable=${j.schedulable} maintenance=${j.maintenance_mode}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`COMPLX_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

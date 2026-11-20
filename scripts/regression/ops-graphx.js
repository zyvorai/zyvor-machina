#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * AI graph object/diagnose + Zeus firewall approval/baremetal negatives + storage tier gates.
 * Avoids: ai/actions execute, guest-tools install, real pool activate, events/stream.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-graphx');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
let POOL = process.env.MACHINA_POOL_ID || process.env.MACHINA_STORAGE_POOL_ID || '';
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
    log.append({ kind: 'GRAPHX', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'GRAPHX', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

(async () => {
  await login({ retries: 5, waitMs: 65000 });
  const _ids = await resolveIds(api, cfg);
  if (_ids.hostId) HID = _ids.hostId;
  if (_ids.platformVmId) PID = _ids.platformVmId;
  if (_ids.storagePoolId) POOL = _ids.storagePoolId;  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ai-graph', async () => {
    const r = await api('GET', `${P}/api/v1/ai/graph`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `nodes=${(j.nodes || []).length}`;
  });

  await mark('ai-graph-object-vm', async () => {
    const r = await api('GET', `${P}/api/v1/ai/graph/object/vm/${PID}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.kind !== 'vm') throw new Error(`kind=${j.kind}`);
    return `name=${j.name}`;
  });

  await mark('ai-graph-object-host', async () => {
    const r = await api('GET', `${P}/api/v1/ai/graph/object/host/${HID}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.kind !== 'host') throw new Error(`kind=${j.kind}`);
    return `name=${j.name}`;
  });

  await mark('ai-fleet-diagnose', async () => {
    const r = await api('POST', `${P}/api/v1/ai/fleet/diagnose`, { query: 'why is cpu high?' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary) throw new Error('empty summary');
    return String(j.summary).slice(0, 60);
  });

  await mark('ai-fleet-diagnose-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/ai/fleet/diagnose`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('ai-actions-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/ai/actions`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('ai-actions-reject-negative', async () => {
    const r = await api('POST', `${P}/api/v1/ai/actions/${FAKE}/reject`, {});
    if (r.status >= 400) return `${r.status}`;
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.rejected === true) throw new Error('unexpected rejected=true');
    return `rejected=${j.rejected}`;
  });

  await mark('ai-memory-delete-schema-negative', async () => {
    const r = await api('DELETE', `${P}/api/v1/ai/memory`);
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('domain-xml', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/domain-xml`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.xml || !j.xml.includes('<domain')) throw new Error('no domain xml');
    return `bytes=${j.xml.length}`;
  });

  await mark('pending-config', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/pending-config`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `needs_shutdown=${j.needs_shutdown} changes=${(j.pending_changes || []).length}`;
  });

  await mark('zeus-firewall-approvals', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-firewall/approvals`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('zeus-approval-approve-negative', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/approvals/${FAKE}/approve`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('zeus-approval-reject-negative', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/approvals/${FAKE}/reject`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('zeus-baremetal-scan-negative', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/baremetal/${FAKE}/scan`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('zeus-baremetal-temporary-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/baremetal/${FAKE}/temporary`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('storage-pool-volumes', async () => {
    const r = await api('GET', `${P}/api/v1/storage/pools/${POOL}/volumes`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    const vols = j.volumes || j;
    if (!Array.isArray(vols)) throw new Error('not array');
    return `pool=${j.pool || '?'} volumes=${vols.length}`;
  });

  await mark('storage-tier-invalid-negative', async () => {
    const r = await api('POST', `${P}/api/v1/storage/pools/${POOL}/tier/gold`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('storage-volume-delete-negative', async () => {
    const r = await api('DELETE', `${P}/api/v1/storage/pools/${FAKE}/volumes/${FAKE}`);
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('fleet-finder-query', async () => {
    const r = await api('GET', `${P}/api/v1/fleet/finder?q=chrome`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary) throw new Error('no summary');
    return String(j.summary).slice(0, 60);
  });

  await mark('launchpad-catalog-soft', async () => {
    const r = await api('GET', `${P}/api/v1/launchpad/catalog`);
    if (r.status === 503) {
      const j = JSON.parse(r.body);
      if (j.error_code !== 'launchpad_unavailable') throw new Error(j.error_code || r.body.slice(0, 80));
      return 'launchpad_unavailable';
    }
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return 'ok';
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `/api/v1/vms/${cfg.vmName}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.state !== 'running') throw new Error(`state=${j.state}`);
    return `state=${j.state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`GRAPHX_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

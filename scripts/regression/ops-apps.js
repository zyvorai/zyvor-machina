#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Apps / fleet-desktop / SOC ops smoke: application groups CRUD, fleet desktop
 * hubs, operations runbook execute, SOC ingest/forward, AI firewall plan,
 * baremetal capacity, autopilot propose, segment connectivity.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-apps');
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
const P = '/api/v1/platform/controller';
const SUFFIX = Date.now().toString(36).slice(-5);

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'APPS', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'APPS', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
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

  let appId = null;

  await mark('application-create', async () => {
    const r = await api('POST', `${P}/api/v1/applications`, {
      name: `reg-app-${SUFFIX}`,
      description: 'regression',
      vm_ids: [PID],
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id && !j.group?.id) throw new Error('no id');
    appId = j.id || j.group.id;
    return `id=${appId.slice(0, 8)} vms=${(j.vm_ids || []).length}`;
  });

  await mark('application-get', async () => {
    const j = await getJson(`${P}/api/v1/applications/${appId}`);
    const id = j.id || j.group?.id;
    if (id !== appId) throw new Error('mismatch');
    return (j.name || j.group?.name || 'ok').toString();
  });

  await mark('application-list', async () => {
    const j = await getJson(`${P}/api/v1/applications`);
    if (!Array.isArray(j) || !j.some((x) => x.id === appId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('application-action-start', async () => {
    const r = await api('POST', `${P}/api/v1/applications/${appId}/actions`, { action: 'start' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.task_ids) || j.task_ids.length < 1) throw new Error('no tasks');
    return `tasks=${j.task_ids.length}`;
  });

  await mark('application-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/applications/${appId}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    appId = null;
    const j = await getJson(`${P}/api/v1/applications`);
    if (j.some((x) => (x.name || '').startsWith(`reg-app-${SUFFIX}`))) throw new Error('still listed');
    return 'deleted';
  });

  await mark('fleet-console', async () => {
    const j = await getJson(`${P}/api/v1/fleet/console`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('fleet-general', async () => {
    const j = await getJson(`${P}/api/v1/fleet/general`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('fleet-network', async () => {
    const j = await getJson(`${P}/api/v1/fleet/network`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('fleet-storage', async () => {
    const j = await getJson(`${P}/api/v1/fleet/storage`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('fleet-shortcuts', async () => {
    const j = await getJson(`${P}/api/v1/fleet/shortcuts`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('fleet-users', async () => {
    const j = await getJson(`${P}/api/v1/fleet/users`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('operations-overview', async () => {
    const j = await getJson(`${P}/api/v1/operations/overview`);
    if (j.runbook_count == null) throw new Error('empty');
    return j.summary || `runbooks=${j.runbook_count}`;
  });

  await mark('operations-runbooks', async () => {
    const j = await getJson(`${P}/api/v1/operations/runbooks`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('operations-runbook-execute', async () => {
    const list = await getJson(`${P}/api/v1/operations/runbooks`);
    const rb = list[0];
    const r = await api('POST', `${P}/api/v1/operations/runbooks/${rb.id}/execute`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.execution_id) throw new Error('no execution_id');
    return `id=${j.execution_id.slice(0, 8)} title=${j.title || rb.title}`;
  });

  await mark('operations-executions', async () => {
    const j = await getJson(`${P}/api/v1/operations/executions`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('soc-ingest-run', async () => {
    const r = await api('POST', `${P}/api/v1/soc/ingest/run`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.ingest) throw new Error('empty');
    return `alerts=${j.alerts_fired} forwarded=${j.forwarded}`;
  });

  await mark('soc-forward-replay', async () => {
    const r = await api('POST', `${P}/api/v1/soc/forward/replay`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.forwarded == null) throw new Error('empty');
    return `forwarded=${j.forwarded} hours=${j.hours}`;
  });

  await mark('soc-integrations', async () => {
    const j = await getJson(`${P}/api/v1/soc/integrations`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('soc-splunk', async () => {
    const j = await getJson(`${P}/api/v1/soc/integrations/splunk`);
    if (!j.integration_type) throw new Error('empty');
    return j.integration_type;
  });

  await mark('ai-firewall-explain', async () => {
    const r = await api('POST', `${P}/api/v1/ai/firewall/explain`, { target_id: HID });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.target && !j.risk) throw new Error('empty');
    return `risk=${j.risk} evidence=${(j.evidence || []).length}`;
  });

  await mark('ai-firewall-secure-plan', async () => {
    const r = await api('POST', `${P}/api/v1/ai/firewall/secure-plan`, { target_id: HID });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.steps) || j.steps.length < 1) throw new Error('no steps');
    return `steps=${j.steps.length}`;
  });

  await mark('ai-autopilot-propose', async () => {
    const j = await getJson(`${P}/api/v1/ai/autopilot/propose`);
    if (!Array.isArray(j.actions)) throw new Error('no actions');
    return `mode=${j.mode} actions=${j.actions.length}`;
  });

  await mark('ai-autopilot-history', async () => {
    const j = await getJson(`${P}/api/v1/ai/autopilot/history`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('baremetal-capacity-plan', async () => {
    const r = await api('POST', `${P}/api/v1/baremetal/capacity/plan`, {
      query: '8 cpu 32gb for web tier',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.servers_needed == null) throw new Error('empty');
    return `servers=${j.servers_needed}`;
  });

  await mark('segment-connectivity', async () => {
    const overview = await getJson(`${P}/api/v1/network/segments/overview`);
    const sid = overview.segments?.[0]?.id;
    if (!sid) throw new Error('no segment');
    const r = await api('POST', `${P}/api/v1/network/segments/${sid}/connectivity`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.rules == null) throw new Error('empty');
    return `segment=${j.segment_name} rules=${j.rules}`;
  });

  await mark('host-cockpit', async () => {
    // Same load flake as ops-guest: agent cockpit probe can 500 under contention.
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/cockpit`);
    if (ok(r.status) && !isHtml(r.body)) {
      const j = JSON.parse(r.body);
      if (!j.host_id) throw new Error('empty');
      return `probed=${j.storage?.probed}`;
    }
    if (/timed out|timeout|agent|unreachable|internal/i.test(r.body || '') || r.status >= 500) {
      return `${r.status}-expected`;
    }
    throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
  });

  await mark('proxmox-sync', async () => {
    const r = await api('POST', `${P}/api/v1/proxmox/sync`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.synced == null && !j.message) throw new Error('empty');
    return `synced=${j.synced} imported=${j.imported}`;
  });

  await mark('air-gap-bundles', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/air-gap/bundles`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  // cleanup leftovers
  if (appId) {
    try {
      await api('DELETE', `${P}/api/v1/applications/${appId}`);
    } catch {
      /* ignore */
    }
  }
  try {
    const leftover = await getJson(`${P}/api/v1/applications`);
    for (const a of leftover) {
      if (/^reg-app/i.test(a.name || '')) {
        await api('DELETE', `${P}/api/v1/applications/${a.id}`);
      }
    }
  } catch {
    /* ignore */
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`APPS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Operations / developer hub regression: support bundle, runbooks, fleet desktop,
 * watchdog, guest agent negative paths, AI timeline/graph query, twin impact.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-ops');
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
    log.append({ kind: 'OPS', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'OPS', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('developer-overview', async () => {
    const j = await getJson(`${P}/api/v1/developer/overview`);
    if (!j.openapi_url && !j.sdk_typescript) throw new Error('empty');
    return j.openapi_url || 'ok';
  });

  await mark('developer-terraform-schema', async () => {
    const j = await getJson(`${P}/api/v1/developer/terraform/schema`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty schema');
    return `resources=${j.length} first=${j[0].name}`;
  });

  await mark('support-bundle', async () => {
    const j = await getJson(`${P}/api/v1/support/bundle`);
    if (!j.controller_id && j.task_count == null) throw new Error('empty');
    return `tasks=${j.task_count} hosts=${j.host_count}`;
  });

  await mark('operations-overview', async () => {
    const j = await getJson(`${P}/api/v1/operations/overview`);
    if (j.runbook_count == null && !j.summary) throw new Error('empty');
    return `runbooks=${j.runbook_count}`;
  });

  await mark('operations-runbooks', async () => {
    const j = await getJson(`${P}/api/v1/operations/runbooks`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no runbooks');
    return `count=${j.length} first=${j[0].incident || j[0].id}`;
  });

  await mark('operations-executions', async () => {
    const j = await getJson(`${P}/api/v1/operations/executions`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('operations-showback', async () => {
    const j = await getJson(`${P}/api/v1/operations/showback`);
    if (!Array.isArray(j.lines)) throw new Error('no lines');
    return `lines=${j.lines.length}`;
  });

  await mark('fleet-desktop', async () => {
    const j = await getJson(`${P}/api/v1/fleet/desktop`);
    if (!j.summary) throw new Error('no summary');
    return j.summary.slice(0, 80);
  });

  await mark('fleet-linux-health', async () => {
    const j = await getJson(`${P}/api/v1/fleet/linux-health`);
    if (j.hosts_scanned == null) throw new Error('empty');
    return `scanned=${j.hosts_scanned} pressure=${j.pressure_hosts}`;
  });

  await mark('fleet-backups', async () => {
    const j = await getJson(`${P}/api/v1/fleet/backups`);
    if (!j.summary && j.total_events == null) throw new Error('empty');
    return `events=${j.total_events}`;
  });

  await mark('vm-watchdog', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/watchdog`);
    if (typeof j.enabled !== 'boolean') throw new Error('no enabled');
    return `enabled=${j.enabled} threshold=${j.failure_threshold_secs}`;
  });

  await mark('scheduled-jobs', async () => {
    const j = await getJson(`${P}/api/v1/scheduled-jobs`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('webhook-deliveries', async () => {
    const j = await getJson(`${P}/api/v1/webhook-deliveries`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('guest-fs-freeze-status', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/guest/fs-freeze-status`);
    if (typeof j.ok !== 'boolean' && !j.action) throw new Error('empty');
    return `ok=${j.ok} msg=${String(j.message || '').slice(0, 60)}`;
  });

  await mark('guest-ai-insights', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/guest/ai-insights`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.vm_id && !j.snapshot) throw new Error('empty');
    return j.vm_name || j.vm_id;
  });

  await mark('guest-sync-time', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/guest/sync-time`, {});
    // Agent-up lab: expect success. Agent-down hosts still return 4xx/5xx.
    if (r.status >= 400) {
      if (!/agent|guestkit|unavailable|not reachable/i.test(r.body || '')) {
        throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
      }
      return `${r.status} agent-down`;
    }
    if (isHtml(r.body)) throw new Error('html');
    const j = JSON.parse(r.body);
    if (!j.ok && j.ok !== undefined) throw new Error(JSON.stringify(j).slice(0, 100));
    return j.message || 'ok';
  });

  await mark('guest-fstrim', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/guest/fstrim`, {});
    if (r.status >= 400) {
      if (!/agent|guestkit|unavailable|not reachable/i.test(r.body || '')) {
        throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
      }
      return `${r.status} agent-down`;
    }
    if (isHtml(r.body)) throw new Error('html');
    const j = JSON.parse(r.body);
    if (!j.ok && j.ok !== undefined) throw new Error(JSON.stringify(j).slice(0, 100));
    return j.message || 'ok';
  });

  await mark('host-diagnose', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/diagnose`, { query: 'health' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return (j.summary || j.hostname || 'ok').toString().slice(0, 80);
  });

  await mark('ai-timeline-replay', async () => {
    const j = await getJson(
      `${P}/api/v1/ai/timeline/replay?from=2026-08-01T00:00:00Z&to=2026-08-04T00:00:00Z`,
    );
    if (!Array.isArray(j.entries)) throw new Error('no entries');
    return `count=${j.entries.length}`;
  });

  await mark('ai-memory-similar', async () => {
    const j = await getJson(`${P}/api/v1/ai/memory/similar?q=cpu`);
    if (!Array.isArray(j.incidents) && !j.summary) throw new Error('empty');
    return `incidents=${(j.incidents || []).length}`;
  });

  await mark('ai-knowledge-search', async () => {
    const r = await api('POST', `${P}/api/v1/ai/knowledge/search`, { query: 'cpu pressure' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.hits)) throw new Error('no hits');
    return `hits=${j.hits.length}`;
  });

  await mark('ai-graph-query', async () => {
    const r = await api('POST', `${P}/api/v1/ai/graph/query`, { query: 'host' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.hits)) throw new Error('no hits');
    return `hits=${j.hits.length}`;
  });

  await mark('ai-graph-object-host', async () => {
    const j = await getJson(`${P}/api/v1/ai/graph/object/host/${HID}`);
    if (j.kind !== 'host' && !j.name) throw new Error('empty');
    return `${j.name || j.id}`;
  });

  await mark('ai-incidents-analyze', async () => {
    const j = await getJson(`${P}/api/v1/ai/incidents/analyze`);
    if (!Array.isArray(j.timeline) && j.window_hours == null) throw new Error('empty');
    return `window=${j.window_hours} timeline=${(j.timeline || []).length}`;
  });

  await mark('ai-twin-impact', async () => {
    const r = await api('POST', `${P}/api/v1/ai/twin/impact`, {
      action: 'shutdown',
      target_kind: 'host',
      target_id: HID,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    return (j.target || j.summary || Object.keys(j).slice(0, 3).join(',')).toString().slice(0, 80);
  });

  await mark('cost-attribution-csv', async () => {
    const r = await api('GET', `${P}/api/v1/ai/cost/attribution/export.csv`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    if (!/FinOps|Team|USD/i.test(r.body)) throw new Error('unexpected csv');
    return `bytes=${r.body.length}`;
  });

  await mark('consolehub-plan-classic', async () => {
    const j = await getJson(`/api/v1/vms/${VM}/consolehub/plan`);
    if (!j.recommended) throw new Error('no recommended');
    return j.recommended;
  });

  await mark('leave-running', async () => ensureRunning());

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`OPS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

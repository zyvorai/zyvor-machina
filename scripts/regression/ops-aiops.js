#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * AI ops: jarvis/heatmap/mission/intent/rebalance/autopilot.
 * Never calls mission/stack/execute or intent execute — plan/propose only.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-aiops');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
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
    log.append({ kind: 'AIOPS', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'AIOPS', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

(async () => {
  await login({ retries: 5, waitMs: 65000 });
  const _ids = await resolveIds(api, cfg);
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('jarvis-landing', async () => {
    const r = await api('GET', `${P}/api/v1/ai/jarvis/landing`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `intents=${(j.intents || []).length}`;
  });

  await mark('fleet-heatmap', async () => {
    const r = await api('GET', `${P}/api/v1/ai/fleet/heatmap`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `hosts=${(j.hosts || []).length}`;
  });

  await mark('mission-stack-status', async () => {
    const r = await api('GET', `${P}/api/v1/ai/mission/stack/status`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `total=${j.total_stack_vms} running=${j.running}`;
  });

  await mark('mission-stack-plan', async () => {
    const r = await api('POST', `${P}/api/v1/ai/mission/stack`, { query: 'list running vms' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.label && !j.review) throw new Error('empty');
    return String(j.label || j.review).slice(0, 60);
  });

  await mark('mission-stack-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/ai/mission/stack`, { goal: 'x' });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('intent-environment-plan', async () => {
    const r = await api('POST', `${P}/api/v1/ai/intent/environment`, { query: 'describe fleet' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.label && !j.review) throw new Error('empty');
    return String(j.label || '').slice(0, 60);
  });

  await mark('rebalance-propose', async () => {
    const r = await api('GET', `${P}/api/v1/ai/fleet/rebalance/propose`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `moves=${(j.moves || []).length}`;
  });

  await mark('rebalance-execute-dry', async () => {
    const r = await api('POST', `${P}/api/v1/ai/fleet/rebalance/execute`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.dry_run !== true && (j.task_ids || []).length > 0) {
      throw new Error('unexpected live rebalance tasks');
    }
    return `dry_run=${j.dry_run} tasks=${(j.task_ids || []).length}`;
  });

  await mark('autopilot-history', async () => {
    const r = await api('GET', `${P}/api/v1/ai/autopilot/history`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`AIOPS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

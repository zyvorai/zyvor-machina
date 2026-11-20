#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Zeus AI engine reads: twin/graph, incidents, compliance, security,
 * remediate/actions hubs, cost budget/attribution, prompts, routing.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-ai');
const VM = cfg.vmName;
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
    log.append({ kind: 'AI', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'AI', api: name, ok: false, note: e.message.slice(0, 220) });
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
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('ensure-running', async () => ensureRunning());

  await mark('ai-twin-graph', async () => {
    const j = await getJson(`${P}/api/v1/ai/twin/graph`);
    if (!Array.isArray(j.nodes) || j.nodes.length < 1) throw new Error('no nodes');
    return `nodes=${j.nodes.length}`;
  });

  await mark('ai-infra-graph', async () => {
    const j = await getJson(`${P}/api/v1/ai/graph`);
    if (!Array.isArray(j.nodes) || j.nodes.length < 1) throw new Error('no nodes');
    return `nodes=${j.nodes.length}`;
  });

  await mark('ai-incidents-active', async () => {
    const j = await getJson(`${P}/api/v1/ai/incidents/active`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}${j[0] ? ` first=${j[0].title}` : ''}`;
  });

  await mark('ai-predictions', async () => {
    const j = await getJson(`${P}/api/v1/ai/predictions`);
    if (!Array.isArray(j.predictions)) throw new Error('no predictions');
    return `count=${j.predictions.length}`;
  });

  await mark('ai-sre-forecast', async () => {
    const j = await getJson(`${P}/api/v1/ai/sre/forecast`);
    if (!Array.isArray(j.forecasts)) throw new Error('no forecasts');
    return `count=${j.forecasts.length}`;
  });

  await mark('ai-compliance-frameworks', async () => {
    const j = await getJson(`${P}/api/v1/ai/compliance/frameworks`);
    if (!Array.isArray(j.frameworks) || j.frameworks.length < 1) throw new Error('no frameworks');
    return `count=${j.frameworks.length} first=${j.frameworks[0].framework}`;
  });

  await mark('ai-security-graph', async () => {
    const j = await getJson(`${P}/api/v1/ai/security/graph`);
    if (!Array.isArray(j.nodes) || j.nodes.length < 1) throw new Error('no nodes');
    return `nodes=${j.nodes.length}`;
  });

  await mark('ai-services-graph', async () => {
    const j = await getJson(`${P}/api/v1/ai/services/graph`);
    if (!Array.isArray(j.nodes) || j.nodes.length < 1) throw new Error('no nodes');
    return `nodes=${j.nodes.length}`;
  });

  await mark('ai-memory-incidents', async () => {
    const j = await getJson(`${P}/api/v1/ai/memory/incidents`);
    if (!Array.isArray(j.incidents)) throw new Error('no incidents');
    return `count=${j.incidents.length}`;
  });

  await mark('ai-remediate-hub', async () => {
    const j = await getJson(`${P}/api/v1/ai/remediate/hub`);
    if (!Array.isArray(j.items)) throw new Error('no items');
    return `count=${j.items.length}`;
  });

  await mark('ai-actions-hub', async () => {
    const j = await getJson(`${P}/api/v1/ai/actions/hub`);
    if (!Array.isArray(j.zeus_actions) && j.autopilot_proposals == null) throw new Error('empty hub');
    const props = j.autopilot_proposals || [];
    return `actions=${(j.zeus_actions || []).length} proposals=${props.length}`;
  });

  await mark('ai-zeus-summary', async () => {
    const r = await api('GET', `${P}/api/v1/ai/zeus/summary`);
    if (r.status === 404) return 'skipped (route absent)';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.status && !j.tagline) throw new Error('empty');
    return `status=${j.status} hosts=${j.hosts_online}`;
  });

  await mark('ai-fleet-power-optimize', async () => {
    const j = await getJson(`${P}/api/v1/ai/fleet/power/optimize`);
    if (!Array.isArray(j.optimizations) && !j.summary) throw new Error('empty');
    return `opts=${(j.optimizations || []).length}`;
  });

  await mark('ai-fleet-rebalance-propose', async () => {
    const j = await getJson(`${P}/api/v1/ai/fleet/rebalance/propose`);
    if (!Array.isArray(j.moves) && !j.summary) throw new Error('empty');
    return `moves=${(j.moves || []).length}`;
  });

  await mark('ai-cost-budget', async () => {
    const j = await getJson(`${P}/api/v1/ai/cost/budget`);
    if (j.monthly_budget_usd == null) throw new Error('no budget');
    return `budget=${j.monthly_budget_usd} spend=${j.current_spend_usd}`;
  });

  await mark('ai-cost-attribution', async () => {
    const j = await getJson(`${P}/api/v1/ai/cost/attribution`);
    if (j.total_monthly_usd == null && !Array.isArray(j.teams)) throw new Error('empty');
    return `total=${j.total_monthly_usd} teams=${(j.teams || []).length}`;
  });

  await mark('ai-prompts', async () => {
    const j = await getJson(`${P}/api/v1/ai/prompts`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('ai-routing-rules', async () => {
    const j = await getJson(`${P}/api/v1/ai/routing/rules`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no rules');
    return `count=${j.length}`;
  });

  await mark('ai-marketplace-agents', async () => {
    const j = await getJson(`${P}/api/v1/ai/marketplace/agents`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('no agents');
    return `count=${j.length} first=${j[0].slug || j[0].name}`;
  });

  await mark('ai-memory-settings', async () => {
    const j = await getJson(`${P}/api/v1/ai/memory/settings`);
    if (typeof j.enabled !== 'boolean') throw new Error('no enabled');
    return `enabled=${j.enabled} retention=${j.retention_days}`;
  });

  await mark('leave-running', async () => ensureRunning());

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`AI_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

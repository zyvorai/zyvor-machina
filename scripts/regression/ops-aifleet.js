#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * AI fleet / Zeus summary / security / agents / copilot / cost / knowledge.
 * Avoids stream endpoints and mission/intent execute.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-aifleet');
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
    log.append({ kind: 'AIFLEET', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'AIFLEET', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('zeus-summary', async () => {
    const r = await api('GET', `${P}/api/v1/ai/zeus/summary`);
    if (r.status === 404) return 'skipped (route absent)';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `status=${j.status} hosts=${j.hosts_online}`;
  });

  await mark('ai-security', async () => {
    const r = await api('GET', `${P}/api/v1/ai/security`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `risk=${j.risk_level} findings=${(j.findings || []).length}`;
  });

  await mark('ai-agents', async () => {
    const r = await api('GET', `${P}/api/v1/ai/agents`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('cost-attribution', async () => {
    const r = await api('GET', `${P}/api/v1/ai/cost/attribution`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `usd=${j.total_monthly_usd} teams=${(j.teams || []).length}`;
  });

  await mark('ai-cost', async () => {
    const r = await api('GET', `${P}/api/v1/ai/cost`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `usd=${j.estimated_monthly_usd} vms=${j.vm_count}`;
  });

  await mark('ai-capacity', async () => {
    const r = await api('GET', `${P}/api/v1/ai/capacity`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `hosts=${j.hosts_online} headroom=${j.memory_headroom_mib}`;
  });

  await mark('services-graph', async () => {
    const r = await api('GET', `${P}/api/v1/ai/services/graph`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `nodes=${(j.nodes || []).length}`;
  });

  await mark('memory-incidents', async () => {
    const r = await api('GET', `${P}/api/v1/ai/memory/incidents`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `incidents=${(j.incidents || []).length}`;
  });

  await mark('copilot-chat', async () => {
    const r = await api('POST', `${P}/api/v1/ai/copilot/chat`, { message: 'status' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.reply) throw new Error('empty reply');
    return String(j.reply).slice(0, 60);
  });

  await mark('copilot-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/ai/copilot/chat`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('knowledge-search', async () => {
    const r = await api('POST', `${P}/api/v1/ai/knowledge/search`, { query: 'firewall' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `hits=${(j.hits || []).length}`;
  });

  await mark('terminal-suggest', async () => {
    const r = await api('POST', `${P}/api/v1/ai/terminal/suggest`, { partial: 'virsh list' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `suggestions=${(j.suggestions || []).length}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`AIFLEET_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

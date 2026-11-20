#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Hub / network / webhook / users smoke: network activate cycle, webhook
 * toggle, template approval, user CRUD, HA/leadership, operations showback,
 * IPAM, AI budget/routing/memory, install.sh agent bootstrap.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-hub');
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
const P = '/api/v1/platform/controller';
const SUFFIX = Date.now().toString(36).slice(-5);

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}
function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'HUB', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'HUB', api: name, ok: false, note: e.message.slice(0, 220) });
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
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  let netId = null;
  let webhookId = null;
  let userId = null;

  await mark('networks-list', async () => {
    const j = await getJson(`${P}/api/v1/networks`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('network-lifecycle', async () => {
    const name = `reg-net-${SUFFIX}`;
    const bridge = `virbr${String(Date.now()).slice(-4)}`;
    let r = await api('POST', `${P}/api/v1/networks`, { name, bridge });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`create ${r.status} ${String(r.body).slice(0, 80)}`);
    const created = JSON.parse(r.body);
    netId = created.id;
    await sleep(1500);
    r = await api('POST', `${P}/api/v1/networks/${netId}/deactivate`, {});
    if (!ok(r.status)) throw new Error(`deactivate ${r.status} ${String(r.body).slice(0, 80)}`);
    await sleep(1500);
    r = await api('POST', `${P}/api/v1/networks/${netId}/activate`, {});
    if (!ok(r.status)) throw new Error(`activate ${r.status} ${String(r.body).slice(0, 80)}`);
    r = await api('DELETE', `${P}/api/v1/networks/${netId}`);
    if (!ok(r.status)) throw new Error(`delete ${r.status}`);
    netId = null;
    return `${name} cycle`;
  });

  await mark('webhook-toggle-roundtrip', async () => {
    let r = await api('POST', `${P}/api/v1/webhooks`, {
      url: 'https://example.com/machina-hub-hook',
      events: ['vm.power'],
      enabled: true,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`create ${r.status}`);
    const j = JSON.parse(r.body);
    webhookId = j.id;
    r = await api('POST', `${P}/api/v1/webhooks/${webhookId}/toggle`, {});
    if (!ok(r.status)) throw new Error(`toggle1 ${r.status}`);
    let body = JSON.parse(r.body);
    if (body.enabled !== false) throw new Error('expected disabled');
    r = await api('POST', `${P}/api/v1/webhooks/${webhookId}/toggle`, {});
    if (!ok(r.status)) throw new Error(`toggle2 ${r.status}`);
    body = JSON.parse(r.body);
    if (body.enabled !== true) throw new Error('expected enabled');
    r = await api('DELETE', `${P}/api/v1/webhooks/${webhookId}`);
    if (!ok(r.status)) throw new Error(`delete ${r.status}`);
    webhookId = null;
    return 'toggle off/on + delete';
  });

  await mark('webhook-deliveries', async () => {
    const j = await getJson(`${P}/api/v1/webhook-deliveries`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('template-get', async () => {
    const list = await getJson(`${P}/api/v1/templates`);
    const items = Array.isArray(list) ? list : list.items || [];
    if (items.length < 1) throw new Error('empty');
    const name = items[0].name;
    const version = items[0].version || '1.0.0';
    const j = await getJson(
      `${P}/api/v1/templates/${encodeURIComponent(name)}/${encodeURIComponent(version)}`,
    );
    if (j.name !== name) throw new Error('mismatch');
    return `${name}@${version}`;
  });

  await mark('template-approval-patch', async () => {
    const list = await getJson(`${P}/api/v1/templates`);
    const items = Array.isArray(list) ? list : list.items || [];
    const name = items[0].name;
    const version = items[0].version || '1.0.0';
    const path = `${P}/api/v1/templates/${encodeURIComponent(name)}/${encodeURIComponent(version)}/approval`;
    let r = await api('PATCH', path, { approval_status: 'approved' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`approve ${r.status}`);
    r = await api('PATCH', path, { approval_status: 'pending' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`pending ${r.status}`);
    return `${name} approved→pending`;
  });

  await mark('user-roundtrip', async () => {
    let r = await api('POST', `${P}/api/v1/users`, {
      username: `reg-user-${SUFFIX}`,
      role: 'viewer',
      password: 'regpass123',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`create ${r.status} ${String(r.body).slice(0, 80)}`);
    const j = JSON.parse(r.body);
    userId = j.id;
    r = await api('PATCH', `${P}/api/v1/users/${userId}`, { role: 'operator' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`patch ${r.status}`);
    const patched = JSON.parse(r.body);
    if (patched.role !== 'operator') throw new Error('role not patched');
    r = await api('DELETE', `${P}/api/v1/users/${userId}`);
    if (!ok(r.status)) throw new Error(`delete ${r.status}`);
    userId = null;
    return 'viewer→operator→delete';
  });

  await mark('users-list', async () => {
    const j = await getJson(`${P}/api/v1/users`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('ha-status', async () => {
    const j = await getJson(`${P}/api/v1/ha/status`);
    if (!j.status && !Array.isArray(j.events)) throw new Error('empty');
    return `events=${(j.events || []).length}`;
  });

  await mark('cluster-leadership', async () => {
    const j = await getJson(`${P}/api/v1/cluster/leadership`);
    if (!j.controller_id) throw new Error('empty');
    return `leader=${j.is_leader} id=${j.controller_id}`;
  });

  await mark('cluster-settings', async () => {
    const j = await getJson(`${P}/api/v1/cluster/settings`);
    if (!j.placement_policy) throw new Error('empty');
    return `placement=${j.placement_policy} ha=${j.ha_enabled}`;
  });

  await mark('operations-overview', async () => {
    const j = await getJson(`${P}/api/v1/operations/overview`);
    if (j.runbook_count == null) throw new Error('empty');
    return `runbooks=${j.runbook_count} grade=${j.compliance_grade}`;
  });

  await mark('operations-showback', async () => {
    const j = await getJson(`${P}/api/v1/operations/showback`);
    if (!Array.isArray(j.lines)) throw new Error('empty');
    return `lines=${j.lines.length}`;
  });

  await mark('ipam-pools', async () => {
    const j = await getJson(`${P}/api/v1/network/ipam/pools`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('ai-cost-budget', async () => {
    const j = await getJson(`${P}/api/v1/ai/cost/budget`);
    if (j.monthly_budget_usd == null) throw new Error('empty');
    return `budget=${j.monthly_budget_usd} status=${j.status}`;
  });

  await mark('ai-routing-rules', async () => {
    const j = await getJson(`${P}/api/v1/ai/routing/rules`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('ai-memory-settings', async () => {
    const j = await getJson(`${P}/api/v1/ai/memory/settings`);
    if (j.enabled == null) throw new Error('empty');
    return `enabled=${j.enabled} days=${j.retention_days}`;
  });

  await mark('ai-actions-hub', async () => {
    const j = await getJson(`${P}/api/v1/ai/actions/hub`);
    if (!Array.isArray(j.zyra_actions) && !Array.isArray(j.autopilot_proposals)) {
      throw new Error('empty');
    }
    return `actions=${(j.zyra_actions || []).length} proposals=${(j.autopilot_proposals || []).length}`;
  });

  await mark('scheduled-jobs', async () => {
    const j = await getJson(`${P}/api/v1/scheduled-jobs`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('notifications', async () => {
    const j = await getJson(`${P}/api/v1/notifications?limit=5`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('cloud-init-validate', async () => {
    const r = await api('POST', `${P}/api/v1/cloud-init/validate`, {
      user_data: '#cloud-config\npackages: [curl]\n',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.valid !== true) throw new Error(JSON.stringify(j.issues || j).slice(0, 80));
    return 'valid';
  });

  await mark('install-sh', async () => {
    const r = await api('GET', `${P}/install.sh`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!/^#!/m.test(r.body || '') || !/CONTROLLER/.test(r.body || '')) {
      throw new Error('not install script');
    }
    return `bytes=${r.body.length}`;
  });

  await mark('zeus-k8s-export-status', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/k8s/${HID}/export-status`);
    if (!j.cluster_id && !j.namespace) throw new Error('empty');
    return `ns=${j.namespace}`;
  });

  await mark('task-cancel-negative', async () => {
    const list = await getJson(`${P}/api/v1/tasks?limit=10`);
    const done = (list || []).find((t) => t.status === 'completed');
    if (!done) return 'no-completed';
    const r = await api('POST', `${P}/api/v1/tasks/${done.id}/cancel`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('task-retry-negative', async () => {
    const list = await getJson(`${P}/api/v1/tasks?limit=10`);
    const done = (list || []).find((t) => t.status === 'completed');
    if (!done) return 'no-completed';
    const r = await api('POST', `${P}/api/v1/tasks/${done.id}/retry`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  // cleanup
  if (netId) {
    try {
      await api('DELETE', `${P}/api/v1/networks/${netId}`);
    } catch {
      /* ignore */
    }
  }
  if (webhookId) {
    try {
      await api('DELETE', `${P}/api/v1/webhooks/${webhookId}`);
    } catch {
      /* ignore */
    }
  }
  if (userId) {
    try {
      await api('DELETE', `${P}/api/v1/users/${userId}`);
    } catch {
      /* ignore */
    }
  }
  try {
    const leftover = await getJson(`${P}/api/v1/networks`);
    for (const n of leftover) {
      if (/^reg-net/i.test(n.name || '')) await api('DELETE', `${P}/api/v1/networks/${n.id}`);
    }
  } catch {
    /* ignore */
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`HUB_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

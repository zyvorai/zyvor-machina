#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * AI providers / prompts / content / blueprints / placement smoke.
 * Includes content-image delete (create/approve/reject existed without delete).
 */

const { loadConfig } = require('./lib/config');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-providers');
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
    log.append({ kind: 'PROVDR', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'PROVDR', api: name, ok: false, note: e.message.slice(0, 220) });
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
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  let providerId = null;
  let promptId = null;
  let blueprintId = null;
  let imageId = null;
  let imageRejectId = null;

  await mark('providers-list', async () => {
    const j = await getJson(`${P}/api/v1/ai/providers`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('provider-create', async () => {
    const r = await api('POST', `${P}/api/v1/ai/providers`, {
      name: `reg-prov-${SUFFIX}`,
      kind: 'openai_compat',
      base_url: 'http://127.0.0.1:9/v1',
      api_key: 'sk-regression',
      models: [{ model_id: 'llama3', display_name: 'Llama 3', context_window: 8192 }],
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    providerId = j.id;
    return j.id.slice(0, 8);
  });

  await mark('provider-models', async () => {
    const j = await getJson(`${P}/api/v1/ai/providers/${providerId}/models`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `models=${j.length} first=${j[0].model_id}`;
  });

  await mark('provider-test-unreachable', async () => {
    const r = await api('POST', `${P}/api/v1/ai/providers/${providerId}/test`, {});
    // Unreachable local endpoint should fail the probe (not hang the suite).
    if (r.status < 400) throw new Error(`expected fail got ${r.status}`);
    return `${r.status}`;
  });

  await mark('provider-patch', async () => {
    const r = await api('PATCH', `${P}/api/v1/ai/providers/${providerId}`, {
      enabled: false,
      name: `reg-prov-${SUFFIX}-x`,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.name !== `reg-prov-${SUFFIX}-x`) throw new Error('name not patched');
    return `enabled=${j.enabled}`;
  });

  await mark('provider-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/ai/providers/${providerId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    providerId = null;
    return 'deleted';
  });

  await mark('prompts-list', async () => {
    const j = await getJson(`${P}/api/v1/ai/prompts`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('prompt-create', async () => {
    const r = await api('POST', `${P}/api/v1/ai/prompts`, {
      scope: 'org',
      title: `reg-pr-${SUFFIX}`,
      body: 'hello fleet',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    promptId = j.id;
    return j.title;
  });

  await mark('prompt-patch', async () => {
    const r = await api('PATCH', `${P}/api/v1/ai/prompts/${promptId}`, {
      title: `reg-pr-${SUFFIX}-x`,
      body: 'updated',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.body !== 'updated') throw new Error('body not patched');
    return j.title;
  });

  await mark('prompt-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/ai/prompts/${promptId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    promptId = null;
    return 'deleted';
  });

  await mark('blueprints-list', async () => {
    const j = await getJson(`${P}/api/v1/blueprints`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('blueprint-create', async () => {
    const r = await api('POST', `${P}/api/v1/blueprints`, {
      name: `reg-bp-${SUFFIX}`,
      description: 'regression',
      actions: ['start', 'stop'],
      vm_ids: [],
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    blueprintId = j.id;
    return j.name;
  });

  await mark('blueprint-get', async () => {
    const j = await getJson(`${P}/api/v1/blueprints/${blueprintId}`);
    if (j.id !== blueprintId) throw new Error('mismatch');
    return `actions=${(j.actions || []).join(',')}`;
  });

  await mark('blueprint-run-empty', async () => {
    const r = await api('POST', `${P}/api/v1/blueprints/${blueprintId}/run`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.task_ids)) throw new Error('empty');
    return `tasks=${j.task_ids.length}`;
  });

  await mark('blueprint-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/blueprints/${blueprintId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    blueprintId = null;
    return 'deleted';
  });

  await mark('content-list', async () => {
    const j = await getJson(`${P}/api/v1/content/images`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('content-create-approve', async () => {
    const r = await api('POST', `${P}/api/v1/content/images`, {
      name: `reg-img-${SUFFIX}`,
      path: '/var/lib/libvirt/images/ubuntu-24.04.qcow2',
      kind: 'qcow2',
      size_gib: 1,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    imageId = j.id;
    const ar = await api('POST', `${P}/api/v1/content/images/${imageId}/approve`, {});
    if (!ok(ar.status)) throw new Error(`approve ${ar.status}`);
    return `id=${imageId.slice(0, 8)}`;
  });

  await mark('content-create-reject', async () => {
    const r = await api('POST', `${P}/api/v1/content/images`, {
      name: `reg-img-${SUFFIX}-r`,
      path: '/tmp/reg-missing.qcow2',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    imageRejectId = j.id;
    const rr = await api('POST', `${P}/api/v1/content/images/${imageRejectId}/reject`, {
      reason: 'regression',
    });
    if (!ok(rr.status)) throw new Error(`reject ${rr.status}`);
    return `id=${imageRejectId.slice(0, 8)}`;
  });

  await mark('content-delete', async () => {
    for (const id of [imageId, imageRejectId]) {
      if (!id) continue;
      const r = await api('DELETE', `${P}/api/v1/content/images/${id}`);
      if (!ok(r.status)) throw new Error(`delete ${id} ${r.status} ${String(r.body).slice(0, 80)}`);
    }
    imageId = null;
    imageRejectId = null;
    return 'deleted';
  });

  await mark('soc-rules', async () => {
    const j = await getJson(`${P}/api/v1/soc/rules`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('soc-rule-patch', async () => {
    const list = await getJson(`${P}/api/v1/soc/rules`);
    const id = list[0].id;
    const r = await api('PATCH', `${P}/api/v1/soc/rules/${id}`, { enabled: !!list[0].enabled });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return list[0].name;
  });

  await mark('soc-rule-test', async () => {
    const list = await getJson(`${P}/api/v1/soc/rules`);
    const r = await api('POST', `${P}/api/v1/soc/rules/${list[0].id}/test`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.would_fire == null && j.match_count == null) throw new Error('empty');
    return `matches=${j.match_count} fire=${j.would_fire}`;
  });

  await mark('firewall-approvals', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/approvals`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('firewall-k8s-status', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/k8s/status`);
    if (j.ready == null && !j.backend) throw new Error('empty');
    return `ready=${j.ready} backend=${j.backend}`;
  });

  await mark('firewall-k8s-plan', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/k8s/plan`, {
      namespace: 'default',
      profile: 'ProductionServer',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.manifests)) throw new Error('empty');
    return `manifests=${j.manifests.length}`;
  });

  await mark('fleet-activity', async () => {
    const j = await getJson(`${P}/api/v1/fleet/activity`);
    if (!j.summary && !Array.isArray(j.top_vms)) throw new Error('empty');
    return String(j.summary || '').slice(0, 80);
  });

  await mark('placement-recommendations', async () => {
    const j = await getJson(`${P}/api/v1/placement/recommendations`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('placement-refresh', async () => {
    const r = await api('POST', `${P}/api/v1/placement/refresh`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return `status=${r.status}`;
  });

  await mark('storage-tiers-overview', async () => {
    const j = await getJson(`${P}/api/v1/storage/tiers/overview`);
    if (!Array.isArray(j.tiers)) throw new Error('empty');
    return `tiers=${j.tiers.length}`;
  });

  await mark('topology', async () => {
    const j = await getJson(`${P}/api/v1/topology`);
    if (!Array.isArray(j.nodes)) throw new Error('empty');
    return `nodes=${j.nodes.length}`;
  });

  await mark('tasks-list-get', async () => {
    const list = await getJson(`${P}/api/v1/tasks?limit=3`);
    if (!Array.isArray(list) || list.length < 1) throw new Error('empty');
    const id = list[0].id || list[0].task_id;
    const j = await getJson(`${P}/api/v1/tasks/${id}`);
    if (!j.operation) throw new Error('empty');
    return `${j.operation}/${j.status}`;
  });

  await mark('events-list', async () => {
    const j = await getJson(`${P}/api/v1/events?limit=5`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('atlas-disabled', async () => {
    const r = await api('GET', `${P}/api/v1/atlas/pools`);
    if (r.status !== 503) throw new Error(`expected 503 got ${r.status}`);
    const j = JSON.parse(r.body);
    if (j.error_code !== 'atlas_disabled') throw new Error(j.error_code || 'no code');
    return 'atlas_disabled';
  });

  await mark('enrollment-tokens', async () => {
    const j = await getJson(`${P}/api/v1/enrollment/tokens`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  // cleanup leftovers from earlier probes
  try {
    for (const p of await getJson(`${P}/api/v1/ai/providers`)) {
      if (/^reg-prov/i.test(p.name || '')) await api('DELETE', `${P}/api/v1/ai/providers/${p.id}`);
    }
    for (const p of await getJson(`${P}/api/v1/ai/prompts`)) {
      if (/^reg-pr/i.test(p.title || '')) await api('DELETE', `${P}/api/v1/ai/prompts/${p.id}`);
    }
    for (const b of await getJson(`${P}/api/v1/blueprints`)) {
      if (/^reg-bp/i.test(b.name || '')) await api('DELETE', `${P}/api/v1/blueprints/${b.id}`);
    }
    for (const i of await getJson(`${P}/api/v1/content/images`)) {
      if (/^reg-img/i.test(i.name || '')) await api('DELETE', `${P}/api/v1/content/images/${i.id}`);
    }
  } catch {
    /* ignore */
  }
  if (providerId) await api('DELETE', `${P}/api/v1/ai/providers/${providerId}`).catch(() => {});
  if (promptId) await api('DELETE', `${P}/api/v1/ai/prompts/${promptId}`).catch(() => {});
  if (blueprintId) await api('DELETE', `${P}/api/v1/blueprints/${blueprintId}`).catch(() => {});
  if (imageId) await api('DELETE', `${P}/api/v1/content/images/${imageId}`).catch(() => {});
  if (imageRejectId) await api('DELETE', `${P}/api/v1/content/images/${imageRejectId}`).catch(() => {});

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`PROVIDERS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

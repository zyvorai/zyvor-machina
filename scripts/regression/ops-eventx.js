#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Events / tasks / audit filters + notifications deliver + webhook retry negatives.
 * Also: zeus timeline, AI routing rules, MFA policies, storage pool activate negatives,
 * network segments / graphics schema negatives. Avoids events/stream and real pool mutate.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-eventx');
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
    log.append({ kind: 'EVENTX', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'EVENTX', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('events-kind', async () => {
    const r = await api('GET', `${P}/api/v1/events?limit=3&kind=vm.power`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('events-vm', async () => {
    const r = await api('GET', `${P}/api/v1/events?limit=3&vm_id=${PID}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('tasks-operation', async () => {
    const r = await api('GET', `${P}/api/v1/tasks?operation=host.inventory&limit=3`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length} op=${j[0].operation}`;
  });

  await mark('tasks-failed', async () => {
    const r = await api('GET', `${P}/api/v1/tasks?status=failed&limit=3`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  let taskId = null;
  await mark('tasks-get', async () => {
    const list = JSON.parse((await api('GET', `${P}/api/v1/tasks?limit=1`)).body);
    if (!Array.isArray(list) || !list[0]) throw new Error('no tasks');
    taskId = list[0].id;
    const r = await api('GET', `${P}/api/v1/tasks/${taskId}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.id !== taskId) throw new Error('id mismatch');
    return `id=${taskId.slice(0, 8)} status=${j.status}`;
  });

  await mark('tasks-missing-negative', async () => {
    const r = await api('GET', `${P}/api/v1/tasks/${FAKE}`);
    if (r.status !== 404) throw new Error(`expected 404 got ${r.status}`);
    return `${r.status}`;
  });

  await mark('audit-action', async () => {
    const r = await api('GET', `${P}/api/v1/audit?limit=3&action=enrollment.create`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('audit-resource-type', async () => {
    const r = await api('GET', `${P}/api/v1/audit?limit=5&resource_type=vm`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('notifications-kind', async () => {
    const r = await api('GET', `${P}/api/v1/notifications?limit=3&kind=alert.task_failed`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length} kind=${j[0].kind}`;
  });

  await mark('notifications-deliver', async () => {
    const list = JSON.parse((await api('GET', `${P}/api/v1/notifications?limit=1`)).body);
    if (!Array.isArray(list) || !list[0]) throw new Error('no notifications');
    const r = await api('POST', `${P}/api/v1/notifications/${list[0].id}/deliver`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `delivered=${j.delivered}`;
  });

  await mark('webhook-deliveries', async () => {
    const r = await api('GET', `${P}/api/v1/webhook-deliveries?limit=5`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('webhook-retry-negative', async () => {
    const r = await api('POST', `${P}/api/v1/webhook-deliveries/${FAKE}/retry`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('api-keys-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/api-keys`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('zeus-security-timeline', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-security/hosts/${HID}/timeline`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    JSON.parse(r.body);
    return 'ok';
  });

  await mark('zeus-enforcement-policies', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-security/enforcement/policies`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `policies=${(j.policies || []).length} mode=${j.api_mode}`;
  });

  await mark('zeus-firewall-compliance', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-firewall/compliance/${FAKE}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `scanned=${j.machines_scanned} critical=${j.critical}`;
  });

  await mark('ai-routing-rules', async () => {
    const r = await api('GET', `${P}/api/v1/ai/routing/rules`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `rules=${j.length}`;
  });

  await mark('enterprise-mfa-policies', async () => {
    const r = await api('GET', `${P}/api/v1/enterprise/mfa/policies`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('soc-integrations', async () => {
    const r = await api('GET', `${P}/api/v1/soc/integrations`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length} type=${j[0].integration_type}`;
  });

  await mark('soc-integration-test-negative', async () => {
    const r = await api('POST', `${P}/api/v1/soc/integrations/${FAKE}/test`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('storage-activate-negative', async () => {
    const r = await api('POST', `${P}/api/v1/storage/pools/${FAKE}/activate`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('storage-deactivate-negative', async () => {
    const r = await api('POST', `${P}/api/v1/storage/pools/${FAKE}/deactivate`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('network-segments-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/network/segments`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('graphics-add-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/graphics/add`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('guestkit-job-soft', async () => {
    const r = await api('GET', `${P}/api/v1/guestkit/jobs/${FAKE}`);
    if (r.status === 503 || r.status === 404 || r.status === 400) {
      return `expected ${r.status}`;
    }
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return 'ok';
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.state && j.state !== 'running') throw new Error(`state=${j.state}`);
    return `state=${j.state || j.status || 'ok'}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`EVENTX_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

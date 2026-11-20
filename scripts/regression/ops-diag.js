#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Diagnose / developer / settings smoke: VM+host diagnose/health-check,
 * libvirt-details, cockpit libvirt queries, users/me, SOC/cluster/AI settings
 * patches, developer/support/openapi, air-gap bundle CRUD.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-diag');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
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
    log.append({ kind: 'DIAG', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'DIAG', api: name, ok: false, note: e.message.slice(0, 220) });
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

  let bundleId = null;
  let templateName = null;

  await mark('users-me', async () => {
    const j = await getJson(`${P}/api/v1/users/me`);
    if (!j.username) throw new Error('empty');
    return `${j.username}/${j.role}`;
  });

  await mark('users-list', async () => {
    const j = await getJson(`${P}/api/v1/users`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('users-prune', async () => {
    const r = await api('POST', `${P}/api/v1/users/prune-invalid`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.deleted == null) throw new Error('empty');
    return `deleted=${j.deleted}`;
  });

  await mark('vm-libvirt-details', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    if (!j.name || j.state == null) throw new Error('empty');
    return `name=${j.name} state=${j.state}`;
  });

  await mark('vm-diagnose', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/diagnose`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary && !j.vm_id) throw new Error('empty');
    return String(j.summary || j.query || 'ok').slice(0, 80);
  });

  await mark('vm-health-check', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/health-check`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.score && j.score_numeric == null) throw new Error('empty');
    return `score=${j.score || j.score_label} n=${j.score_numeric}`;
  });

  await mark('host-diagnose', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/diagnose`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary && !j.host_id) throw new Error('empty');
    return String(j.summary || j.hostname || 'ok').slice(0, 80);
  });

  await mark('host-health-check', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/health-check`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.ok == null && !Array.isArray(j.checks)) throw new Error('empty');
    return `ok=${j.ok} checks=${(j.checks || []).length}`;
  });

  await mark('host-libvirt-cockpit-storage', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/libvirt?action=cockpit.storage`);
    if (j.probed == null) throw new Error('empty');
    return `probed=${j.probed}`;
  });

  await mark('host-libvirt-cockpit-network', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/libvirt?action=cockpit.network`);
    if (j.probed == null) throw new Error('empty');
    return `probed=${j.probed}`;
  });

  await mark('host-libvirt-cockpit-system', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/libvirt?action=cockpit.system`);
    if (j.probed == null) throw new Error('empty');
    return `probed=${j.probed}`;
  });

  await mark('soc-settings-patch', async () => {
    const before = await getJson(`${P}/api/v1/soc/settings`);
    const r = await api('PATCH', `${P}/api/v1/soc/settings`, {
      webhook_url: before.webhook_url || '',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `webhook=${j.webhook_url === '' ? 'empty' : 'set'}`;
  });

  await mark('cluster-settings-patch', async () => {
    const before = await getJson(`${P}/api/v1/cluster/settings`);
    const r = await api('PATCH', `${P}/api/v1/cluster/settings`, {
      placement_policy: before.placement_policy || 'balanced',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `placement=${j.placement_policy} ha=${j.ha_enabled}`;
  });

  await mark('ai-settings-patch', async () => {
    const before = await getJson(`${P}/api/v1/ai/settings`);
    const r = await api('PATCH', `${P}/api/v1/ai/settings`, {
      enabled: !!before.enabled,
      mode: before.mode || 'advisor',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `enabled=${j.enabled} mode=${j.mode}`;
  });

  await mark('developer-overview', async () => {
    const j = await getJson(`${P}/api/v1/developer/overview`);
    if (!j.openapi_url) throw new Error('empty');
    return j.openapi_url;
  });

  await mark('developer-terraform-schema', async () => {
    const j = await getJson(`${P}/api/v1/developer/terraform/schema`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `resources=${j.length}`;
  });

  await mark('support-bundle', async () => {
    const j = await getJson(`${P}/api/v1/support/bundle`);
    if (!j.controller_id) throw new Error('empty');
    return `id=${j.controller_id} tasks=${j.task_count}`;
  });

  await mark('openapi-json', async () => {
    const j = await getJson(`${P}/api/v1/openapi.json`);
    if (!j.openapi || !j.info) throw new Error('empty');
    return `${j.info.title} ${j.info.version}`;
  });

  await mark('cpu-compat', async () => {
    const j = await getJson(`${P}/api/v1/cpu-compat`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('content-images', async () => {
    const j = await getJson(`${P}/api/v1/content/images`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('mfa-policies', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/mfa/policies`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('air-gap-create', async () => {
    const r = await api('POST', `${P}/api/v1/enterprise/air-gap/bundles`, {
      name: `reg-ag-${SUFFIX}`,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    bundleId = j.id;
    return `id=${j.id.slice(0, 8)} checksum=${(j.checksum || '').slice(0, 20)}`;
  });

  await mark('air-gap-get', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/air-gap/bundles/${bundleId}`);
    if (j.id !== bundleId) throw new Error('mismatch');
    return j.name;
  });

  await mark('air-gap-list', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/air-gap/bundles`);
    if (!Array.isArray(j) || !j.some((x) => x.id === bundleId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('air-gap-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/enterprise/air-gap/bundles/${bundleId}`);
    if (!ok(r.status)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    bundleId = null;
    return 'deleted';
  });

  await mark('publish-template-roundtrip', async () => {
    templateName = `reg-tpl-${SUFFIX}`;
    const r = await api('POST', `${P}/api/v1/vms/${PID}/publish-template`, {
      template_name: templateName,
      version: '0.0.1',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (j.name !== templateName) throw new Error(JSON.stringify(j).slice(0, 80));
    const dr = await api('DELETE', `${P}/api/v1/templates/${encodeURIComponent(templateName)}/0.0.1`);
    if (!ok(dr.status)) throw new Error(`delete ${dr.status} ${String(dr.body).slice(0, 80)}`);
    templateName = null;
    return 'published+deleted';
  });

  // cleanup leftovers
  if (bundleId) {
    try {
      await api('DELETE', `${P}/api/v1/enterprise/air-gap/bundles/${bundleId}`);
    } catch {
      /* ignore */
    }
  }
  if (templateName) {
    try {
      await api('DELETE', `${P}/api/v1/templates/${encodeURIComponent(templateName)}/0.0.1`);
    } catch {
      /* ignore */
    }
  }
  try {
    const leftover = await getJson(`${P}/api/v1/enterprise/air-gap/bundles`);
    for (const b of leftover) {
      if (/^reg-ag/i.test(b.name || '')) {
        await api('DELETE', `${P}/api/v1/enterprise/air-gap/bundles/${b.id}`);
      }
    }
  } catch {
    /* ignore */
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`DIAG_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

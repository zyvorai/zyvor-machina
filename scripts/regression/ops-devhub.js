#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Developer hub / operations / cluster / templates / policy / tasks negatives.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-devhub');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
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
    log.append({ kind: 'DEVHUB', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'DEVHUB', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('health', async () => {
    const r = await api('GET', `${P}/api/v1/health`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `status=${j.status} leader=${j.leader}`;
  });

  await mark('health-ready', async () => {
    const r = await api('GET', `${P}/api/v1/health/ready`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ready=${j.ready}`;
  });

  await mark('developer-overview', async () => {
    const r = await api('GET', `${P}/api/v1/developer/overview`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `openapi=${j.openapi_url}`;
  });

  await mark('terraform-schema', async () => {
    const r = await api('GET', `${P}/api/v1/developer/terraform/schema`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `resources=${j.length}`;
  });

  await mark('openapi-json', async () => {
    const r = await api('GET', `${P}/api/v1/openapi.json`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `title=${j.info && j.info.title}`;
  });

  await mark('support-bundle', async () => {
    const r = await api('GET', `${P}/api/v1/support/bundle`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `tasks=${j.task_count}`;
  });

  await mark('install-sh', async () => {
    const r = await api('GET', `${P}/install.sh`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!/bash|machina|CONTROLLER/i.test(r.body || '')) throw new Error('not script');
    return `bytes=${(r.body || '').length}`;
  });

  await mark('cluster', async () => {
    const r = await api('GET', `${P}/api/v1/cluster`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `hosts=${j.host_count} vms=${j.vm_count}`;
  });

  await mark('cluster-settings', async () => {
    const r = await api('GET', `${P}/api/v1/cluster/settings`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ha=${j.ha_enabled} drs=${j.drs_auto_migrate}`;
  });

  await mark('cluster-leadership', async () => {
    const r = await api('GET', `${P}/api/v1/cluster/leadership`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `leader=${j.is_leader}`;
  });

  await mark('operations-overview', async () => {
    const r = await api('GET', `${P}/api/v1/operations/overview`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `runbooks=${j.runbook_count} grade=${j.compliance_grade}`;
  });

  await mark('operations-runbooks', async () => {
    const r = await api('GET', `${P}/api/v1/operations/runbooks`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('operations-executions', async () => {
    const r = await api('GET', `${P}/api/v1/operations/executions`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('templates', async () => {
    const r = await api('GET', `${P}/api/v1/templates`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('templates-marketplace', async () => {
    const r = await api('GET', `${P}/api/v1/templates/marketplace`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('templates-missing-images', async () => {
    const r = await api('GET', `${P}/api/v1/templates/missing-images`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `missing=${(j.missing || []).length}`;
  });

  await mark('storage-tiers', async () => {
    const r = await api('GET', `${P}/api/v1/storage/tiers/overview`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `tiers=${(j.tiers || []).length}`;
  });

  await mark('backup-sla', async () => {
    const r = await api('GET', `${P}/api/v1/storage/backup-sla`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `policies=${(j.policies || []).length}`;
  });

  await mark('policy-rules', async () => {
    const r = await api('GET', `${P}/api/v1/policy/rules`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('policy-quotas', async () => {
    const r = await api('GET', `${P}/api/v1/policy/quotas`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('policy-export', async () => {
    const r = await api('GET', `${P}/api/v1/ai/policy/export`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.yaml) throw new Error('no yaml');
    return `yaml_bytes=${j.yaml.length}`;
  });

  await mark('segments-gitops-export', async () => {
    const r = await api('GET', `${P}/api/v1/network/segments/gitops/export`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `kind=${j.kind}`;
  });

  await mark('topology', async () => {
    const r = await api('GET', `${P}/api/v1/topology`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `nodes=${(j.nodes || []).length}`;
  });

  await mark('network-canvas', async () => {
    const r = await api('GET', `${P}/api/v1/network-canvas`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `nodes=${((j.topology && j.topology.nodes) || j.nodes || []).length}`;
  });

  await mark('task-cancel-negative', async () => {
    const r = await api('POST', `${P}/api/v1/tasks/${FAKE}/cancel`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('task-retry-negative', async () => {
    const r = await api('POST', `${P}/api/v1/tasks/${FAKE}/retry`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('ai-network-explain', async () => {
    const r = await api('POST', `${P}/api/v1/ai/network/explain`, { vm_a: PID, vm_b: PID });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `can_reach=${j.can_reach}`;
  });

  await mark('ai-runbook', async () => {
    const r = await api('POST', `${P}/api/v1/ai/runbook`, { incident: 'high-cpu' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `steps=${(j.steps || []).length}`;
  });

  await mark('ai-spotlight', async () => {
    const r = await api('POST', `${P}/api/v1/ai/spotlight`, { query: 'hosts' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `intents=${(j.intents || []).length}`;
  });

  await mark('ai-explain', async () => {
    const r = await api('POST', `${P}/api/v1/ai/explain`, {
      screen: 'vm-detail',
      subject: 'vm',
      id: PID,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.explanation) throw new Error('empty');
    return String(j.explanation).slice(0, 60);
  });

  await mark('ai-explain-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/ai/explain`, { subject: 'vm', id: PID });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`DEVHUB_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

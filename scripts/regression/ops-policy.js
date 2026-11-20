#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Policy / templates / storage / marketplace smoke: template seed, storage
 * pool refresh, policy quotas, vault sync, marketplace plugin install, cost /
 * rightsizing / memory settings, port-forward guest-IP negative.
 * Does NOT enqueue templates.prefetch_missing (disk-heavy).
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-policy');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let POOL = process.env.MACHINA_STORAGE_POOL_ID || '';
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
    log.append({ kind: 'POLICY', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'POLICY', api: name, ok: false, note: e.message.slice(0, 220) });
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
  if (_ids.platformVmId) PID = _ids.platformVmId;
  if (_ids.storagePoolId) POOL = _ids.storagePoolId;  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('templates-seed', async () => {
    const r = await api('POST', `${P}/api/v1/templates/seed`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (j.inserted == null && !Array.isArray(j.templates)) throw new Error('empty');
    return `inserted=${j.inserted} templates=${(j.templates || []).length}`;
  });

  await mark('templates-sync-git-unset', async () => {
    const r = await api('POST', `${P}/api/v1/templates/sync-git`, {});
    if (r.status < 400) throw new Error(`expected error got ${r.status}`);
    if (!/MACHINA_TEMPLATES_GIT_DIR|not set/i.test(r.body || '')) {
      throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    }
    return `${r.status} git-dir-unset`;
  });

  await mark('storage-pool-get', async () => {
    const j = await getJson(`${P}/api/v1/storage/pools/${POOL}`);
    if (!j.id || j.id !== POOL) throw new Error('id mismatch');
    return `name=${j.name} backend=${j.backend}`;
  });

  await mark('storage-pool-volumes', async () => {
    const j = await getJson(`${P}/api/v1/storage/pools/${POOL}/volumes`);
    if (!Array.isArray(j.volumes) && !Array.isArray(j)) throw new Error('empty');
    const vols = j.volumes || j;
    return `count=${vols.length}`;
  });

  await mark('storage-pool-refresh', async () => {
    const r = await api('POST', `${P}/api/v1/storage/pools/${POOL}/refresh`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.status !== 'refreshed' && !j.name) throw new Error(JSON.stringify(j).slice(0, 80));
    return j.status || j.name;
  });

  await mark('storage-snapshot-policy', async () => {
    const j = await getJson(`${P}/api/v1/storage/pools/${POOL}/snapshot-policy`);
    if (j.snapshot_retention_days == null && !j.summary) throw new Error('empty');
    return j.summary || `retain_days=${j.snapshot_retention_days}`;
  });

  await mark('network-gitops-export', async () => {
    const j = await getJson(`${P}/api/v1/network/segments/gitops/export`);
    if (!j.kind || !Array.isArray(j.segments)) throw new Error('empty');
    return `kind=${j.kind} segments=${j.segments.length}`;
  });

  await mark('network-ipam-pools', async () => {
    const j = await getJson(`${P}/api/v1/network/ipam/pools`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('fence-events', async () => {
    const j = await getJson(`${P}/api/v1/fence/events`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('fleet-maintenance-mission', async () => {
    const j = await getJson(`${P}/api/v1/fleet/maintenance-mission`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('marketplace-plugins', async () => {
    const j = await getJson(`${P}/api/v1/marketplace/plugins`);
    const plugins = j.plugins || j;
    if (!Array.isArray(plugins) || plugins.length < 1) throw new Error('empty');
    return `count=${plugins.length}`;
  });

  await mark('marketplace-plugin-install-uninstall', async () => {
    let r = await api('POST', `${P}/api/v1/marketplace/plugins/kubevirt-bridge/install`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`install ${r.status} ${String(r.body).slice(0, 80)}`);
    const inst = JSON.parse(r.body);
    if (!inst.installed) throw new Error(JSON.stringify(inst).slice(0, 80));
    r = await api('POST', `${P}/api/v1/marketplace/plugins/kubevirt-bridge/uninstall`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`uninstall ${r.status}`);
    const un = JSON.parse(r.body);
    if (un.installed !== false) throw new Error(JSON.stringify(un).slice(0, 80));
    return 'kubevirt-bridge roundtrip';
  });

  await mark('policy-rules', async () => {
    const j = await getJson(`${P}/api/v1/policy/rules`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('policy-quotas-upsert', async () => {
    const r = await api('POST', `${P}/api/v1/policy/quotas`, {
      project: 'default',
      max_vms: 100,
      max_vcpu: 256,
      max_memory_mib: 1048576,
      max_storage_gib: 10000,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (j.project !== 'default') throw new Error(JSON.stringify(j).slice(0, 80));
    const list = await getJson(`${P}/api/v1/policy/quotas`);
    if (!list.some((x) => x.project === 'default')) throw new Error('not listed');
    return `max_vms=${j.max_vms} max_vcpu=${j.max_vcpu}`;
  });

  await mark('recommendations', async () => {
    const j = await getJson(`${P}/api/v1/recommendations`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('ai-rightsizing-report', async () => {
    const j = await getJson(`${P}/api/v1/ai/rightsizing/report`);
    if (!Array.isArray(j.recommendations)) throw new Error('empty');
    return `recs=${j.recommendations.length}`;
  });

  await mark('ai-cost', async () => {
    const j = await getJson(`${P}/api/v1/ai/cost`);
    if (j.estimated_monthly_usd == null) throw new Error('empty');
    return `monthly=${j.estimated_monthly_usd} vms=${j.vm_count}`;
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
    const r = await api('PATCH', `${P}/api/v1/ai/memory/settings`, {
      enabled: true,
      retention_days: j.retention_days || 90,
      project_scope: j.project_scope !== false,
      team_scope: !!j.team_scope,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    return `enabled=${j.enabled} retention=${j.retention_days}`;
  });

  await mark('ai-memory-incidents', async () => {
    const j = await getJson(`${P}/api/v1/ai/memory/incidents`);
    if (!Array.isArray(j.incidents) && !Array.isArray(j)) throw new Error('empty');
    const list = j.incidents || j;
    return `count=${list.length}`;
  });

  await mark('vault-sync-all', async () => {
    const r = await api('POST', `${P}/api/v1/enterprise/vault/sync-all`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.synced == null) throw new Error('empty');
    return `synced=${j.synced}`;
  });

  await mark('vault-provider-sync', async () => {
    const providers = await getJson(`${P}/api/v1/enterprise/vault/providers`);
    if (!Array.isArray(providers) || providers.length < 1) throw new Error('no providers');
    const id = providers[0].id;
    const r = await api('POST', `${P}/api/v1/enterprise/vault/providers/${id}/sync`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `status=${j.status} name=${j.provider_name || providers[0].name}`;
  });

  await mark('port-forwards-list', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/port-forwards`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('port-forward-create-when-guest-ip-known', async () => {
    // Controller resolves guest IP from DB (not request body). When known, create succeeds.
    // Windows guests without QGA often have no IP yet — accept that expected 400.
    await api('POST', `${P}/api/v1/vms/${PID}/port-forwards/delete`, {
      protocol: 'tcp',
      host_port: 18080,
      vm_port: 80,
    }).catch(() => {});
    const r = await api('POST', `${P}/api/v1/vms/${PID}/port-forwards`, {
      host_port: 18080,
      vm_port: 80,
      protocol: 'tcp',
      description: 'ops-policy regression',
    });
    if (ok(r.status)) {
      const d = await api('POST', `${P}/api/v1/vms/${PID}/port-forwards/delete`, {
        protocol: 'tcp',
        host_port: 18080,
        vm_port: 80,
      });
      if (!ok(d.status)) throw new Error(`cleanup ${d.status}`);
      return 'created+deleted tcp/18080→80';
    }
    if (r.status === 400 && /guest ip is not known|dhcp|guest tools/i.test(r.body || '')) {
      return `${r.status}-guest-ip-unknown`;
    }
    throw new Error(`create ${r.status} ${String(r.body).slice(0, 120)}`);
  });

  await mark('port-forward-bad-port', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/port-forwards`, {
      host_port: 1,
      vm_port: 80,
      protocol: 'tcp',
    });
    if (r.status < 400) throw new Error(`expected error got ${r.status}`);
    return `${r.status} bad-host-port`;
  });

  await mark('port-forward-templates', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/port-forward-templates`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  // ensure kubevirt-bridge uninstalled
  try {
    await api('POST', `${P}/api/v1/marketplace/plugins/kubevirt-bridge/uninstall`, {});
  } catch {
    /* ignore */
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`POLICY_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Zeus security fabric / multisite / enforcement smoke: asset inventory,
 * fleet sensors/threat, host fabric endpoints, multisite sync, temporary
 * rules, operator dry-run, AI Zeus plan/chat/copilot, fleet keychain/spaces.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-security');
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
    log.append({ kind: 'SEC', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'SEC', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('security-status', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/status`);
    if (!j.summary) throw new Error('empty');
    return 'ok';
  });

  await mark('security-sensors', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/sensors`);
    if (!Array.isArray(j.sensors)) throw new Error('empty');
    return `count=${j.sensors.length}`;
  });

  await mark('security-graph', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/graph`);
    if (!Array.isArray(j.nodes)) throw new Error('empty');
    return `nodes=${j.nodes.length}`;
  });

  await mark('asset-inventory', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/asset-inventory`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('correlations', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/correlations`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('fabric-health', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/fabric/health`);
    if (!j.status) throw new Error('empty');
    return `${j.status} sensors=${j.sensors_total}`;
  });

  await mark('fleet-sensors', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/fleet/sensors`);
    if (!Array.isArray(j.sensors) && !Array.isArray(j.matrix)) throw new Error('empty');
    return `matrix=${(j.matrix || []).length}`;
  });

  await mark('fleet-threat', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/fleet/threat`);
    if (j.fleet_threat_score == null) throw new Error('empty');
    return `score=${j.fleet_threat_score}`;
  });

  await mark('fleet-timeline', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/fleet/timeline`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('host-summary', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/summary`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('host-ports', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/ports`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('host-processes', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/processes`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('host-connections', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/connections`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('host-fabric-status', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/fabric-status`);
    if (!j.host_id) throw new Error('empty');
    return `reachable=${j.agent_reachable}`;
  });

  await mark('host-enforcement', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/enforcement`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('enforcement-status', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/enforcement/status`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('enforcement-policies', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/enforcement/policies`);
    if (!Array.isArray(j.policies)) throw new Error('empty');
    return `count=${j.policies.length} mode=${j.api_mode}`;
  });

  await mark('enforcement-sync', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/enforcement/sync`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ok=${j.ok} note=${String(j.note || '').slice(0, 60)}`;
  });

  await mark('enforcement-attach-mode', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/enforcement/attach`, { host_id: HID });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ok=${j.ok} note=${String(j.note || '').slice(0, 60)}`;
  });

  await mark('alerts-sync', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/alerts/sync`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `inserted=${j.inserted}`;
  });

  await mark('security-search', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-security/search`, { query: 'ssh' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ok=${j.ok}`;
  });

  await mark('host-fabric-status', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/fabric-status`);
    if (j.agent_reachable == null) throw new Error('empty');
    return `agent_reachable=${j.agent_reachable}`;
  });

  await mark('host-enforcement', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/enforcement`);
    if (j.api_mode !== 'native') throw new Error(`api_mode=${j.api_mode}`);
    return `policies=${(j.policies || []).length}`;
  });

  await mark('hunt-queries', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hunt/queries`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('multisite-overview', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/multisite/overview`);
    if (!Array.isArray(j.sites) || j.sites.length < 1) throw new Error('empty');
    return `sites=${j.sites.length}`;
  });

  await mark('multisite-connectivity', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/multisite/connectivity`);
    if (!Array.isArray(j.matrix)) throw new Error('empty');
    return `edges=${j.matrix.length}`;
  });

  await mark('multisite-drift', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/multisite/drift`);
    if (!Array.isArray(j.sites)) throw new Error('empty');
    return `sites=${j.sites.length}`;
  });

  await mark('multisite-timeline', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/multisite/timeline`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('multisite-dr-templates', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/multisite/dr-templates`);
    if (!j.primary_site || !Array.isArray(j.profiles)) throw new Error('empty');
    return `profiles=${j.profiles.length}`;
  });

  await mark('multisite-export', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/multisite/export`);
    if (!j.kind || !Array.isArray(j.sites)) throw new Error('empty');
    return j.kind;
  });

  await mark('multisite-sync', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/multisite/sync`, {
      source_site: 'primary-local',
      target_site: 'dr-replica',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
    const j = JSON.parse(r.body);
    if (j.synced_policies == null) throw new Error('empty');
    return `synced=${j.synced_policies}`;
  });

  await mark('operator-thresholds', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/operator/thresholds`);
    if (j.max_risk_score == null) throw new Error('empty');
    return `max=${j.max_risk_score} auto=${j.auto_apply_enabled}`;
  });

  await mark('operator-plan', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/operator/plan`);
    if (!Array.isArray(j.previews)) throw new Error('empty');
    return `previews=${j.previews.length}`;
  });

  await mark('operator-execute-dry', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/operator/execute`, {
      host_id: HID,
      dry_run: true,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.dry_run !== true) throw new Error('expected dry_run');
    return `ops=${j.operations}`;
  });

  await mark('temporary-rule', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/temporary-rules`, {
      target_kind: 'host',
      target_id: HID,
      dest_port: 2222,
      protocol: 'tcp',
      duration_hours: 1,
      reason: `reg-sec-${SUFFIX}`,
      source_cidr: '10.0.0.0/8',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id && !j.rule && !j.dest_port) throw new Error(JSON.stringify(j).slice(0, 80));
    return `port=${j.dest_port || 2222} id=${String(j.id || '').slice(0, 8)}`;
  });

  await mark('gitops-export', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/policies/gitops/export`);
    if (!j.kind) throw new Error('empty');
    return j.kind;
  });

  await mark('gitops-sync', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/policies/gitops/sync`, { policies: [] });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.upserted == null) throw new Error('empty');
    return `upserted=${j.upserted} sync=${String(j.sync_id || '').slice(0, 8)}`;
  });

  await mark('finops-export-csv', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-firewall/finops/exposure/export.csv`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!/Exposure|USD/i.test(r.body || '')) throw new Error('empty csv');
    return `bytes=${r.body.length}`;
  });

  await mark('compliance-cis-pdf', async () => {
    const r = await api('GET', `${P}/api/v1/zeus-firewall/compliance/cis/export.pdf`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!/%PDF/.test(r.body || '')) throw new Error('not pdf');
    return `bytes=${r.body.length}`;
  });

  await mark('fleet-keychain', async () => {
    const j = await getJson(`${P}/api/v1/fleet/keychain`);
    if (!j.summary) throw new Error('empty');
    return String(j.summary).slice(0, 80);
  });

  await mark('fleet-spaces', async () => {
    const j = await getJson(`${P}/api/v1/fleet/spaces`);
    if (!j.summary || !Array.isArray(j.spaces)) throw new Error('empty');
    return `spaces=${j.space_count} vms=${j.total_vms}`;
  });

  await mark('ai-enterprise-zeus', async () => {
    const r = await api('GET', `${P}/api/v1/ai/enterprise/zeus`);
    if (r.status === 404) return 'skipped (route absent)';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.zeus_admin_role == null) throw new Error('empty');
    return `admin=${j.zeus_admin_role} exec=${j.zeus_execute_role}`;
  });

  await mark('ai-policy-export', async () => {
    const j = await getJson(`${P}/api/v1/ai/policy/export`);
    if (!j.yaml) throw new Error('empty');
    return `yaml_bytes=${j.yaml.length}`;
  });

  await mark('ai-zeus-plan', async () => {
    const r = await api('POST', `${P}/api/v1/ai/zeus/plan`, {
      goal: 'summarize fleet security posture',
    });
    if (r.status === 404) return 'skipped (route absent)';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.goal || !Array.isArray(j.steps)) throw new Error('empty');
    return `steps=${j.steps.length} agent=${j.agent_id}`;
  });

  await mark('ai-zeus-chat', async () => {
    const r = await api('POST', `${P}/api/v1/ai/zeus/chat`, { message: 'fleet status' });
    if (r.status === 404) return 'skipped (route absent)';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.reply) throw new Error('empty');
    return String(j.reply).slice(0, 60);
  });

  await mark('ai-copilot-chat', async () => {
    const r = await api('POST', `${P}/api/v1/ai/copilot/chat`, { message: 'what is the fleet status?' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.reply) throw new Error('empty');
    return String(j.reply).slice(0, 60);
  });

  await mark('ai-terminal-suggest', async () => {
    const r = await api('POST', `${P}/api/v1/ai/terminal/suggest`, { prompt: 'list vms' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.suggestions)) throw new Error('empty');
    return `suggestions=${j.suggestions.length}`;
  });

  await mark('ai-intent-environment', async () => {
    const r = await api('POST', `${P}/api/v1/ai/intent/environment`, {
      query: 'small web stack with 2 vms',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.vm_count == null && !j.label) throw new Error('empty');
    return `vms=${j.vm_count} label=${j.label}`;
  });

  await mark('ai-network-explain', async () => {
    const r = await api('POST', `${P}/api/v1/ai/network/explain`, { vm_a: PID, vm_b: PID });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.can_reach == null && !j.explanation) throw new Error('empty');
    return `can_reach=${j.can_reach}`;
  });

  await mark('hosts-sync-all', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/sync-all`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `tasks=${j.length} op=${j[0].operation}`;
  });

  await mark('vms-prune-missing', async () => {
    const r = await api('POST', `${P}/api/v1/vms/prune-missing`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.deleted == null) throw new Error('empty');
    return `deleted=${j.deleted}`;
  });

  await mark('vmware-sync', async () => {
    const r = await api('POST', `${P}/api/v1/vmware/sync`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.synced == null && !j.message) throw new Error('empty');
    return `synced=${j.synced} imported=${j.imported}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`SECURITY_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

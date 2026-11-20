#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Observability / compliance / consolehub / reports smoke: SLO overview,
 * traces, prometheus, capacity/finops reports, AI compliance + incidents,
 * marketplace agents, API key rotate, ConsoleHub JIT/break-glass, exports.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-obs');
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
    log.append({ kind: 'OBS', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'OBS', api: name, ok: false, note: e.message.slice(0, 220) });
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

  let keyId = null;
  let accessRequestId = null;
  const sessionIds = [];

  await mark('observability-overview', async () => {
    const j = await getJson(`${P}/api/v1/observability/overview`);
    if (!Array.isArray(j.slos) || j.slos.length < 1) throw new Error('empty slos');
    return `slos=${j.slos.length}`;
  });

  await mark('observability-traces', async () => {
    const j = await getJson(`${P}/api/v1/observability/traces?limit=5`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('metrics-prometheus', async () => {
    const r = await api('GET', `${P}/api/v1/metrics/prometheus`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!/machina_platform_hosts/.test(r.body || '')) throw new Error('missing metric');
    return `bytes=${r.body.length}`;
  });

  await mark('reports-capacity', async () => {
    const j = await getJson(`${P}/api/v1/reports/capacity`);
    if (j.total_vms == null) throw new Error('empty');
    return `vms=${j.total_vms} online=${j.hosts_online}`;
  });

  await mark('reports-finops', async () => {
    const j = await getJson(`${P}/api/v1/reports/finops`);
    if (j.vm_count == null) throw new Error('empty');
    return `vms=${j.vm_count} vcpu=${j.total_vcpu}`;
  });

  await mark('ai-compliance', async () => {
    const j = await getJson(`${P}/api/v1/ai/compliance`);
    if (j.score == null || !j.grade) throw new Error('empty');
    return `score=${j.score} grade=${j.grade}`;
  });

  await mark('ai-compliance-frameworks', async () => {
    const j = await getJson(`${P}/api/v1/ai/compliance/frameworks`);
    if (!Array.isArray(j.frameworks) || j.frameworks.length < 1) throw new Error('empty');
    return `count=${j.frameworks.length}`;
  });

  await mark('ai-compliance-export', async () => {
    const r = await api('GET', `${P}/api/v1/ai/compliance/export`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!isHtml(r.body) && !/Compliance/i.test(r.body || '')) throw new Error('empty export');
    return `bytes=${(r.body || '').length}`;
  });

  await mark('ai-compliance-remediate', async () => {
    const j = await getJson(`${P}/api/v1/ai/compliance/remediate`);
    if (!Array.isArray(j.remediations)) throw new Error('empty');
    return `count=${j.remediations.length}`;
  });

  await mark('ai-incidents-active', async () => {
    const j = await getJson(`${P}/api/v1/ai/incidents/active`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('ai-incident-analyze', async () => {
    const r = await api('POST', `${P}/api/v1/ai/incidents/analyze`, {
      symptoms: ['high cpu', 'latency'],
      vm_id: PID,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j.timeline) && !j.window_hours) throw new Error('empty');
    return `hours=${j.window_hours} events=${(j.timeline || []).length}`;
  });

  await mark('ai-incident-ack-room', async () => {
    const list = await getJson(`${P}/api/v1/ai/incidents/active`);
    if (!Array.isArray(list) || list.length < 1) return 'no-incidents';
    const id = list[0].id;
    const ar = await api('POST', `${P}/api/v1/ai/incidents/${id}/ack`, {});
    if (!ok(ar.status)) throw new Error(`ack ${ar.status}`);
    const room = await getJson(`${P}/api/v1/ai/incidents/${id}/room`);
    if (!room.incident && !room.id) throw new Error('empty room');
    return `id=${String(id).slice(0, 8)}`;
  });

  await mark('ai-marketplace-agents', async () => {
    const j = await getJson(`${P}/api/v1/ai/marketplace/agents`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('ai-agent-install-uninstall', async () => {
    const slug = 'aws-expert';
    let r = await api('POST', `${P}/api/v1/ai/marketplace/agents/${slug}/install`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`install ${r.status}`);
    let j = JSON.parse(r.body);
    if (!j.installed) throw new Error('not installed');
    r = await api('POST', `${P}/api/v1/ai/marketplace/agents/${slug}/uninstall`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`uninstall ${r.status}`);
    j = JSON.parse(r.body);
    if (j.installed) throw new Error('still installed');
    return slug;
  });

  await mark('ai-remediate-hub', async () => {
    const j = await getJson(`${P}/api/v1/ai/remediate/hub`);
    if (!Array.isArray(j.items)) throw new Error('empty');
    return `items=${j.items.length}`;
  });

  await mark('ai-rightsizing-report', async () => {
    const j = await getJson(`${P}/api/v1/ai/rightsizing/report`);
    if (!Array.isArray(j.recommendations)) throw new Error('empty');
    return `recs=${j.recommendations.length}`;
  });

  await mark('ai-gpu-placement', async () => {
    const j = await getJson(`${P}/api/v1/ai/fleet/gpu-placement`);
    if (!j.workload && !Array.isArray(j.candidates)) throw new Error('empty');
    return `candidates=${(j.candidates || []).length}`;
  });

  await mark('ai-migration-readiness', async () => {
    const r = await api('POST', `${P}/api/v1/ai/migration/readiness-report`, { vm_id: PID });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.executive_summary) throw new Error('empty');
    return String(j.executive_summary).slice(0, 80);
  });

  await mark('migrations-advisor', async () => {
    const j = await getJson(`${P}/api/v1/migrations/advisor?vm=${PID}`);
    if (j.readiness_percent == null && !j.safe) throw new Error('empty');
    return `ready=${j.readiness_percent}`;
  });

  await mark('capacity-export-csv', async () => {
    const r = await api('GET', `${P}/api/v1/ai/capacity/export.csv`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!/Capacity/i.test(r.body || '')) throw new Error('empty csv');
    return `bytes=${r.body.length}`;
  });

  await mark('cost-export-csv', async () => {
    const r = await api('GET', `${P}/api/v1/ai/cost/export.csv`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    if (!/Cost|USD/i.test(r.body || '')) throw new Error('empty csv');
    return `bytes=${r.body.length}`;
  });

  await mark('api-key-rotate-roundtrip', async () => {
    let r = await api('POST', `${P}/api/v1/api-keys`, { name: `reg-obs-${SUFFIX}` });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`create ${r.status}`);
    let j = JSON.parse(r.body);
    if (!j.id || !j.token) throw new Error('no token');
    keyId = j.id;
    const first = j.token;
    r = await api('POST', `${P}/api/v1/api-keys/${keyId}/rotate`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`rotate ${r.status}`);
    j = JSON.parse(r.body);
    if (!j.token || j.token === first) throw new Error('token unchanged');
    r = await api('DELETE', `${P}/api/v1/api-keys/${keyId}`);
    if (!ok(r.status)) throw new Error(`delete ${r.status}`);
    keyId = null;
    return 'create+rotate+delete';
  });

  await mark('upgrade-matrix', async () => {
    const j = await getJson(`${P}/api/v1/upgrade/matrix`);
    if (!j.controller_version) throw new Error('empty');
    return `ctrl=${j.controller_version} agent=${j.recommended_agent}`;
  });

  await mark('soc-integrations', async () => {
    const j = await getJson(`${P}/api/v1/soc/integrations`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('soc-integration-test-negative', async () => {
    const r = await api('POST', `${P}/api/v1/soc/integrations/splunk/test`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status} url/token required`;
  });

  await mark('mfa-compliance', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/mfa/compliance`);
    if (j.compliant_users == null && !j.summary) throw new Error('empty');
    return String(j.summary || `compliant=${j.compliant_users}`).slice(0, 80);
  });

  await mark('fips-matrix', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/fips/matrix`);
    if (!j.active_profile) throw new Error('empty');
    return j.active_profile;
  });

  await mark('tenants-overview', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/tenants/overview`);
    if (!Array.isArray(j.projects)) throw new Error('empty');
    return `projects=${j.projects.length}`;
  });

  await mark('fleet-gpu', async () => {
    const j = await getJson(`${P}/api/v1/fleet/gpu`);
    if (!j.summary && !Array.isArray(j.gpu_hosts)) throw new Error('empty');
    return String(j.summary || '').slice(0, 80);
  });

  await mark('fleet-console', async () => {
    const j = await getJson(`${P}/api/v1/fleet/console`);
    if (j.total_24h == null) throw new Error('empty');
    return `events=${j.total_24h}`;
  });

  await mark('host-gpus', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/gpus`);
    if (!Array.isArray(j.devices)) throw new Error('empty');
    return `devices=${j.devices.length}`;
  });

  await mark('host-linux-observability', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/linux/observability`);
    if (!j.pressure) throw new Error('empty');
    return `cpu_some=${j.pressure.cpu && j.pressure.cpu.some}`;
  });

  await mark('baremetal-servers', async () => {
    const j = await getJson(`${P}/api/v1/baremetal/servers`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('cloud-init-validate', async () => {
    const r = await api('POST', `${P}/api/v1/cloud-init/validate`, {
      user_data: '#cloud-config\npackages: [nginx]\n',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.valid !== true) throw new Error(JSON.stringify(j.issues || j).slice(0, 80));
    return 'valid';
  });

  await mark('kubevirt-sync', async () => {
    const r = await api('POST', `${P}/api/v1/kubevirt/sync`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.synced) throw new Error('not synced');
    return j.cluster_id ? String(j.cluster_id).slice(0, 8) : 'ok';
  });

  await mark('consolehub-plan', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/consolehub/plan`);
    if (!j.recommended && !j.vm_id) throw new Error('empty');
    return `rec=${j.recommended}`;
  });

  await mark('consolehub-explain', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/consolehub/explain`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.explanation) throw new Error('empty');
    return String(j.explanation).slice(0, 60);
  });

  await mark('consolehub-access-approve', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/consolehub/access-requests`, {
      protocol: 'novnc',
      reason: `reg-obs-${SUFFIX}`,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.request_id) throw new Error('no request_id');
    accessRequestId = j.request_id;
    const ar = await api('POST', `${P}/api/v1/consolehub/access-requests/${accessRequestId}/approve`, {});
    if (!ok(ar.status)) throw new Error(`approve ${ar.status} ${String(ar.body).slice(0, 80)}`);
    accessRequestId = null;
    return 'requested+approved';
  });

  await mark('consolehub-break-glass', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/consolehub/break-glass`, {
      protocol: 'novnc',
      reason: `reg-obs-${SUFFIX}`,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.session_id) throw new Error('no session');
    sessionIds.push(j.session_id);
    return j.session_id.slice(0, 8);
  });

  await mark('consolehub-collaborate', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/consolehub/collaborate`, {
      protocol: 'novnc',
      reason: `reg-obs-${SUFFIX}`,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.session_id) throw new Error('no session');
    sessionIds.push(j.session_id);
    return j.session_id.slice(0, 8);
  });

  await mark('consolehub-sessions-end', async () => {
    const list = await getJson(`${P}/api/v1/vms/${PID}/consolehub/sessions`);
    const ids = new Set(sessionIds);
    for (const s of list || []) {
      const sid = s.id || s.session_id;
      if (sid) ids.add(sid);
    }
    let ended = 0;
    for (const sid of ids) {
      const r = await api('POST', `${P}/api/v1/consolehub/sessions/${sid}/end`, {});
      if (ok(r.status)) ended++;
    }
    return `ended=${ended}`;
  });

  // cleanup
  if (keyId) {
    try {
      await api('DELETE', `${P}/api/v1/api-keys/${keyId}`);
    } catch {
      /* ignore */
    }
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`OBS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

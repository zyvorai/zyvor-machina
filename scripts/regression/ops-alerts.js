#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Alerts / backup planning smoke: alert-rules + SOC playbooks CRUD, alert patch,
 * notification deliver, backup targets/schedules, timeline + fleet backups.
 * Does NOT enqueue vm.backup (disk-heavy).
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-alerts');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
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
    log.append({ kind: 'ALERTS', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'ALERTS', api: name, ok: false, note: e.message.slice(0, 220) });
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
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  let ruleId = null;
  let playbookId = null;
  let targetId = null;
  let scheduleId = null;
  let alertId = null;
  let notifId = null;

  await mark('alert-rule-create', async () => {
    const r = await api('POST', `${P}/api/v1/alert-rules`, {
      name: `reg-ar-${SUFFIX}`,
      metric: 'cpu_percent',
      comparator: 'gt',
      threshold: 92,
      severity: 'warning',
      enabled: false,
      cooldown_minutes: 60,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    ruleId = j.id;
    return `id=${j.id.slice(0, 8)} metric=${j.metric}`;
  });

  await mark('alert-rule-list', async () => {
    const j = await getJson(`${P}/api/v1/alert-rules`);
    if (!Array.isArray(j) || !j.some((x) => x.id === ruleId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('alert-rule-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/alert-rules/${ruleId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    ruleId = null;
    return 'deleted';
  });

  await mark('soc-playbook-create', async () => {
    const r = await api('POST', `${P}/api/v1/soc/playbooks`, {
      name: `reg-pb-${SUFFIX}`,
      description: 'regression',
      enabled: false,
      trigger: { min_severity: 'high' },
      steps: [{ action: 'notify' }],
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    playbookId = j.id;
    return `id=${j.id.slice(0, 8)}`;
  });

  await mark('soc-playbook-get', async () => {
    const j = await getJson(`${P}/api/v1/soc/playbooks/${playbookId}`);
    if (j.id !== playbookId) throw new Error('id mismatch');
    return j.name;
  });

  await mark('soc-playbook-patch', async () => {
    const r = await api('PATCH', `${P}/api/v1/soc/playbooks/${playbookId}`, {
      description: 'reg-patched',
      enabled: false,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.description !== 'reg-patched') throw new Error(j.description || 'no desc');
    return 'patched';
  });

  await mark('soc-playbook-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/soc/playbooks/${playbookId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    playbookId = null;
    return 'deleted';
  });

  await mark('soc-playbook-runs', async () => {
    const j = await getJson(`${P}/api/v1/soc/playbook-runs`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('soc-rules', async () => {
    const j = await getJson(`${P}/api/v1/soc/rules`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('soc-alert-get-patch', async () => {
    const list = await getJson(`${P}/api/v1/soc/alerts?limit=5`);
    if (!Array.isArray(list)) throw new Error('alerts not array');
    // A clean host (nothing flagged, e.g. right after a fresh install) legitimately
    // has zero alerts — that's real zero-data state, not a fixture to fabricate.
    if (list.length < 1) return 'soft no alerts to exercise get/patch against';
    alertId = list[0].id;
    const g = await getJson(`${P}/api/v1/soc/alerts/${alertId}`);
    if (g.id !== alertId) throw new Error('get mismatch');
    const prev = g.status;
    const r = await api('PATCH', `${P}/api/v1/soc/alerts/${alertId}`, { status: 'acknowledged' });
    if (!ok(r.status)) throw new Error(`ack ${r.status}`);
    const r2 = await api('PATCH', `${P}/api/v1/soc/alerts/${alertId}`, { status: prev || 'open' });
    if (!ok(r2.status)) throw new Error(`restore ${r2.status}`);
    return `id=${alertId.slice(0, 8)} restored=${prev || 'open'}`;
  });

  await mark('notification-deliver', async () => {
    const list = await getJson(`${P}/api/v1/notifications`);
    if (!Array.isArray(list) || list.length < 1) throw new Error('no notifications');
    notifId = list[0].id;
    const r = await api('POST', `${P}/api/v1/notifications/${notifId}/deliver`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.delivered) throw new Error(JSON.stringify(j).slice(0, 80));
    return `id=${notifId.slice(0, 8)}`;
  });

  await mark('backup-target-create', async () => {
    const r = await api('POST', `${P}/api/v1/backup-targets`, {
      name: `reg-bt-${SUFFIX}`,
      kind: 'local',
      config_json: { path: '/tmp/machina-reg-bak' },
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    targetId = j.id;
    return `id=${j.id.slice(0, 8)} kind=${j.kind}`;
  });

  await mark('backup-target-list', async () => {
    const j = await getJson(`${P}/api/v1/backup-targets`);
    if (!Array.isArray(j) || !j.some((x) => x.id === targetId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('backup-schedule-create', async () => {
    const r = await api('POST', `${P}/api/v1/backup-schedules`, {
      name: `reg-bs-${SUFFIX}`,
      interval_hours: 24,
      retain_count: 3,
      enabled: false,
      target_id: targetId,
      backup_type: 'full',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    scheduleId = j.id;
    return `id=${j.id.slice(0, 8)}`;
  });

  await mark('backup-schedule-list', async () => {
    const j = await getJson(`${P}/api/v1/backup-schedules`);
    if (!Array.isArray(j) || !j.some((x) => x.id === scheduleId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('backup-schedule-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/backup-schedules/${scheduleId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    scheduleId = null;
    return 'deleted';
  });

  await mark('backup-target-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/backup-targets/${targetId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    targetId = null;
    return 'deleted';
  });

  await mark('backups-timeline', async () => {
    const j = await getJson(`${P}/api/v1/backups/timeline`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('vm-backups-list', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/backups`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('fleet-backups', async () => {
    const j = await getJson(`${P}/api/v1/fleet/backups`);
    if (j.total_events == null && !j.summary) throw new Error('empty');
    return (j.summary || `events=${j.total_events}`).toString().slice(0, 80);
  });

  await mark('storage-backup-sla', async () => {
    const j = await getJson(`${P}/api/v1/storage/backup-sla`);
    if (!Array.isArray(j.policies)) throw new Error('no policies');
    return `policies=${j.policies.length}`;
  });

  await mark('operations-showback', async () => {
    const j = await getJson(`${P}/api/v1/operations/showback`);
    if (!Array.isArray(j.lines) || j.lines.length < 1) throw new Error('no lines');
    return `lines=${j.lines.length}`;
  });

  await mark('migrations-list', async () => {
    const j = await getJson(`${P}/api/v1/migrations`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('users-list', async () => {
    const j = await getJson(`${P}/api/v1/users`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  // cleanup leftovers
  if (scheduleId) {
    try {
      await api('DELETE', `${P}/api/v1/backup-schedules/${scheduleId}`);
    } catch {
      /* ignore */
    }
  }
  if (targetId) {
    try {
      await api('DELETE', `${P}/api/v1/backup-targets/${targetId}`);
    } catch {
      /* ignore */
    }
  }
  if (ruleId) {
    try {
      await api('DELETE', `${P}/api/v1/alert-rules/${ruleId}`);
    } catch {
      /* ignore */
    }
  }
  if (playbookId) {
    try {
      await api('DELETE', `${P}/api/v1/soc/playbooks/${playbookId}`);
    } catch {
      /* ignore */
    }
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`ALERTS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

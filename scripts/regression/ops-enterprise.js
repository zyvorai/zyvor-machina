#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Enterprise / maintenance smoke: maintenance schedules, fleet snapshot
 * schedules, notification channels, host cordon, cluster + enterprise reads,
 * network segments/IPAM/canvas, fleet DNA/finder hubs.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-enterprise');
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
    log.append({ kind: 'ENTERPRISE', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'ENTERPRISE', api: name, ok: false, note: e.message.slice(0, 220) });
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

  let maintId = null;
  let snapId = null;
  let channelId = null;

  await mark('maintenance-schedule-create', async () => {
    const r = await api('POST', `${P}/api/v1/maintenance/schedules`, {
      host_id: HID,
      action: 'enter',
      evacuate: false,
      run_at: new Date(Date.now() + 7 * 86400000).toISOString(),
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    maintId = j.id;
    return `id=${j.id.slice(0, 8)} action=${j.action}`;
  });

  await mark('maintenance-schedule-get', async () => {
    const j = await getJson(`${P}/api/v1/maintenance/schedules/${maintId}`);
    if (j.id !== maintId) throw new Error('mismatch');
    return j.status;
  });

  await mark('maintenance-schedule-list', async () => {
    const j = await getJson(`${P}/api/v1/maintenance/schedules`);
    if (!Array.isArray(j) || !j.some((x) => x.id === maintId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('maintenance-schedule-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/maintenance/schedules/${maintId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    maintId = null;
    return 'deleted';
  });

  await mark('fleet-snapshot-schedule-create', async () => {
    const r = await api('POST', `${P}/api/v1/fleet/snapshot-schedules`, {
      name: `reg-fss-${SUFFIX}`,
      cron_expr: '0 3 * * *',
      retain_count: 2,
      disk_only: true,
      quiesce: false,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    snapId = j.id;
    return `id=${j.id.slice(0, 8)}`;
  });

  await mark('fleet-snapshot-schedule-list', async () => {
    const j = await getJson(`${P}/api/v1/fleet/snapshot-schedules`);
    if (!Array.isArray(j) || !j.some((x) => x.id === snapId)) throw new Error('missing');
    return `count=${j.length}`;
  });

  await mark('fleet-snapshot-schedule-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/fleet/snapshot-schedules/${snapId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    snapId = null;
    return 'deleted';
  });

  await mark('notification-channel-create', async () => {
    const r = await api('POST', `${P}/api/v1/notification-channels`, {
      name: `reg-nc-${SUFFIX}`,
      kind: 'webhook',
      target: 'https://example.com/hook',
      enabled: false,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const j = JSON.parse(r.body);
    if (!j.id) throw new Error('no id');
    channelId = j.id;
    return `id=${j.id.slice(0, 8)} kind=${j.kind}`;
  });

  await mark('notification-channel-test', async () => {
    const r = await api('POST', `${P}/api/v1/notification-channels/${channelId}/test`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.queued) throw new Error(JSON.stringify(j).slice(0, 80));
    return 'queued';
  });

  await mark('notification-channel-delete', async () => {
    const r = await api('DELETE', `${P}/api/v1/notification-channels/${channelId}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    channelId = null;
    return 'deleted';
  });

  await mark('host-cordon-on', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/cordon`, { cordon: true });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.schedulable !== false) throw new Error(JSON.stringify(j));
    return 'unschedulable';
  });

  await mark('host-cordon-off', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/cordon`, { cordon: false });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.schedulable !== true) throw new Error(JSON.stringify(j));
    const host = await getJson(`${P}/api/v1/hosts/${HID}`);
    if (host.schedulable === false) throw new Error('still cordoned');
    return 'schedulable';
  });

  await mark('cluster', async () => {
    const j = await getJson(`${P}/api/v1/cluster`);
    if (!j.id && j.host_count == null) throw new Error('empty');
    return `hosts=${j.host_count} vms=${j.vm_count}`;
  });

  await mark('cluster-leadership', async () => {
    const j = await getJson(`${P}/api/v1/cluster/leadership`);
    if (!j.controller_id) throw new Error('empty');
    return `leader=${j.is_leader} id=${j.controller_id}`;
  });

  await mark('cluster-settings', async () => {
    const j = await getJson(`${P}/api/v1/cluster/settings`);
    if (j.placement_policy == null && j.ha_enabled == null) throw new Error('empty');
    return `placement=${j.placement_policy} ha=${j.ha_enabled}`;
  });

  await mark('cert-status', async () => {
    const j = await getJson(`${P}/api/v1/cert-status`);
    if (j.days_remaining == null) throw new Error('empty');
    return `days=${j.days_remaining} expired=${j.expired}`;
  });

  await mark('enterprise-security', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/security/overview`);
    if (j.vault_providers == null) throw new Error('empty');
    return `vault=${j.vault_providers} mfa=${j.mfa_policies}`;
  });

  await mark('enterprise-mfa', async () => {
    const policies = await getJson(`${P}/api/v1/enterprise/mfa/policies`);
    const compliance = await getJson(`${P}/api/v1/enterprise/mfa/compliance`);
    if (!Array.isArray(policies) || policies.length < 1) throw new Error('no policies');
    return `policies=${policies.length} summary=${compliance.summary || ''}`;
  });

  await mark('enterprise-tenants', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/tenants/overview`);
    if (!Array.isArray(j.projects)) throw new Error('no projects');
    return `projects=${j.projects.length}`;
  });

  await mark('enterprise-fips', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/fips/matrix`);
    if (!j.active_profile) throw new Error('empty');
    return j.active_profile;
  });

  await mark('enterprise-vault', async () => {
    const j = await getJson(`${P}/api/v1/enterprise/vault/providers`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `count=${j.length}`;
  });

  await mark('network-segments', async () => {
    const j = await getJson(`${P}/api/v1/network/segments/overview`);
    if (!Array.isArray(j.segments) || j.segments.length < 1) throw new Error('empty');
    return `segments=${j.segments.length}`;
  });

  await mark('network-ipam', async () => {
    const j = await getJson(`${P}/api/v1/network/ipam/pools`);
    if (!Array.isArray(j) || j.length < 1) throw new Error('empty');
    return `pools=${j.length}`;
  });

  await mark('network-canvas', async () => {
    const j = await getJson(`${P}/api/v1/network-canvas`);
    if (!j.topology || !Array.isArray(j.topology.nodes)) throw new Error('empty');
    return `nodes=${j.topology.nodes.length}`;
  });

  await mark('fleet-dna', async () => {
    const j = await getJson(`${P}/api/v1/fleet/dna`);
    if (j.score == null) throw new Error('empty');
    return `score=${j.score} grade=${j.grade}`;
  });

  await mark('fleet-finder', async () => {
    const j = await getJson(`${P}/api/v1/fleet/finder`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('fleet-updates', async () => {
    const j = await getJson(`${P}/api/v1/fleet/updates`);
    if (!j.summary) throw new Error('empty');
    return j.summary.slice(0, 80);
  });

  await mark('soc-asm', async () => {
    const j = await getJson(`${P}/api/v1/soc/asm/summary`);
    if (j.exposure_score == null) throw new Error('empty');
    return `score=${j.exposure_score}`;
  });

  await mark('templates-marketplace', async () => {
    const j = await getJson(`${P}/api/v1/templates/marketplace`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('templates-missing-images', async () => {
    const j = await getJson(`${P}/api/v1/templates/missing-images`);
    if (!Array.isArray(j.missing)) throw new Error('empty');
    return `missing=${j.missing.length}`;
  });

  await mark('guestkit-status', async () => {
    const j = await getJson(`${P}/api/v1/guestkit/status`);
    if (j.enabled == null) throw new Error('empty');
    return `enabled=${j.enabled} ver=${j.library_version || ''}`;
  });

  await mark('host-linux-updates', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/linux/updates`);
    if (!j.backend) throw new Error('empty');
    return `backend=${j.backend} probed=${j.probed}`;
  });

  // cleanup leftovers + ensure uncordoned
  try {
    await api('POST', `${P}/api/v1/hosts/${HID}/cordon`, { cordon: false });
  } catch {
    /* ignore */
  }
  if (maintId) {
    try {
      await api('DELETE', `${P}/api/v1/maintenance/schedules/${maintId}`);
    } catch {
      /* ignore */
    }
  }
  if (snapId) {
    try {
      await api('DELETE', `${P}/api/v1/fleet/snapshot-schedules/${snapId}`);
    } catch {
      /* ignore */
    }
  }
  if (channelId) {
    try {
      await api('DELETE', `${P}/api/v1/notification-channels/${channelId}`);
    } catch {
      /* ignore */
    }
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`ENTERPRISE_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

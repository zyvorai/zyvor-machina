#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Deep Zeus firewall reads + simulate + dry-run profile.
 * Never applies real lockdown / non-dry profile.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-firewallx');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
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
    log.append({ kind: 'FIREWALLX', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'FIREWALLX', api: name, ok: false, note: e.message.slice(0, 220) });
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

  const get = async (label, path, pick) => {
    await mark(label, async () => {
      const r = await api('GET', path);
      if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
      const j = JSON.parse(r.body);
      return pick(j);
    });
  };

  await get('target-score', `${P}/api/v1/zeus-firewall/targets/${HID}/score`, (j) => `score=${j.score}`);
  await get('target-drift', `${P}/api/v1/zeus-firewall/targets/${HID}/drift`, (j) => `drift=${j.drift_detected}`);
  await get('target-activity', `${P}/api/v1/zeus-firewall/targets/${HID}/activity`, (j) => `blocked=${j.blocked_today}`);
  await get('target-timeline', `${P}/api/v1/zeus-firewall/targets/${HID}/timeline`, (j) => `n=${(Array.isArray(j) ? j : []).length}`);
  await get('target-ports', `${P}/api/v1/zeus-firewall/targets/${HID}/ports`, (j) => `n=${(Array.isArray(j) ? j : []).length}`);
  await get('target-services', `${P}/api/v1/zeus-firewall/targets/${HID}/services`, (j) => `n=${(Array.isArray(j) ? j : []).length}`);

  await mark('simulate-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/simulate`, { profile: 'ProductionServer' });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('simulate', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/simulate`, {
      target_id: HID,
      profile: 'ProductionServer',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `entries=${((j.diff && j.diff.entries) || []).length}`;
  });

  await get('guest-ports', `${P}/api/v1/zeus-firewall/vms/${PID}/guest-ports`, (j) => `ports=${(j.ports || []).length}`);
  await get('anomalies', `${P}/api/v1/zeus-firewall/anomalies`, (j) => `n=${(j.anomalies || []).length}`);
  await get('siem-export', `${P}/api/v1/zeus-firewall/siem/export`, (j) => `events=${j.event_count}`);

  await mark('profile-dry-run', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/targets/${HID}/profile`, {
      profile: 'ProductionServer',
      dry_run: true,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.dry_run === false && j.applied === true) throw new Error('profile applied unexpectedly');
    return `entries=${((j.diff && j.diff.entries) || j.operations || []).length || 'ok'}`;
  });

  await mark('lockdown-dry-run', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/targets/${HID}/lockdown`, { dry_run: true });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.dry_run !== true) throw new Error('expected dry_run');
    return 'gated';
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`FIREWALLX_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

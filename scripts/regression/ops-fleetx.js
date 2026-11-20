#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Fleet hub overviews + live storage/network discover smoke.
 * Avoids: host maintenance, evacuate, real migrate, Atlas mutate.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-fleetx');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
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
    log.append({ kind: 'FLEETX', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'FLEETX', api: name, ok: false, note: e.message.slice(0, 220) });
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

  const getSummary = async (label, path) => {
    await mark(label, async () => {
      const r = await api('GET', path);
      if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
      const j = JSON.parse(r.body);
      return String(j.summary || `keys=${Object.keys(j).length}`).slice(0, 80);
    });
  };

  await getSummary('fleet-general', `${P}/api/v1/fleet/general`);
  await getSummary('fleet-shortcuts', `${P}/api/v1/fleet/shortcuts`);
  await getSummary('fleet-users', `${P}/api/v1/fleet/users`);
  await getSummary('fleet-network', `${P}/api/v1/fleet/network`);
  await getSummary('fleet-storage', `${P}/api/v1/fleet/storage`);
  await getSummary('fleet-console', `${P}/api/v1/fleet/console`);
  await getSummary('fleet-updates', `${P}/api/v1/fleet/updates`);
  await getSummary('fleet-spaces', `${P}/api/v1/fleet/spaces`);
  await getSummary('fleet-dna', `${P}/api/v1/fleet/dna`);
  await mark('fleet-mission', async () => {
    const r = await api('GET', `${P}/api/v1/fleet/mission`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    const s = typeof j.summary === 'string' ? j.summary : JSON.stringify(j.summary || j).slice(0, 80);
    return s;
  });

  await mark('fleet-linux-health', async () => {
    const r = await api('GET', `${P}/api/v1/fleet/linux-health`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `scanned=${j.hosts_scanned} pressure=${j.pressure_hosts}`;
  });

  await mark('fleet-finder', async () => {
    const r = await api('GET', `${P}/api/v1/fleet/finder`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return String(j.summary || `keys=${Object.keys(j).length}`).slice(0, 80);
  });

  await mark('fleet-keychain', async () => {
    const r = await api('GET', `${P}/api/v1/fleet/keychain`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return String(j.summary || `keys=${Object.keys(j).length}`).slice(0, 80);
  });

  await mark('storage-pools-live', async () => {
    const r = await api('GET', `${P}/api/v1/storage/pools/live`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `pools=${(j.pools || []).length}`;
  });

  await mark('storage-pools-discover', async () => {
    const r = await api('POST', `${P}/api/v1/storage/pools/discover`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `imported=${j.imported}`;
  });

  await mark('networks-live', async () => {
    const r = await api('GET', `${P}/api/v1/networks/live`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `networks=${(j.networks || []).length}`;
  });

  await mark('networks-discover', async () => {
    const r = await api('POST', `${P}/api/v1/networks/discover`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `imported=${j.imported}`;
  });

  await mark('ai-fleet-summary', async () => {
    const r = await api('GET', `${P}/api/v1/ai/fleet/summary`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `clusters=${(j.clusters || []).length}`;
  });

  await mark('ai-fleet-local', async () => {
    const r = await api('GET', `${P}/api/v1/ai/fleet/local`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `vms=${j.vm_count} reachable=${j.reachable}`;
  });

  await mark('launchpad-unavailable', async () => {
    const r = await api('GET', `${P}/api/v1/launchpad/tiles`);
    if (r.status === 503 && /launchpad/i.test(r.body || '')) return 'unavailable';
    if (r.status === 404) return 'skipped (route absent)';
    if (ok(r.status)) return 'configured';
    throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`FLEETX_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

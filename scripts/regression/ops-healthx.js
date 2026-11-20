#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Health / doctor / diagnose / guest-agent smoke.
 * Guest mutate (sync-time/fstrim) expects agent-down failures.
 * Avoids: host reboot, evacuate, migrate POST, lockdown apply.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-healthx');
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
    log.append({ kind: 'HEALTHX', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'HEALTHX', api: name, ok: false, note: e.message.slice(0, 220) });
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

  await mark('vm-doctor', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/doctor`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `score=${j.score} n=${j.score_n}`;
  });

  await mark('vm-diagnose', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/diagnose`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `vm=${j.vm_name || j.vm_id}`;
  });

  await mark('vm-health-check', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/health-check`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `score=${j.score}`;
  });

  await mark('guest-health', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/guest/health`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `agent=${j.agent_reachable}`;
  });

  await mark('guest-services', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/guest/services`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `agent=${j.agent_reachable} services=${(j.services || []).length}`;
  });

  await mark('guest-sync-time-expected', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/guest/sync-time`, {});
    if (ok(r.status)) return 'ok';
    if (/guest agent|guestkit|not responding/i.test(r.body || '')) return `${r.status}-expected`;
    throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
  });

  await mark('guest-fstrim-expected', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/guest/fstrim`, {});
    if (ok(r.status)) return 'ok';
    if (/guestkit|guest-fstrim|guest agent/i.test(r.body || '')) return `${r.status}-expected`;
    throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
  });

  await mark('host-detail', async () => {
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/detail`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `state=${j.state} host=${j.hostname}`;
  });

  await mark('host-gpus', async () => {
    const r = await api('GET', `${P}/api/v1/hosts/${HID}/gpus`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `devices=${(j.devices || []).length}`;
  });

  await mark('host-health-check', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/health-check`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `ok=${j.ok} checks=${(j.checks || []).length}`;
  });

  await mark('ai-troubleshoot', async () => {
    const r = await api('POST', `${P}/api/v1/ai/troubleshoot`, { vm_id: PID });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `severity=${j.severity} vm=${j.vm_name}`;
  });

  await mark('ai-nl-ops', async () => {
    const r = await api('POST', `${P}/api/v1/ai/nl-ops`, { query: 'list vms' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `intent=${j.intent} dry_run=${j.dry_run}`;
  });

  await mark('ai-predictions', async () => {
    const r = await api('GET', `${P}/api/v1/ai/predictions`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `n=${(j.predictions || []).length}`;
  });

  await mark('ai-sre-forecast', async () => {
    const r = await api('GET', `${P}/api/v1/ai/sre/forecast`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `n=${(j.forecasts || []).length}`;
  });

  await mark('ai-autopilot-propose', async () => {
    const r = await api('GET', `${P}/api/v1/ai/autopilot/propose`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `mode=${j.mode} actions=${(j.actions || []).length}`;
  });

  await mark('cloud-init-invalid', async () => {
    const r = await api('POST', `${P}/api/v1/cloud-init/validate`, { user_data: 'invalid: [' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.valid !== false) throw new Error('expected invalid');
    return `issues=${(j.issues || []).length}`;
  });

  await mark('secrets-schema-negative', async () => {
    const r = await api('POST', '/api/v1/secrets', {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`HEALTHX_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

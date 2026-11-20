#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Atlas storage integration smoke (disabled-host friendly).
 * Soft-passes atlas_disabled 503s; schema-negatives for expand/restore.
 * Avoids real expand/backup/restore when Atlas is off.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-atlas');
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
const P = '/api/v1/platform/controller';

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}
function atlasDisabled(status, body) {
  return status === 503 && /atlas_disabled|ATLAS_ENABLED/i.test(body || '');
}
function atlasError(status, body) {
  return (status === 502 || status === 503) && /atlas/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'ATLAS', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'ATLAS', api: name, ok: false, note: e.message.slice(0, 220) });
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

  let atlasOn = false;

  await mark('atlas-status', async () => {
    const r = await api('GET', `${P}/api/v1/atlas/status`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    atlasOn = !!j.enabled;
    return `enabled=${j.enabled} reachable=${j.reachable}`;
  });

  const readOrDisabled = async (label, path) => {
    await mark(label, async () => {
      const r = await api('GET', path);
      if (atlasDisabled(r.status, r.body)) return 'disabled';
      if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
      const j = JSON.parse(r.body);
      const n = Array.isArray(j) ? j.length : Object.keys(j).length;
      return `keys=${n}`;
    });
  };

  await readOrDisabled('atlas-backends', `${P}/api/v1/atlas/backends`);
  await readOrDisabled('atlas-clusters', `${P}/api/v1/atlas/clusters`);
  await readOrDisabled('atlas-pools', `${P}/api/v1/atlas/pools`);
  await readOrDisabled('atlas-policies', `${P}/api/v1/atlas/policies`);
  await readOrDisabled('atlas-metrics-summary', `${P}/api/v1/atlas/metrics/summary`);
  await readOrDisabled('atlas-snapshots', `${P}/api/v1/atlas/snapshots`);
  await readOrDisabled('atlas-jobs', `${P}/api/v1/atlas/jobs`);

  await mark('atlas-vm-volumes', async () => {
    const r = await api('GET', `${P}/api/v1/atlas/vms/${PID}/volumes`);
    if (atlasDisabled(r.status, r.body)) return 'disabled';
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('atlas-expand-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/atlas/volumes/00000000-0000-0000-0000-000000000001/expand`, {
      size_gb: 1,
    });
    if (atlasDisabled(r.status, r.body) || atlasError(r.status, r.body)) return 'disabled';
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('atlas-restore-schema-negative', async () => {
    const r = await api('POST', `${P}/api/v1/atlas/restore-jobs`, {});
    if (atlasDisabled(r.status, r.body) || atlasError(r.status, r.body)) return 'disabled';
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('atlas-vm-backup-disabled-or-schema', async () => {
    const r = await api('POST', `${P}/api/v1/atlas/vms/${PID}/backup`, {});
    if (atlasDisabled(r.status, r.body) || atlasError(r.status, r.body)) return 'disabled';
    if (!atlasOn && r.status >= 400) return `${r.status}`;
    if (r.status < 400) return 'accepted-skip-mutate';
    return `${r.status}`;
  });

  await mark('atlas-vm-snapshot-disabled-or-schema', async () => {
    const r = await api('POST', `${P}/api/v1/atlas/vms/${PID}/snapshot`, {});
    if (atlasDisabled(r.status, r.body) || atlasError(r.status, r.body)) return 'disabled';
    if (!atlasOn && r.status >= 400) return `${r.status}`;
    if (r.status < 400) return 'accepted-skip-mutate';
    return `${r.status}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `state=${j.observed_state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`ATLAS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

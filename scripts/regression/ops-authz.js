#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Auth / enterprise edges: OIDC, MFA policy upsert, api-keys, enrollment, cert.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-authz');
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
    log.append({ kind: 'AUTHZ', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'AUTHZ', api: name, ok: false, note: e.message.slice(0, 220) });
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

  let keyId = null;
  let enrollToken = null;

  await mark('auth-oidc', async () => {
    const r = await api('GET', `${P}/api/v1/auth/oidc`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `enabled=${j.enabled}`;
  });

  await mark('mfa-policies', async () => {
    const r = await api('GET', `${P}/api/v1/enterprise/mfa/policies`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('mfa-policy-upsert-viewer', async () => {
    const r = await api('POST', `${P}/api/v1/enterprise/mfa/policies/viewer`, {
      method: 'totp',
      required: false,
      grace_days: 7,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `role=${j.role_name} method=${j.method}`;
  });

  await mark('cert-status', async () => {
    const r = await api('GET', `${P}/api/v1/cert-status`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `days=${j.days_remaining}`;
  });

  await mark('enrollment-list', async () => {
    const r = await api('GET', `${P}/api/v1/enrollment/tokens`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('enrollment-create-delete', async () => {
    let r = await api('POST', `${P}/api/v1/enrollment/tokens`, { ttl_hours: 1 });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`create ${r.status}`);
    const j = JSON.parse(r.body);
    enrollToken = j.token || j.id;
    if (!enrollToken) throw new Error('no token');
    r = await api('DELETE', `${P}/api/v1/enrollment/tokens/${encodeURIComponent(enrollToken)}`);
    if (!ok(r.status) && r.status !== 204) throw new Error(`delete ${r.status}`);
    enrollToken = null;
    return 'ok';
  });

  await mark('api-keys-rotate', async () => {
    let r = await api('POST', `${P}/api/v1/api-keys`, { name: `reg-authz-${SUFFIX}` });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`create ${r.status}`);
    const created = JSON.parse(r.body);
    keyId = created.id;
    if (!keyId) throw new Error('no id');
    r = await api('POST', `${P}/api/v1/api-keys/${keyId}/rotate`, {});
    if (!ok(r.status)) throw new Error(`rotate ${r.status}`);
    r = await api('DELETE', `${P}/api/v1/api-keys/${keyId}`);
    if (!ok(r.status) && r.status !== 204) throw new Error(`delete ${r.status}`);
    keyId = null;
    return 'rotated+deleted';
  });

  await mark('users-me', async () => {
    const r = await api('GET', `${P}/api/v1/users/me`);
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    return `user=${j.username || j.name || j.id}`;
  });

  await mark('postcheck-vm', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}`);
    if (!ok(r.status)) throw new Error(`${r.status}`);
    return `state=${JSON.parse(r.body).observed_state}`;
  });

  if (keyId) {
    try {
      await api('DELETE', `${P}/api/v1/api-keys/${keyId}`);
    } catch {
      /* ignore */
    }
  }
  if (enrollToken) {
    try {
      await api('DELETE', `${P}/api/v1/enrollment/tokens/${encodeURIComponent(enrollToken)}`);
    } catch {
      /* ignore */
    }
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`AUTHZ_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

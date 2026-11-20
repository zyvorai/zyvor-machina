// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Resolve live platform host / VM UUIDs after login.
 * Prefer MACHINA_HOST_ID / MACHINA_PLATFORM_HOST_ID / MACHINA_PLATFORM_VM_ID;
 * otherwise pick from controller inventory (first host; VM matching cfg.vmName).
 */

const P = '/api/v1/platform/controller';

async function resolveIds(api, cfg = {}) {
  let hostId =
    cfg.hostId ||
    process.env.MACHINA_HOST_ID ||
    process.env.MACHINA_PLATFORM_HOST_ID ||
    '';
  let platformVmId = cfg.platformVmId || process.env.MACHINA_PLATFORM_VM_ID || '';
  let storagePoolId =
    cfg.storagePoolId ||
    process.env.MACHINA_STORAGE_POOL_ID ||
    process.env.MACHINA_POOL_ID ||
    '';
  const vmName = cfg.vmName || process.env.MACHINA_VM_NAME || '';

  if (!hostId) {
    const r = await api('GET', `${P}/api/v1/hosts`);
    if (r.status < 200 || r.status >= 400) {
      throw new Error(`hosts list ${r.status}`);
    }
    const body = JSON.parse(r.body || '[]');
    const list = Array.isArray(body) ? body : body.items || [];
    if (!list.length) throw new Error('no platform hosts — set MACHINA_HOST_ID');
    hostId = list[0].id;
  }

  if (!platformVmId && vmName) {
    const r = await api('GET', `${P}/api/v1/vms`);
    if (r.status >= 200 && r.status < 400) {
      const body = JSON.parse(r.body || '[]');
      const list = Array.isArray(body) ? body : body.items || [];
      const matches = list.filter((v) => v.name === vmName);
      if (matches.length) {
        matches.sort((a, b) => {
          const ta = a.last_seen_at ? Date.parse(a.last_seen_at) : 0;
          const tb = b.last_seen_at ? Date.parse(b.last_seen_at) : 0;
          return tb - ta;
        });
        platformVmId = matches[0].id;
      }
    }
  }

  if (!storagePoolId) {
    const r = await api('GET', `${P}/api/v1/storage/pools`);
    if (r.status >= 200 && r.status < 400) {
      const body = JSON.parse(r.body || '[]');
      const list = Array.isArray(body) ? body : body.items || [];
      if (list.length) storagePoolId = list[0].id;
    }
  }

  return { hostId, platformVmId, storagePoolId, vmName };
}

module.exports = { resolveIds, P };

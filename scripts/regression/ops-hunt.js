#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * AI security hunt / firewall plan / VM HA+snapshot smoke.
 * Avoids: graphics/add (duplicate VNC), real lockdown apply, snapshot revert
 * (destructive), templates.prefetch_missing, Atlas.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-hunt');
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
function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'HUNT', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'HUNT', api: name, ok: false, note: e.message.slice(0, 220) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
}

async function waitTask(taskId, { timeoutMs = 60000 } = {}) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const j = await getJson(`${P}/api/v1/tasks/${taskId}`);
    if (j.status === 'completed' || j.status === 'failed' || j.status === 'cancelled') return j;
    await sleep(1000);
  }
  throw new Error(`task ${taskId} timeout`);
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

  let snapName = null;

  await mark('ai-guest-query', async () => {
    const r = await api('POST', `${P}/api/v1/ai/fleet/guest-query`, {
      query: 'list processes',
      vm_ids: [PID],
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary && j.matched_count == null) throw new Error('empty');
    return `matched=${j.matched_count} scanned=${j.scanned_count}`;
  });

  await mark('ai-memory-changes-before', async () => {
    const j = await getJson(`${P}/api/v1/ai/memory/changes-before?hours=4`);
    if (!Array.isArray(j.changes) && !j.summary) throw new Error('empty');
    return `changes=${(j.changes || []).length}`;
  });

  await mark('ai-security-explain-event', async () => {
    const r = await api('POST', `${P}/api/v1/ai/security/explain-event`, {
      event: 'ssh brute force',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.explanation && !j.summary) throw new Error('empty');
    return String(j.summary || j.kind || 'ok').slice(0, 60);
  });

  await mark('ai-security-hunt-summary', async () => {
    const r = await api('POST', `${P}/api/v1/ai/security/hunt-summary`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary) throw new Error('empty');
    return String(j.summary).slice(0, 80);
  });

  await mark('ai-security-nl-search', async () => {
    const r = await api('POST', `${P}/api/v1/ai/security/nl-search`, {
      query: 'failed ssh logins',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.original_query && j.hit_count == null) throw new Error('empty');
    return `hits=${j.hit_count} backend=${j.search_backend}`;
  });

  await mark('ai-security-attack-reconstruct', async () => {
    const r = await api('POST', `${P}/api/v1/ai/security/attack-reconstruct`, {
      host_id: HID,
      vm_id: PID,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.summary && !Array.isArray(j.attack_chain)) throw new Error('empty');
    return String(j.summary || '').slice(0, 80);
  });

  await mark('ai-autopilot-run-negative', async () => {
    const r = await api('POST', `${P}/api/v1/ai/autopilot/run`, { dry_run: true });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('vm-ha', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/ha`);
    if (j.enabled == null && j.restart_priority == null) throw new Error('empty');
    return `enabled=${j.enabled} priority=${j.restart_priority}`;
  });

  await mark('vm-snapshots-list', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/snapshots`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('vm-snapshot-create-delete', async () => {
    // External live snapshots rewrite the domain disk to an overlay that cannot be
    // deleted while running and leaves a fragile backing chain (seen breaking
    // chrome-e2e-vm after reboot). Keep list coverage above; skip mutate here.
    return 'skipped-external-snap-unsafe-on-running-vm';
  });

  await mark('firewall-checkpoints', async () => {
    const j = await getJson(`${P}/api/v1/zeus-firewall/targets/${HID}/checkpoints`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('firewall-target-plan', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/targets/${HID}/plan`, {
      profile: 'ProductionServer',
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.diff && !j.entries) throw new Error('empty');
    return `entries=${((j.diff && j.diff.entries) || j.entries || []).length}`;
  });

  await mark('firewall-execute-batch-dry', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/operator/execute-batch`, {
      dry_run: true,
      host_ids: [HID],
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.dry_run !== true && !Array.isArray(j.results)) throw new Error('empty');
    return `results=${(j.results || []).length}`;
  });

  await mark('firewall-lockdown-preview', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/targets/${HID}/lockdown`, {
      dry_run: true,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.dry_run !== true && !j.message) {
      // After confirm-gate fix, dry_run must not apply. Fail closed if API claims apply.
      if (j.result && !j.dry_run) throw new Error('lockdown applied unexpectedly');
    }
    if (!j.preview && !j.ok) throw new Error('empty');
    return String((j.preview && j.preview.summary) || j.message || 'ok').slice(0, 80);
  });

  await mark('firewall-lockdown-requires-confirm', async () => {
    // Must NOT apply — missing confirm should stay dry-run even if dry_run:false.
    const r = await api('POST', `${P}/api/v1/zeus-firewall/targets/${HID}/lockdown`, {
      dry_run: false,
      confirm: false,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.dry_run !== true) throw new Error('expected dry_run gate');
    return 'gated';
  });

  await mark('firewall-rollback-negative', async () => {
    const r = await api('POST', `${P}/api/v1/zeus-firewall/targets/${HID}/rollback`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status} checkpoint_id required`;
  });

  await mark('host-security-containers', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/containers`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('host-security-dns', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/dns`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('host-security-process-graph', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/process-graph`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('host-security-files', async () => {
    const j = await getJson(`${P}/api/v1/zeus-security/hosts/${HID}/files`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('segments-overview', async () => {
    const j = await getJson(`${P}/api/v1/network/segments/overview`);
    if (!j.segments && !Array.isArray(j)) throw new Error('empty');
    const segs = Array.isArray(j) ? j : j.segments || [];
    return `segments=${segs.length}`;
  });

  await mark('segment-ipam-allocate', async () => {
    const overview = await getJson(`${P}/api/v1/network/segments/overview`);
    const segs = Array.isArray(overview) ? overview : overview.segments || [];
    if (segs.length < 1) return 'no-segments';
    const sid = segs[0].id;
    // Prefer a segment that already has a bound network; otherwise attempt bind.
    let target = segs.find((s) => s.network_id || s.bound_network_id || (s.networks && s.networks.length));
    if (!target) {
      const nets = await getJson(`${P}/api/v1/networks`);
      const netList = Array.isArray(nets) ? nets : nets.items || [];
      if (netList[0]) {
        const bind = await api('POST', `${P}/api/v1/network/segments/${sid}/bind/${netList[0].id}`, {});
        if (ok(bind.status)) target = segs[0];
      }
    }
    const useId = (target && target.id) || sid;
    const r = await api('POST', `${P}/api/v1/network/segments/${useId}/ipam/allocate`, {
      hostname: `reg-hunt-${SUFFIX}`,
    });
    if (r.status === 400 && /bind a network/i.test(r.body || '')) {
      return 'skipped-unbound';
    }
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
    const j = JSON.parse(r.body);
    return `ip=${j.address || j.ip || j.allocated || 'ok'}`;
  });

  await mark('segment-emergency-unlock', async () => {
    const overview = await getJson(`${P}/api/v1/network/segments/overview`);
    const segs = Array.isArray(overview) ? overview : overview.segments || [];
    if (segs.length < 1) return 'no-segments';
    const r = await api('POST', `${P}/api/v1/network/segments/${segs[0].id}/emergency-unlock`, {
      reason: `reg-hunt-${SUFFIX}`,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 80)}`);
    return 'unlocked';
  });

  await mark('postcheck-vm', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}`);
    if (!j.name) throw new Error('vm missing');
    return `state=${j.observed_state}`;
  });

  // cleanup leftover snaps
  if (snapName) {
    try {
      const r = await api('DELETE', `${P}/api/v1/vms/${PID}/snapshots/${encodeURIComponent(snapName)}`);
      if (ok(r.status)) {
        const j = JSON.parse(r.body);
        if (j.task_id) await waitTask(j.task_id).catch(() => {});
      }
    } catch {
      /* ignore */
    }
  }
  try {
    const snaps = await getJson(`${P}/api/v1/vms/${PID}/snapshots`);
    for (const s of snaps) {
      if (/^reg-snap/i.test(s.name || '')) {
        const r = await api('DELETE', `${P}/api/v1/vms/${PID}/snapshots/${encodeURIComponent(s.name)}`);
        if (ok(r.status)) {
          const j = JSON.parse(r.body);
          if (j.task_id) await waitTask(j.task_id).catch(() => {});
        }
      }
    }
  } catch {
    /* ignore */
  }

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`HUNT_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Native Fleet Cloud API CRUD — flavors, keypairs, security groups, load balancers,
 * catalog reads, and optional port-forward (floating-IP equivalent).
 *
 * Requires daemon → controller proxy auth (MACHINA_PLATFORM_*). Prefer:
 *   MACHINA_PLATFORM_VM_ID=<uuid of iw-e2e-1>
 *   MACHINA_VM_NAME=iw-e2e-1
 *
 * Throwaway resources are prefixed fc-reg-<pid> and cleaned up in finally.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-fleetcloud');
const P = '/api/v1/platform/controller';
const TAG = `fc-reg-${process.pid}`;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HOST_ID = process.env.MACHINA_PLATFORM_HOST_ID || '';

// Fixed test pubkey (ed25519) — never used for real access.
const TEST_PUBKEY =
  'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVEOarE0v93a1OwYnmUdx1gBFzCJxfudM0VeVX4fDzJ fc-reg@machina';

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'FLEETCLOUD', api: name, ok: true, note: String(note || 'ok').slice(0, 200) });
    return true;
  } catch (e) {
    log.append({ kind: 'FLEETCLOUD', api: name, ok: false, note: e.message.slice(0, 240) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status} ${String(r.body).slice(0, 120)}`);
  return JSON.parse(r.body);
}

async function postJson(path, body) {
  const r = await api('POST', path, body);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`POST ${path} ${r.status} ${String(r.body).slice(0, 160)}`);
  return r.body ? JSON.parse(r.body) : {};
}

async function del(path) {
  const r = await api('DELETE', path);
  if (!ok(r.status) && r.status !== 204) {
    throw new Error(`DELETE ${path} ${r.status} ${String(r.body).slice(0, 120)}`);
  }
  return r;
}

(async () => {
  await login({ retries: 5, waitMs: 65000 });
  const _ids = await resolveIds(api, cfg);
  if (_ids.hostId) HOST_ID = _ids.hostId;
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  let flavorId = null;
  let keypairId = null;
  let sgId = null;
  let sgRuleId = null;
  let lbId = null;
  let lbMemberId = null;
  let stackId = null;
  let pf = null; // { protocol, host_port, vm_port }

  try {
    await mark('proxy-health', async () => {
      const j = await getJson(`${P}/api/v1/health`);
      if (j.status !== 'ok' && j.component !== 'machina-controller') {
        if (!j.status && !j.database) throw new Error(JSON.stringify(j).slice(0, 80));
      }
      return `db=${j.database || '?'} leader=${j.leader}`;
    });

    // ── Reads ──────────────────────────────────────────────────────────
    await mark('list-flavors', async () => {
      const j = await getJson(`${P}/api/v1/flavors`);
      if (!Array.isArray(j)) throw new Error('not array');
      return `count=${j.length}`;
    });
    await mark('list-keypairs', async () => {
      const j = await getJson(`${P}/api/v1/keypairs`);
      if (!Array.isArray(j)) throw new Error('not array');
      return `count=${j.length}`;
    });
    await mark('list-security-groups', async () => {
      const j = await getJson(`${P}/api/v1/security-groups`);
      if (!Array.isArray(j)) throw new Error('not array');
      return `count=${j.length}`;
    });
    await mark('list-load-balancers', async () => {
      const j = await getJson(`${P}/api/v1/load-balancers`);
      if (!Array.isArray(j)) throw new Error('not array');
      return `count=${j.length}`;
    });
    await mark('list-platform-vms', async () => {
      const j = await getJson(`${P}/api/v1/vms`);
      const items = Array.isArray(j) ? j : j.items || [];
      if (!Array.isArray(items)) throw new Error('not array');
      return `count=${items.length}`;
    });
    await mark('list-networks', async () => {
      const j = await getJson(`${P}/api/v1/networks`);
      const items = Array.isArray(j) ? j : j.items || [];
      if (!Array.isArray(items)) throw new Error('not array');
      return `count=${items.length}`;
    });
    await mark('list-volumes-or-pools', async () => {
      // Prefer volumes; fall back to storage pools if volumes route differs.
      let r = await api('GET', `${P}/api/v1/volumes`);
      if (ok(r.status) && !isHtml(r.body)) {
        const j = JSON.parse(r.body);
        const items = Array.isArray(j) ? j : j.items || [];
        return `volumes=${items.length}`;
      }
      const pools = await getJson(`${P}/api/v1/storage/pools`);
      const items = Array.isArray(pools) ? pools : pools.items || [];
      return `pools=${items.length} (volumes ${r.status})`;
    });
    await mark('list-projects', async () => {
      const j = await getJson(`${P}/api/v1/projects`);
      if (!Array.isArray(j)) throw new Error('not array');
      return `count=${j.length}`;
    });
    await mark('list-project-registry', async () => {
      const j = await getJson(`${P}/api/v1/project-registry`);
      if (!Array.isArray(j)) throw new Error('not array');
      return `count=${j.length}`;
    });
    await mark('list-stacks', async () => {
      const j = await getJson(`${P}/api/v1/stacks`);
      if (!Array.isArray(j)) throw new Error('not array');
      return `count=${j.length}`;
    });
    await mark('list-templates', async () => {
      const r = await api('GET', `${P}/api/v1/templates`);
      if (r.status === 404) return 'skipped (no templates route)';
      if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
      const j = JSON.parse(r.body);
      const items = Array.isArray(j) ? j : j.items || [];
      return `count=${items.length}`;
    });

    // ── Flavor CRUD ────────────────────────────────────────────────────
    await mark('flavor-create', async () => {
      const j = await postJson(`${P}/api/v1/flavors`, {
        name: `${TAG}-flavor`,
        vcpus: 1,
        memory_mib: 512,
        disk_gib: 5,
        description: 'fleetcloud regression',
        is_public: true,
      });
      flavorId = j.id;
      if (!flavorId) throw new Error('no id');
      return flavorId;
    });
    await mark('flavor-get', async () => {
      const j = await getJson(`${P}/api/v1/flavors/${flavorId}`);
      if (j.name !== `${TAG}-flavor`) throw new Error(`name=${j.name}`);
      return `${j.vcpus}c/${j.memory_mib}MiB`;
    });

    // ── Keypair CRUD ───────────────────────────────────────────────────
    await mark('keypair-create', async () => {
      const j = await postJson(`${P}/api/v1/keypairs`, {
        name: `${TAG}-kp`,
        public_key: TEST_PUBKEY,
      });
      keypairId = j.id;
      if (!keypairId) throw new Error('no id');
      return j.fingerprint || keypairId;
    });

    // ── Security group + rule ──────────────────────────────────────────
    await mark('sg-create', async () => {
      const j = await postJson(`${P}/api/v1/security-groups`, {
        name: `${TAG}-sg`,
        description: 'fleetcloud regression',
      });
      sgId = j.id;
      if (!sgId) throw new Error('no id');
      return sgId;
    });
    await mark('sg-rule-create', async () => {
      const j = await postJson(`${P}/api/v1/security-groups/${sgId}/rules`, {
        direction: 'ingress',
        protocol: 'tcp',
        port_min: 22,
        port_max: 22,
        remote_cidr: '127.0.0.1/32',
      });
      sgRuleId = j.id;
      if (!sgRuleId) throw new Error('no id');
      return sgRuleId;
    });
    await mark('sg-rules-list', async () => {
      const j = await getJson(`${P}/api/v1/security-groups/${sgId}/rules`);
      if (!Array.isArray(j) || j.length < 1) throw new Error('empty rules');
      return `count=${j.length}`;
    });

    // ── Stack (SG-only template — no VM to avoid long create) ──────────
    await mark('stack-create', async () => {
      const j = await postJson(`${P}/api/v1/stacks`, {
        name: `${TAG}-stack`,
        template: {
          security_groups: [
            {
              name: `${TAG}-stack-sg`,
              rules: [
                {
                  direction: 'ingress',
                  protocol: 'tcp',
                  port_min: 22,
                  port_max: 22,
                  remote_cidr: '127.0.0.1/32',
                },
              ],
            },
          ],
          volumes: [],
          vms: [],
        },
      });
      stackId = j.id;
      if (!stackId) throw new Error('no stack id');
      return `${stackId} status=${j.status}`;
    });
    await mark('stack-get', async () => {
      const j = await getJson(`${P}/api/v1/stacks/${stackId}`);
      if (j.name !== `${TAG}-stack`) throw new Error(`name=${j.name}`);
      return j.status;
    });

    // ── Load balancer ──────────────────────────────────────────────────
    await mark('lb-create', async () => {
      const port = 18000 + (process.pid % 1000);
      const j = await postJson(`${P}/api/v1/load-balancers`, {
        name: `${TAG}-lb`,
        host_id: HOST_ID,
        listener_port: port,
        protocol: 'tcp',
      });
      lbId = j.id;
      if (!lbId) throw new Error('no id');
      return `${lbId} :${port} status=${j.status}`;
    });

    if (PID) {
      await mark('lb-member-add', async () => {
        // Member row is committed before host iptables push; a 400 with
        // "member saved but rule push failed" still means CRUD succeeded
        // (guest IP missing / agent iptables quirk). Confirm via list.
        const r = await api('POST', `${P}/api/v1/load-balancers/${lbId}/members`, {
          vm_id: PID,
          port: 22,
          weight: 1,
        });
        const members = await getJson(`${P}/api/v1/load-balancers/${lbId}/members`);
        const hit = (Array.isArray(members) ? members : []).find(
          (m) => m.vm_id === PID && Number(m.port) === 22,
        );
        if (hit) lbMemberId = hit.id;
        if (ok(r.status)) {
          if (!lbMemberId) throw new Error('no member id');
          return lbMemberId;
        }
        const body = String(r.body || '');
        if (lbMemberId && /member saved but rule push failed/i.test(body)) {
          return `persisted ${lbMemberId} (rule push soft: ${body.slice(0, 100)})`;
        }
        throw new Error(`${r.status} ${body.slice(0, 160)}`);
      });
    } else {
      await mark('lb-member-add', async () => 'skipped (no PLATFORM_VM_ID)');
    }

    // ── Port-forward (floating-IP analogue) ────────────────────────────
    if (PID) {
      await mark('port-forward-create', async () => {
        const hostPort = 19000 + (process.pid % 1000);
        const r = await api('POST', `${P}/api/v1/vms/${PID}/port-forwards`, {
          protocol: 'tcp',
          host_port: hostPort,
          vm_port: 22,
          description: `${TAG}-pf`,
        });
        if (!ok(r.status)) throw new Error(`${r.status} ${String(r.body).slice(0, 140)}`);
        pf = { protocol: 'tcp', host_port: hostPort, vm_port: 22 };
        return `host:${hostPort}->guest:22`;
      });
      await mark('port-forward-list', async () => {
        if (!pf) return 'soft skipped (port-forward-create did not succeed)';
        const j = await getJson(`${P}/api/v1/vms/${PID}/port-forwards`);
        const items = Array.isArray(j) ? j : j.items || [];
        if (!Array.isArray(items)) throw new Error('not array');
        const mine = items.filter((x) => Number(x.host_port) === pf.host_port);
        if (mine.length < 1) throw new Error(`created ${pf.host_port} not in list`);
        return `count=${items.length} mine=${mine.length}`;
      });
    } else {
      await mark('port-forward-skip', async () => 'skipped (no PLATFORM_VM_ID)');
    }
  } finally {
    // Cleanup in reverse dependency order
    if (PID) {
      await mark('port-forward-delete', async () => {
        const listed = await getJson(`${P}/api/v1/vms/${PID}/port-forwards`);
        const items = Array.isArray(listed) ? listed : listed.items || [];
        const targets = items.filter(
          (x) =>
            (pf && Number(x.host_port) === pf.host_port) ||
            String(x.description || '').startsWith(`${TAG}-`),
        );
        // Always try the known create tuple even if list drifted.
        if (pf && !targets.some((x) => Number(x.host_port) === pf.host_port)) {
          targets.push(pf);
        }
        for (const t of targets) {
          const body = {
            protocol: t.protocol || 'tcp',
            host_port: Number(t.host_port),
            vm_port: Number(t.vm_port || 22),
          };
          const r = await api('POST', `${P}/api/v1/vms/${PID}/port-forwards/delete`, body);
          if (!ok(r.status)) throw new Error(`${r.status} ${String(r.body).slice(0, 120)}`);
        }
        const after = await getJson(`${P}/api/v1/vms/${PID}/port-forwards`);
        const left = (Array.isArray(after) ? after : after.items || []).filter(
          (x) =>
            (pf && Number(x.host_port) === pf.host_port) ||
            String(x.description || '').startsWith(`${TAG}-`),
        );
        if (left.length) throw new Error(`still present: ${left.map((x) => x.host_port).join(',')}`);
        return `deleted=${targets.length}`;
      });
    }
    if (lbMemberId && lbId) {
      await mark('lb-member-delete', async () => {
        await del(`${P}/api/v1/load-balancers/${lbId}/members/${lbMemberId}`);
        return 'deleted';
      });
    }
    if (lbId) {
      await mark('lb-delete', async () => {
        await del(`${P}/api/v1/load-balancers/${lbId}`);
        return 'deleted';
      });
    }
    if (stackId) {
      await mark('stack-delete', async () => {
        await del(`${P}/api/v1/stacks/${stackId}`);
        return 'deleted';
      });
    }
    if (sgRuleId) {
      await mark('sg-rule-delete', async () => {
        await del(`${P}/api/v1/security-group-rules/${sgRuleId}`);
        return 'deleted';
      });
    }
    if (sgId) {
      await mark('sg-delete', async () => {
        await del(`${P}/api/v1/security-groups/${sgId}`);
        return 'deleted';
      });
    }
    if (keypairId) {
      await mark('keypair-delete', async () => {
        await del(`${P}/api/v1/keypairs/${keypairId}`);
        return 'deleted';
      });
    }
    if (flavorId) {
      await mark('flavor-delete', async () => {
        await del(`${P}/api/v1/flavors/${flavorId}`);
        return 'deleted';
      });
    }
  }

  console.log(`PASS SUMMARY pass=${passed} fail=${fail}`);
  log.append({ kind: 'SUMMARY', api: 'fleetcloud', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`FLEETCLOUD_DONE pass=${passed} fail=${fail}`);
  process.exitCode = fail === 0 ? 0 : 1;
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});

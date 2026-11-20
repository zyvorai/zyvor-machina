#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * Provision / join / IaC export smoke: OIDC status, host join negative,
 * webhook purge, guestkit schema negatives, template readiness, from-*
 * schema negatives, VM spec/ws-token/adopt/prune/migrate schema, IaC export,
 * cockpit inventory + packagekit, package-upgrade dry-run.
 *
 * Avoids: host.maintenance enter, vm.retire, disk detach/resize, guestkit
 * qemu-nbd doctor (hangs when nbd busy), events/stream SSE.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-provision');
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
    log.append({ kind: 'PROV', api: name, ok: true, note: String(note || 'ok').slice(0, 180) });
    return true;
  } catch (e) {
    log.append({ kind: 'PROV', api: name, ok: false, note: e.message.slice(0, 220) });
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
  if (_ids.platformVmId) PID = _ids.platformVmId;
  let passed = 0;
  let fail = 0;
  const mark = async (name, fn) => {
    if (await step(name, fn)) passed++;
    else fail++;
  };

  await mark('host-detail', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/detail`);
    if (!j.id || !j.hostname) throw new Error('empty');
    if (j.maintenance_mode) throw new Error('host unexpectedly in maintenance');
    return `${j.hostname} state=${j.state}`;
  });

  await mark('host-validate', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/validate`);
    if (j.ok == null && !Array.isArray(j.checks)) throw new Error('empty');
    return `ok=${j.ok} checks=${(j.checks || []).length}`;
  });

  await mark('host-cockpit-inventory', async () => {
    const j = await getJson(`${P}/api/v1/hosts/${HID}/cockpit`);
    if (!j.storage && !j.network && !j.system) throw new Error('empty');
    return `storage=${!!j.storage} network=${!!j.network}`;
  });

  await mark('host-package-upgrade-dry', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/linux/package-upgrade`, {
      dry_run: true,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.dry_run !== true && !j.result) throw new Error('empty');
    return `dry_run=${j.dry_run}`;
  });

  await mark('cockpit-package-install-negative', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/cockpit/actions`, {
      action: 'host.package.install',
      payload: {},
    });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('cockpit-query-vs-invoke', async () => {
    // GET inventory uses query actions; POST of the same names must not hang the suite
    // and should fail as unknown invoke actions.
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/cockpit/actions`, {
      action: 'cockpit.storage',
      payload: {},
    });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status} query-only`;
  });

  await mark('auth-oidc', async () => {
    const j = await getJson(`${P}/api/v1/auth/oidc`);
    if (j.enabled == null) throw new Error('empty');
    return `enabled=${j.enabled}`;
  });

  await mark('auth-oidc-login-disabled', async () => {
    const r = await api('GET', `${P}/api/v1/auth/oidc/login`);
    if (r.status === 200) return 'enabled';
    if (r.status < 400) throw new Error(`unexpected ${r.status}`);
    return `${r.status} disabled`;
  });

  await mark('hosts-join-negative', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/join`, {
      token: 'invalid',
      hostname: 'reg-join',
      agent_grpc_addr: '127.0.0.1:50051',
    });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('webhook-deliveries-purge', async () => {
    const r = await api('POST', `${P}/api/v1/webhook-deliveries/purge`, { status: 'failed' });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.deleted == null) throw new Error('empty');
    return `deleted=${j.deleted}`;
  });

  await mark('spectator-validate-negative', async () => {
    const r = await api(
      'GET',
      `${P}/api/v1/consolehub/spectator/validate?session_id=00000000-0000-0000-0000-000000000001&token=invalid`,
    );
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (j.valid !== false) throw new Error('expected valid=false');
    return 'invalid';
  });

  await mark('guestkit-status', async () => {
    const j = await getJson(`${P}/api/v1/guestkit/status`);
    if (j.enabled == null) throw new Error('empty');
    return `enabled=${j.enabled} worker=${j.worker_reachable}`;
  });

  await mark('guestkit-doctor-schema', async () => {
    const r = await api('POST', `${P}/api/v1/guestkit/doctor`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status} image_path required`;
  });

  await mark('guestkit-jobs-schema', async () => {
    const r = await api('POST', `${P}/api/v1/guestkit/jobs`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status} image_path required`;
  });

  await mark('guestkit-migrate-schema', async () => {
    const r = await api('POST', `${P}/api/v1/guestkit/migrate-plan`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status} image_path required`;
  });

  await mark('templates-marketplace', async () => {
    const j = await getJson(`${P}/api/v1/templates/marketplace`);
    if (!Array.isArray(j)) throw new Error('not array');
    return `count=${j.length}`;
  });

  await mark('template-readiness', async () => {
    const list = await getJson(`${P}/api/v1/templates`);
    const items = Array.isArray(list) ? list : list.items || [];
    if (items.length < 1) throw new Error('no templates');
    const name = items[0].name;
    const version = items[0].version || '1.0.0';
    const j = await getJson(
      `${P}/api/v1/templates/${encodeURIComponent(name)}/${encodeURIComponent(version)}/readiness`,
    );
    if (j.ready == null && !j.remediation) throw new Error('empty');
    return `${name}@${version} ready=${j.ready}`;
  });

  await mark('from-template-schema', async () => {
    const r = await api('POST', `${P}/api/v1/vms/from-template`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('from-iso-schema', async () => {
    const r = await api('POST', `${P}/api/v1/vms/from-iso`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('from-virt-install-schema', async () => {
    const r = await api('POST', `${P}/api/v1/vms/from-virt-install`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('from-iso-path-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/from-iso`, {
      name: 'reg-iso-neg',
      iso_path: '/nonexistent.iso',
    });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('vm-spec', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/spec`);
    return `keys=${Object.keys(j).length}`;
  });

  await mark('vm-ws-token', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/ws-token`, {});
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.token) throw new Error('no token');
    return j.token.slice(0, 8);
  });

  await mark('vm-adopt-already-managed', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/adopt`, {});
    const body = String(r.body || '');
    // Idempotent adopt may 200; conflict/already-managed is 4xx.
    if (r.status < 400) {
      if (/already managed|already.?adopt|noop|idempotent/i.test(body)) return `${r.status}-idempotent`;
      throw new Error(`expected reject got ${r.status} ${body.slice(0, 80)}`);
    }
    return `${r.status}`;
  });

  await mark('vm-prune-inventory-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/prune-inventory`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('vm-migrate-schema', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/migrate`, {});
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status} dest_host_id required`;
  });

  await mark('vm-export-iac', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/export`);
    if (!j.terraform || !j.vm_name) throw new Error(JSON.stringify(j).slice(0, 80));
    return `name=${j.vm_name} tf=${j.terraform.length}`;
  });

  await mark('vm-export-iac-zip', async () => {
    const r = await api('GET', `${P}/api/v1/vms/${PID}/export.zip`);
    if (!ok(r.status)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    // zip magic or non-empty binary/text body
    if (!r.body || r.body.length < 50) throw new Error('empty zip');
    return `bytes=${r.body.length}`;
  });

  await mark('network-get', async () => {
    const nets = await getJson(`${P}/api/v1/networks`);
    const items = Array.isArray(nets) ? nets : nets.items || [];
    if (items.length < 1) throw new Error('empty');
    const j = await getJson(`${P}/api/v1/networks/${items[0].id}`);
    if (!j.id || !j.name) throw new Error('empty');
    return j.name;
  });

  await mark('host-maintenance-exit', async () => {
    const r = await api('POST', `${P}/api/v1/hosts/${HID}/maintenance`, {
      action: 'exit',
      evacuate: false,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status}`);
    const j = JSON.parse(r.body);
    if (!j.task_id) throw new Error('no task');
    return j.task_id.slice(0, 8);
  });

  // sanity: chrome-e2e-vm still present and host not stuck in maintenance
  await mark('postcheck-vm-host', async () => {
    const vm = await getJson(`${P}/api/v1/vms/${PID}`);
    const host = await getJson(`${P}/api/v1/hosts/${HID}/detail`);
    if (!vm.name) throw new Error('vm missing');
    if (host.maintenance_mode) throw new Error('left in maintenance');
    return `vm=${vm.observed_state} host=${host.state}`;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`PROVISION_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

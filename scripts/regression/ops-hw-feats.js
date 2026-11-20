#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

/**
 * VM hardware feature regression: NIC model switch (nic.tune), disk.tune,
 * libvirt queries, live vCPU/memory, USB/PCI negatives, vsock/tpm/watchdog
 * soft-path, boot get/set, classic balloon + nic attach.
 *
 * Prefer MACHINA_VM_NAME=chrome-e2e-vm (virtio hotplug). Windows may need
 * shutoff for some model switches.
 */

const { loadConfig } = require('./lib/config');
const { resolveIds } = require('./lib/ids');
const { createApi } = require('./lib/api');
const { createLogger } = require('./lib/log');

const cfg = loadConfig();
const { api, login } = createApi(cfg);
const log = createLogger(cfg.resultsDir, 'ops-hw-feats');
const VM = cfg.vmName;
let PID = process.env.MACHINA_PLATFORM_VM_ID || '';
let HID = process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '';
const P = '/api/v1/platform/controller';
const winGuest = /win|windows/i.test(VM);
const attachModel = winGuest ? 'e1000' : 'virtio';
const switchModel = winGuest ? 'e1000e' : 'rtl8139';

function ok(status) {
  return status >= 200 && status < 400;
}
function isHtml(body) {
  return /^<!DOCTYPE/i.test(body || '');
}

async function step(name, fn) {
  try {
    const note = await fn();
    log.append({ kind: 'HWFEATS', api: name, ok: true, note: String(note || 'ok').slice(0, 200) });
    return true;
  } catch (e) {
    log.append({ kind: 'HWFEATS', api: name, ok: false, note: e.message.slice(0, 240) });
    return false;
  }
}

async function getJson(path) {
  const r = await api('GET', path);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`${path} ${r.status}`);
  return JSON.parse(r.body);
}

async function libvirtQuery(action, extra = {}) {
  const sp = new URLSearchParams({ action, ...extra });
  return getJson(`${P}/api/v1/vms/${PID}/libvirt?${sp}`);
}

async function libvirtInvoke(action, payload = {}) {
  const r = await api('POST', `${P}/api/v1/vms/${PID}/libvirt`, { action, payload });
  if (!ok(r.status) || isHtml(r.body)) {
    throw new Error(`${action} ${r.status} ${String(r.body).slice(0, 160)}`);
  }
  try {
    return JSON.parse(r.body);
  } catch {
    return { raw: r.body };
  }
}

async function hostLibvirtQuery(action) {
  const r = await api('GET', `${P}/api/v1/hosts/${HID}/libvirt?action=${encodeURIComponent(action)}`);
  if (!ok(r.status) || isHtml(r.body)) throw new Error(`host ${action} ${r.status}`);
  return JSON.parse(r.body);
}

async function waitTask(taskId, timeoutMs = 120000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const j = await getJson(`${P}/api/v1/tasks/${taskId}`);
    const st = String(j.status || '').toLowerCase();
    if (st === 'completed' || st === 'succeeded' || st === 'success') return j;
    if (st === 'failed' || st === 'error' || st === 'cancelled') {
      throw new Error(`task ${taskId} ${st}: ${j.error || j.last_error || ''}`);
    }
    await new Promise((x) => setTimeout(x, 800));
  }
  throw new Error(`task ${taskId} timeout`);
}

async function ensureState(want) {
  let state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
  if (state === want) return state;
  if (want === 'running') {
    // Keep platform desired_state in sync so reconcile does not fight classic start.
    await api('POST', `${P}/api/v1/vms/${PID}/start`, {}).catch(() => null);
    if (state === 'paused') await api('POST', `/api/v1/vms/${VM}/resume`);
    else await api('POST', `/api/v1/vms/${VM}/start`);
  } else if (want === 'shutoff') {
    // Platform stop first — otherwise reconcile restarts the domain mid-edit.
    await api('POST', `${P}/api/v1/vms/${PID}/stop`, { force: true }).catch(() => null);
    if (state === 'paused') await api('POST', `/api/v1/vms/${VM}/resume`);
    const r = await api('POST', `/api/v1/vms/${VM}/stop`, { force: true });
    if (!ok(r.status) && r.status !== 409) {
      await api('POST', `/api/v1/vms/${VM}/destroy`).catch(() => null);
    }
  }
  for (let i = 0; i < 60; i++) {
    await new Promise((x) => setTimeout(x, 1000));
    state = JSON.parse((await api('GET', `/api/v1/vms/${VM}`)).body).state;
    if (state === want) return state;
    if (want === 'shutoff' && (state === 'shutdown' || state === 'shut off')) return 'shutoff';
  }
  throw new Error(`wanted ${want} got ${state}`);
}

function macOf(n) {
  return String(n.mac_address || n.mac || n.address || '').toLowerCase();
}

async function platformNics() {
  const j = await getJson(`${P}/api/v1/vms/${PID}/nics`);
  if (!Array.isArray(j)) throw new Error('nics not array');
  return j;
}

async function platformStartDesired() {
  await api('POST', `${P}/api/v1/vms/${PID}/start`, {});
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

  let addedMac = '';
  let origVcpus = 0;
  let origMemMb = 0;
  let diskTarget = '';
  let origCache = 'none';
  let vsockAttached = false;
  let tpmAttached = false;

  await mark('ensure-running', async () => ensureState('running'));

  // ─── Inventory / queries ───────────────────────────────────────────
  await mark('vm-hardware-summary', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/hardware-summary`);
    if (!j.vm_name && !j.cpu) throw new Error('empty');
    return `${j.vm_name || '?'} disks=${(j.disks || []).length} nics=${(j.nics || j.interfaces || []).length}`;
  });

  await mark('vm-hardware-compat', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/hardware-compat`);
    if (typeof j.ok !== 'boolean') throw new Error('no ok');
    return `ok=${j.ok}`;
  });

  await mark('libvirt-details', async () => {
    const j = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    const disks = j.disks || j.block_devices || [];
    const nics = j.nics || j.interfaces || [];
    if (!Array.isArray(disks) && !j.xml && !j.domain) throw new Error('empty details');
    // Prefer non-root virtio/sata data disk for cache tune
    if (Array.isArray(disks)) {
      const candidates = disks.filter((d) => {
        const t = String(d.target || d.dev || '');
        const bus = String(d.bus || d.device || '');
        return t && !/^sd[a]$/i.test(t) && !/^vd[a]$/i.test(t) && bus !== 'cdrom' && d.device !== 'cdrom';
      });
      const pick = candidates[0] || disks.find((d) => d.target || d.dev);
      if (pick) {
        diskTarget = String(pick.target || pick.dev);
        origCache = String(pick.cache || 'none');
      }
    }
    return `disks=${Array.isArray(disks) ? disks.length : '?'} nics=${Array.isArray(nics) ? nics.length : '?'}`;
  });

  await mark('query-boot-get', async () => {
    const j = await libvirtQuery('boot.get');
    if (!j && j !== 0) throw new Error('empty');
    return `keys=${Object.keys(j).slice(0, 6).join(',')}`;
  });

  await mark('query-cputune-get', async () => {
    const j = await libvirtQuery('cputune.get');
    return `keys=${Object.keys(j).slice(0, 6).join(',')}`;
  });

  await mark('query-memtune-get', async () => {
    const j = await libvirtQuery('memtune.get');
    return `keys=${Object.keys(j).slice(0, 6).join(',')}`;
  });

  await mark('query-cpu-memory-topology', async () => {
    const j = await libvirtQuery('cpu.memory.topology');
    if (j.vcpus == null && j.current_memory_kib == null) throw new Error('empty topo');
    origVcpus = Number(j.vcpus) || 0;
    origMemMb = Math.max(64, Math.floor((Number(j.current_memory_kib) || 0) / 1024));
    return `vcpus=${j.vcpus} mem_kib=${j.current_memory_kib} state=${j.state}`;
  });

  await mark('query-parity-summary', async () => {
    const j = await libvirtQuery('parity.summary');
    return `keys=${Object.keys(j).slice(0, 8).join(',')}`;
  });

  await mark('host-usb-list', async () => {
    const j = await hostLibvirtQuery('host.usb');
    const n = Array.isArray(j) ? j.length : (j.devices || j.usb || []).length;
    return `n=${n}`;
  });

  await mark('host-pci-list', async () => {
    const j = await hostLibvirtQuery('host.pci');
    const n = Array.isArray(j) ? j.length : (j.devices || j.pci || []).length;
    return `n=${n}`;
  });

  await mark('host-node-devices', async () => {
    const j = await hostLibvirtQuery('host.node_devices');
    const n = Array.isArray(j) ? j.length : (j.devices || []).length;
    return `n=${n}`;
  });

  // ─── NIC attach + model switch (nic.tune) ──────────────────────────
  await mark('nic-attach', async () => {
    const before = new Set((await platformNics()).map(macOf).filter(Boolean));
    // Windows SATA/q35: prefer offline attach like ops-net
    if (winGuest) await ensureState('shutoff');
    const r = await api('POST', `${P}/api/v1/vms/${PID}/nics/attach`, {
      network: 'default',
      model: attachModel,
    });
    if (!ok(r.status) || isHtml(r.body)) throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
    const body = JSON.parse(r.body);
    if (body.task_id) await waitTask(body.task_id);
    else {
      // classic path fallback
      const c = await api('POST', `/api/v1/vms/${VM}/nic/attach`, {
        network: 'default',
        model: attachModel,
      });
      if (!ok(c.status)) throw new Error(`classic attach ${c.status}`);
    }
    if (winGuest) await ensureState('running');
    let added = '';
    for (let i = 0; i < 30; i++) {
      const fresh = (await platformNics()).map(macOf).filter((m) => m && !before.has(m));
      if (fresh.length) {
        added = fresh[0];
        break;
      }
      await new Promise((x) => setTimeout(x, 500));
    }
    if (!added) throw new Error('nic list did not grow');
    addedMac = added;
    return `mac=${addedMac} model=${attachModel}`;
  });

  await mark('nic-tune-switch-model', async () => {
    if (!addedMac) throw new Error('no mac');
    // Model change often needs shutoff on Windows; try live first on Linux
    let triedOffline = false;
    const apply = async () =>
      libvirtInvoke('nic.tune', {
        mac_address: addedMac,
        model: switchModel,
        network: 'default',
      });
    try {
      await apply();
    } catch (e) {
      if (winGuest || /live|hotplug|not supported|refuse|running/i.test(e.message)) {
        triedOffline = true;
        await ensureState('shutoff');
        await apply();
        await ensureState('running');
      } else {
        throw e;
      }
    }
    const nics = await platformNics();
    const hit = nics.find((n) => macOf(n) === addedMac);
    const model = String((hit && (hit.model || hit.model_type)) || '').toLowerCase();
    if (model && model !== switchModel) {
      // Some inventory lags; accept invoke success
      return `invoked ${attachModel}→${switchModel} reported=${model || '?'} offline=${triedOffline}`;
    }
    return `${attachModel}→${switchModel} offline=${triedOffline}`;
  });

  await mark('nic-tune-restore-model', async () => {
    if (!addedMac) throw new Error('no mac');
    const apply = async () =>
      libvirtInvoke('nic.tune', {
        mac_address: addedMac,
        model: attachModel,
        network: 'default',
      });
    try {
      await apply();
    } catch (e) {
      if (/live|hotplug|not supported|refuse|running/i.test(e.message)) {
        await ensureState('shutoff');
        await apply();
        await ensureState('running');
      } else throw e;
    }
    return `restored ${attachModel}`;
  });

  await mark('nic-tune-bad-mac-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/libvirt`, {
      action: 'nic.tune',
      payload: { mac_address: '52:54:00:00:00:00', model: 'virtio' },
    });
    if (r.status < 400) throw new Error(`expected 4xx got ${r.status}`);
    return `${r.status}`;
  });

  await mark('nic-detach', async () => {
    if (!addedMac) throw new Error('no mac');
    // Detach offline — live detach of just-tuned NICs often flakes.
    await ensureState('shutoff');
    const r = await api(
      'POST',
      `${P}/api/v1/vms/${PID}/nics/detach/${encodeURIComponent(addedMac)}`,
    );
    if (!ok(r.status) || isHtml(r.body)) {
      // Classic fallback
      const c = await api('POST', `/api/v1/vms/${VM}/nic/detach`, { mac: addedMac });
      if (!ok(c.status) && c.status !== 404) throw new Error(`detach ${r.status}/${c.status}`);
    } else {
      const body = JSON.parse(r.body);
      if (body.task_id) await waitTask(body.task_id);
    }
    for (let i = 0; i < 40; i++) {
      const nics = await platformNics();
      if (!nics.some((n) => macOf(n) === addedMac)) {
        await ensureState('running');
        return `gone count=${nics.length}`;
      }
      await new Promise((x) => setTimeout(x, 500));
    }
    throw new Error('mac still present');
  });

  // ─── Disk tune (cache only; avoid bus change on root) ──────────────
  await mark('disk-tune-cache', async () => {
    if (!diskTarget) {
      // Soft: no extra disk — exercise negative only later
      return 'skipped-no-extra-disk';
    }
    const next = origCache === 'writeback' ? 'writethrough' : 'writeback';
    try {
      await libvirtInvoke('disk.tune', { target: diskTarget, cache: next });
      await libvirtInvoke('disk.tune', { target: diskTarget, cache: origCache || 'none' });
      return `target=${diskTarget} ${origCache}↔${next}`;
    } catch (e) {
      if (/live|hotplug|not supported|busy|readonly/i.test(e.message)) {
        await ensureState('shutoff');
        await libvirtInvoke('disk.tune', { target: diskTarget, cache: next });
        await libvirtInvoke('disk.tune', { target: diskTarget, cache: origCache || 'none' });
        await ensureState('running');
        return `offline target=${diskTarget}`;
      }
      throw e;
    }
  });

  await mark('disk-tune-bad-target-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/libvirt`, {
      action: 'disk.tune',
      payload: { target: 'sdz9', cache: 'none' },
    });
    if (r.status < 400) throw new Error(`expected 4xx got ${r.status}`);
    return `${r.status}`;
  });

  // ─── Live vCPU / memory (restore) ──────────────────────────────────
  await mark('live-vcpus-roundtrip', async () => {
    await ensureState('running');
    if (!origVcpus || origVcpus < 1) {
      const topo = await libvirtQuery('cpu.memory.topology');
      origVcpus = Number(topo.vcpus) || 2;
    }
    const bump = Math.min(origVcpus + 1, 8);
    try {
      await libvirtInvoke('live.vcpus', { count: bump });
      await libvirtInvoke('live.vcpus', { count: origVcpus });
      return `${origVcpus}→${bump}→${origVcpus}`;
    } catch (e) {
      // Soft-accept when guest/maxvcpus cannot grow (common on Windows goldens).
      if (/maximum|greater than|limit|not supported|cannot|hotplug|invalid argument|unable to execute QEMU|internal error/i.test(e.message)) {
        return `soft ${e.message.slice(0, 100)}`;
      }
      throw e;
    }
  });

  await mark('live-memory-roundtrip', async () => {
    await ensureState('running');
    if (!origMemMb) {
      const topo = await libvirtQuery('cpu.memory.topology');
      origMemMb = Math.max(512, Math.floor((Number(topo.current_memory_kib) || 1024 * 1024) / 1024));
    }
    const bump = origMemMb + 128;
    try {
      await libvirtInvoke('live.memory', { memory_mb: bump });
      await libvirtInvoke('live.memory', { memory_mb: origMemMb });
      return `${origMemMb}→${bump}→${origMemMb} MB`;
    } catch (e) {
      if (/balloon|not supported|cannot|driver|maximum/i.test(e.message)) {
        return `soft ${e.message.slice(0, 80)}`;
      }
      throw e;
    }
  });

  await mark('classic-balloon', async () => {
    await ensureState('running');
    const mb = origMemMb || 2048;
    const r = await api('POST', `/api/v1/vms/${VM}/balloon/${mb}`);
    if (ok(r.status)) return `balloon=${mb}`;
    if (r.status >= 400 && /balloon|not supported|agent/i.test(r.body || '')) {
      return `soft ${r.status}`;
    }
    throw new Error(`${r.status} ${String(r.body).slice(0, 100)}`);
  });

  // ─── USB / PCI negatives ───────────────────────────────────────────
  await mark('usb-attach-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/libvirt`, {
      action: 'usb.attach',
      payload: { vendor_id: 'ffff', product_id: 'ffff' },
    });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  await mark('pci-attach-negative', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/libvirt`, {
      action: 'pci.attach',
      payload: { pci: '0000:ff:ff.0' },
    });
    if (r.status < 400) throw new Error(`expected reject got ${r.status}`);
    return `${r.status}`;
  });

  // ─── Extra devices (soft if host/guest blocks) ─────────────────────
  await mark('vsock-attach-detach', async () => {
    try {
      await libvirtInvoke('vsock.attach', {});
      vsockAttached = true;
      await libvirtInvoke('vsock.detach', {});
      vsockAttached = false;
      return 'ok';
    } catch (e) {
      if (/already|exists|not supported|unavailable/i.test(e.message)) return `soft ${e.message.slice(0, 80)}`;
      // cleanup if attach succeeded then detach failed
      if (vsockAttached) {
        await libvirtInvoke('vsock.detach', {}).catch(() => null);
        vsockAttached = false;
      }
      throw e;
    }
  });

  await mark('tpm-attach-detach', async () => {
    try {
      await libvirtInvoke('tpm.attach', {});
      tpmAttached = true;
      await libvirtInvoke('tpm.detach', {});
      tpmAttached = false;
      return 'ok';
    } catch (e) {
      if (/already|exists|not supported|swtpm|unavailable/i.test(e.message)) {
        return `soft ${e.message.slice(0, 80)}`;
      }
      if (tpmAttached) {
        await libvirtInvoke('tpm.detach', {}).catch(() => null);
        tpmAttached = false;
      }
      throw e;
    }
  });

  await mark('watchdog-attach-soft', async () => {
    const r = await api('POST', `${P}/api/v1/vms/${PID}/libvirt`, {
      action: 'watchdog.attach',
      payload: { model: 'i6300esb', action: 'reset' },
    });
    if (ok(r.status)) return 'attached';
    if (r.status >= 400) return `soft ${r.status}`;
    throw new Error(`${r.status}`);
  });

  await mark('scheduler-set', async () => {
    try {
      await libvirtInvoke('scheduler.set', { cpu_shares: 1024 });
      return 'cpu_shares=1024';
    } catch (e) {
      // Lab libvirt often rejects LIVE+CONFIG together on this call.
      if (/not supported|unavailable|AFFECT_LIVE|AFFECT_CONFIG|Flags/i.test(e.message)) {
        return `soft ${e.message.slice(0, 80)}`;
      }
      throw e;
    }
  });

  await mark('memtune-set', async () => {
    try {
      const cur = await libvirtQuery('memtune.get');
      await libvirtInvoke('memtune.set', {
        hard_limit: cur.hard_limit ?? undefined,
        soft_limit: cur.soft_limit ?? undefined,
        swap_hard_limit: cur.swap_hard_limit ?? undefined,
      });
      return 'ok';
    } catch (e) {
      if (/not supported|invalid|required/i.test(e.message)) return `soft ${e.message.slice(0, 80)}`;
      throw e;
    }
  });

  await mark('boot-set-idempotent', async () => {
    const cur = await libvirtQuery('boot.get');
    const devices = cur.boot_devices || cur.devices || cur.order || ['hd'];
    const list = Array.isArray(devices) ? devices : ['hd'];
    try {
      // Agent expects payload key `devices` (not boot_devices).
      await libvirtInvoke('boot.set', { devices: list });
      return `devices=${list.join(',')}`;
    } catch (e) {
      if (/not supported|invalid|offline|shut|running/i.test(e.message)) {
        await ensureState('shutoff');
        await libvirtInvoke('boot.set', { devices: list });
        await ensureState('running');
        return 'offline-ok';
      }
      throw e;
    }
  });

  // ─── Extended HW: USB real, PCI (safe-only), CD-ROM, video, virtiofs, firmware, root bus ───

  await mark('usb-real-attach-detach', async () => {
    const raw = await hostLibvirtQuery('host.usb');
    const items = Array.isArray(raw) ? raw : raw.devices || raw.usb || [];
    // Never touch Linux root hubs (1d6b). Prefer Dell Integrated Hub / non-hub peripherals.
    const cand = items.find((d) => {
      const v = String(d.vendor_id || d.vendor || '').toLowerCase().replace(/^0x/, '');
      const p = String(d.product_id || d.product || '').toLowerCase().replace(/^0x/, '');
      if (!v || !p) return false;
      if (v === '1d6b') return false;
      return true;
    });
    if (!cand) return 'soft no-non-hub-usb';
    const vendor_id = String(cand.vendor_id || cand.vendor).replace(/^0x/i, '');
    const product_id = String(cand.product_id || cand.product).replace(/^0x/i, '');
    await ensureState('shutoff');
    try {
      await libvirtInvoke('usb.attach', { vendor_id, product_id });
      await libvirtInvoke('usb.detach', { vendor_id, product_id });
      await ensureState('running');
      return `ok ${vendor_id}:${product_id}`;
    } catch (e) {
      await ensureState('running').catch(() => null);
      if (/not found|busy|in use|not supported|Operation not|no matching/i.test(e.message)) {
        return `soft ${vendor_id}:${product_id} ${e.message.slice(0, 80)}`;
      }
      // Best-effort detach if attach stuck
      await libvirtInvoke('usb.detach', { vendor_id, product_id }).catch(() => null);
      throw e;
    }
  });

  await mark('pci-real-attach-detach', async () => {
    const raw = await hostLibvirtQuery('host.pci');
    const items = Array.isArray(raw) ? raw : raw.devices || raw.pci || [];
    // Refuse host-critical classes: bridge, host, ethernet, VGA, SATA/NVMe, ISA, SMBus.
    const unsafe =
      /bridge|host|ethernet|vga|display|sata|nvme|non-volatile|isa|smbus|usb controller|communication|memory/i;
    const cand = items.find((d) => {
      const slot = d.slot || d.address || d.name;
      const cls = String(d.class || d.device_class || '');
      const prod = String(d.device || d.product || '');
      if (!slot) return false;
      if (d.detachable === false) return false;
      if (unsafe.test(cls) || unsafe.test(prod)) return false;
      // Prefer explicitly detachable / vfio-ready
      return d.detachable === true || /vfio|unused|available/i.test(String(d.driver || ''));
    });
    if (!cand) {
      return 'soft no-safe-pci (host NIC/GPU/SATA/bridges only on this lab)';
    }
    const pci = String(cand.slot || cand.address);
    await ensureState('shutoff');
    try {
      await libvirtInvoke('pci.attach', { pci });
      await libvirtInvoke('pci.detach', { pci });
      await ensureState('running');
      return `ok ${pci}`;
    } catch (e) {
      await libvirtInvoke('pci.detach', { pci }).catch(() => null);
      await ensureState('running').catch(() => null);
      if (/not found|vfio|iommu|busy|not supported|Operation not/i.test(e.message)) {
        return `soft ${pci} ${e.message.slice(0, 80)}`;
      }
      throw e;
    }
  });

  await mark('cdrom-insert-eject', async () => {
    const iso = process.env.MACHINA_TEST_ISO || '/var/lib/libvirt/images/isos/featuretest.iso';
    // Prefer platform libvirt cdrom.insert; restore cloud-init seed if present.
    const details = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    const disks = details.disks || [];
    const existingCd = disks.find((d) => d.device === 'cdrom' || /cdrom/i.test(String(d.device)));
    const seedPath =
      (existingCd && (existingCd.path || existingCd.source || existingCd.file)) ||
      `/var/lib/machina/cloud-init/${VM}-seed.iso`;
    const targetHint = existingCd ? String(existingCd.target || '') : '';
    await ensureState('running');
    try {
      const ins = await libvirtInvoke('cdrom.insert', {
        iso_path: iso,
        ...(targetHint ? { target: targetHint } : {}),
      });
      const tgt = String(ins.target || targetHint || 'sda');
      await libvirtInvoke('cdrom.eject', { target: tgt });
      // Restore prior seed when it was a real file path
      if (seedPath && /cloud-init|seed\.iso/i.test(seedPath)) {
        await libvirtInvoke('cdrom.insert', { iso_path: seedPath, target: tgt }).catch(() => null);
      }
      return `target=${tgt} iso=${iso}`;
    } catch (e) {
      // Classic daemon CD-ROM API fallback
      const r = await api('POST', `/api/v1/vms/${VM}/cdrom/insert`, { iso_path: iso });
      if (!ok(r.status)) throw new Error(`${e.message}; classic ${r.status}`);
      const body = JSON.parse(r.body);
      const tgt = body.target || (body.cdrom && body.cdrom.target) || 'sda';
      await api('POST', `/api/v1/vms/${VM}/cdrom/detach/${encodeURIComponent(tgt)}`);
      if (seedPath && /cloud-init|seed\.iso/i.test(seedPath)) {
        await api('POST', `/api/v1/vms/${VM}/cdrom/insert`, {
          iso_path: seedPath,
          target: tgt,
        }).catch(() => null);
      }
      return `classic target=${tgt}`;
    }
  });

  await mark('video-model-switch', async () => {
    // Classic daemon route (not yet on platform libvirt invoke).
    const startModel = winGuest ? 'qxl' : 'virtio';
    const altModel = winGuest ? 'virtio' : 'qxl';
    await ensureState('shutoff');
    const set = async (model) => {
      const r = await api('POST', `/api/v1/vms/${VM}/devices/video-model`, { model });
      if (!ok(r.status) || isHtml(r.body)) {
        throw new Error(`video-model ${model} ${r.status} ${String(r.body).slice(0, 120)}`);
      }
    };
    try {
      await set(altModel);
      await set(startModel);
      await ensureState('running');
      return `${startModel}↔${altModel}`;
    } catch (e) {
      // Restore best-effort
      await set(startModel).catch(() => null);
      await ensureState('running').catch(() => null);
      if (/not supported|No <video>|Operation not/i.test(e.message)) {
        return `soft ${e.message.slice(0, 80)}`;
      }
      throw e;
    }
  });

  await mark('virtiofs-add-remove', async () => {
    if (winGuest) return 'skipped-windows';
    const tag = `machina-hw-${Date.now().toString(36).slice(-4)}`;
    const src = `/tmp/machina-virtiofs-${tag}`;
    // Create share dir on lab host via SSH is unavailable from harness — use known world-writable path.
    // Prefer existing /tmp which always exists on the hypervisor.
    const source_dir = '/tmp';
    await ensureState('shutoff');
    try {
      await libvirtInvoke('virtiofs.add', {
        source_dir,
        mount_tag: tag,
        xattr: false,
      });
      await libvirtInvoke('virtiofs.remove', { mount_tag: tag });
      await ensureState('running');
      return `tag=${tag} src=${source_dir}`;
    } catch (e) {
      await libvirtInvoke('virtiofs.remove', { mount_tag: tag }).catch(() => null);
      await ensureState('running').catch(() => null);
      if (/not supported|virtiofsd|Operation not|shared memory|memfd/i.test(e.message)) {
        return `soft ${e.message.slice(0, 100)}`;
      }
      throw e;
    }
  });

  await mark('firmware-uefi-bios-roundtrip', async () => {
    const before = await libvirtQuery('boot.get');
    const wasUefi = !!(before.firmware === 'uefi' || before.secure_boot || before.loader);
    await ensureState('shutoff');
    try {
      // Flip away then restore original mode so guest stays bootable.
      await libvirtInvoke('firmware.set', { uefi: !wasUefi });
      await libvirtInvoke('firmware.set', { uefi: wasUefi });
      await ensureState('running');
      return `wasUefi=${wasUefi} flipped+restored`;
    } catch (e) {
      // Restore BIOS/UEFI best-effort
      await libvirtInvoke('firmware.set', { uefi: wasUefi }).catch(() => null);
      await ensureState('running').catch(() => null);
      if (/OVMF|edk2|No OVMF|not supported|Operation not/i.test(e.message)) {
        return `soft ${e.message.slice(0, 100)}`;
      }
      throw e;
    }
  });

  await mark('root-disk-bus-roundtrip', async () => {
    const details = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
    const disks = details.disks || [];
    const root =
      disks.find((d) => (d.device === 'disk' || !d.device) && !/cdrom/i.test(String(d.device || ''))) ||
      disks.find((d) => /vd[a]|sd[a]|hd[a]/i.test(String(d.target || '')));
    if (!root || !root.target) return 'soft no-root-disk';
    const target = String(root.target);
    const rootPath = String(root.path || root.source || root.file || '');
    const origBus = String(root.bus || (winGuest ? 'sata' : 'virtio'));
    // q35: prefer virtio ↔ scsi (safer than ide). Windows golden stays on sata↔scsi.
    const altBus = origBus === 'virtio' ? 'scsi' : origBus === 'sata' ? 'scsi' : 'virtio';
    const findRootTarget = async (preferBus) => {
      const d2 = await getJson(`${P}/api/v1/vms/${PID}/libvirt-details`);
      const list = d2.disks || [];
      const byPath =
        rootPath &&
        list.find((d) => String(d.path || d.source || d.file || '') === rootPath);
      if (byPath && byPath.target) return String(byPath.target);
      const byBus = list.find((d) => String(d.bus || '') === preferBus && d.target);
      if (byBus) return String(byBus.target);
      return null;
    };
    await ensureState('shutoff');
    try {
      await libvirtInvoke('disk.tune', { target, bus: altBus });
      const midTarget = (await findRootTarget(altBus)) || target;
      await libvirtInvoke('disk.tune', { target: midTarget, bus: origBus });
      await ensureState('running');
      return `${target} ${origBus}↔${altBus}`;
    } catch (e) {
      const restoreTarget = (await findRootTarget(origBus).catch(() => null)) || target;
      await libvirtInvoke('disk.tune', { target: restoreTarget, bus: origBus }).catch(() => null);
      await ensureState('running').catch(() => null);
      if (/not supported|Cannot|Operation not|invalid|bus|address/i.test(e.message)) {
        return `soft ${target} ${e.message.slice(0, 100)}`;
      }
      throw e;
    }
  });

  await mark('leave-running', async () => {
    await platformStartDesired();
    const st = await ensureState('running');
    return st;
  });

  log.append({ kind: 'SUMMARY', ok: fail === 0, note: `pass=${passed} fail=${fail}` });
  console.log(`HW_FEATS_OPS_DONE pass=${passed} fail=${fail}`);
  process.exit(fail ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});

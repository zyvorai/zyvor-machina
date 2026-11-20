// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// UX audit sweep: loads every route in scripts/regression/fixtures/pages.json with the e2e
// API mock (no daemon needed) and reports layout / accessibility findings per route and mode.
//
//   npm run build && npm run preview -- --host 127.0.0.1 --port 5192 &
//   node scripts/ux-audit.mjs --out reports/ux-audit.json
//   node scripts/ux-audit.mjs --baseline reports/ux-audit-baseline.json      # compare with a saved run
//
// Options: --base URL  --out FILE  --baseline FILE  --routes /a,/b  --modes light,dark,phone
//          --limit N  --concurrency N  --no-focus
// Env:     UX_AUDIT_CHROME=/path/to/chrome (defaults to macOS Google Chrome when present)
//
// Modes: light = 1440px light theme (also runs the focus/tab-stop probe), dark = 1440px dark
// theme, phone = 390px light theme. Findings are heuristics from a mocked API: empty or thin
// mock data hides populated-state bugs, so treat a clean run as "no regressions", not "perfect".

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'
import { mockPlatformApi } from '../e2e/platformMock.ts'

const here = path.dirname(fileURLToPath(import.meta.url))
const args = process.argv.slice(2)
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`)
  return i >= 0 ? args[i + 1] : fallback
}
const flag = (name) => args.includes(`--${name}`)

const BASE = opt('base', 'http://127.0.0.1:5192')
const OUT = opt('out', '')
const BASELINE = opt('baseline', '')
const CONCURRENCY = Number(opt('concurrency', '4'))
const MODES = opt('modes', 'light,dark,phone').split(',')
const ALL_ROUTES = JSON.parse(fs.readFileSync(path.join(here, '../../scripts/regression/fixtures/pages.json'), 'utf8'))
let routes = opt('routes', '') ? opt('routes', '').split(',') : ALL_ROUTES
if (opt('limit', '')) routes = routes.slice(0, Number(opt('limit', '0')))

const MODE = {
  light: { theme: 'light', width: 1440, height: 1000, focus: !flag('no-focus') },
  dark: { theme: 'dark', width: 1440, height: 1000, focus: false },
  phone: { theme: 'light', width: 390, height: 844, focus: false },
}

// ---- browser-side probes (must be self-contained: they are serialised into the page) ----------

function probeContrast() {
  // Computed colours can be rgb(), oklab(), color(srgb ...) (Tailwind v4 emits the latter two for
  // opacity modifiers such as bg-emerald-600/90). Let the browser convert any of them via a canvas.
  const cv = document.createElement('canvas')
  cv.width = cv.height = 1
  const cx = cv.getContext('2d', { willReadFrequently: true })
  const cache = new Map()
  const parse = (c) => {
    if (!c || c === 'transparent') return { r: 0, g: 0, b: 0, a: 0 }
    if (cache.has(c)) return cache.get(c)
    cx.clearRect(0, 0, 1, 1)
    cx.fillStyle = '#000'
    cx.fillStyle = c
    cx.fillRect(0, 0, 1, 1)
    const [r, g, b, a] = cx.getImageData(0, 0, 1, 1).data
    const out = { r, g, b, a: a / 255 }
    cache.set(c, out)
    return out
  }
  const lum = ({ r, g, b }) => {
    const f = (v) => {
      v /= 255
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
    }
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
  }
  // Composite translucent layers up to the first opaque one; gradients/images are skipped.
  const backgroundOf = (el) => {
    const layers = []
    for (let e = el; e; e = e.parentElement) {
      const cs = getComputedStyle(e)
      if (cs.backgroundImage !== 'none') return 'image'
      const c = parse(cs.backgroundColor)
      if (c && c.a > 0) {
        layers.push(c)
        if (c.a >= 0.98) break
      }
    }
    let base = layers.length && layers[layers.length - 1].a >= 0.98 ? layers.pop() : parse(getComputedStyle(document.documentElement).backgroundColor)
    if (!base || base.a < 0.98) base = { r: 255, g: 255, b: 255, a: 1 }
    while (layers.length) {
      const t = layers.pop()
      base = { r: t.r * t.a + base.r * (1 - t.a), g: t.g * t.a + base.g * (1 - t.a), b: t.b * t.a + base.b * (1 - t.a), a: 1 }
    }
    return base
  }
  const out = []
  const seen = new Set()
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
  while (walker.nextNode()) {
    const n = walker.currentNode
    if (!n.textContent.trim()) continue
    const el = n.parentElement
    if (!el || seen.has(el) || el.closest('button:disabled, input:disabled, [aria-disabled="true"], [data-ux-audit-ignore]')) continue
    seen.add(el)
    const cs = getComputedStyle(el)
    const r = el.getBoundingClientRect()
    if (cs.visibility === 'hidden' || cs.display === 'none' || +cs.opacity === 0 || !r.width || !r.height) continue
    const fg = parse(cs.color)
    const bg = backgroundOf(el)
    // Fully transparent text is deliberate (overlay editors such as the terminal textarea).
    if (!fg || fg.a === 0 || bg === 'image') continue
    const l1 = lum(fg)
    const l2 = lum(bg)
    const ratio = (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05)
    if (ratio < 3) out.push({ text: n.textContent.trim().slice(0, 36), ratio: +ratio.toFixed(2), sel: el.tagName.toLowerCase() + '.' + String(el.className || '').split(/\s+/).filter(Boolean).slice(0, 2).join('.') })
  }
  return out
}

function probeClip() {
  const vw = innerWidth
  const out = []
  const scrollAncestor = (el) => {
    for (let e = el.parentElement; e && e !== document.body; e = e.parentElement) {
      const o = getComputedStyle(e).overflowX
      if (['auto', 'scroll', 'hidden', 'clip'].includes(o)) return e
    }
    return null
  }
  for (const el of document.querySelectorAll('main *, [role=dialog], aside, header, nav')) {
    if (!['SECTION', 'FORM', 'HEADER', 'H1', 'H2', 'BUTTON', 'INPUT', 'SELECT', 'TABLE', 'ASIDE', 'NAV', 'DIV'].includes(el.tagName)) continue
    const r = el.getBoundingClientRect()
    if (!r.width || !r.height) continue
    // Only right-edge spill counts as clipping: a closed off-canvas drawer sits at left < 0 on purpose.
    if (r.right > vw + 1) {
      const sa = scrollAncestor(el)
      if (sa && sa.getBoundingClientRect().right <= vw + 1) continue
      if (getComputedStyle(el).position === 'fixed' && r.left >= 0) continue
      out.push(el.tagName.toLowerCase() + '.' + String(el.className || '').split(/\s+/).filter(Boolean)[0])
    }
  }
  return [...new Set(out)].slice(0, 4)
}

function probeA11y() {
  const out = []
  const visible = (el) => {
    const r = el.getBoundingClientRect()
    const cs = getComputedStyle(el)
    return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && cs.display !== 'none'
  }
  const nameOf = (el) => {
    const aria = el.getAttribute('aria-label')
    if (aria && aria.trim()) return { name: aria.trim(), weak: false }
    const lb = el.getAttribute('aria-labelledby')
    if (lb) {
      const t = lb.split(/\s+/).map((i) => document.getElementById(i)?.textContent || '').join(' ').trim()
      if (t) return { name: t, weak: false }
    }
    if (el.labels && el.labels.length) {
      const t = [...el.labels].map((l) => l.textContent).join(' ').trim()
      if (t) return { name: t, weak: false }
    }
    const t = (el.innerText || el.textContent || '').trim()
    if (t && !/^input|select|textarea$/i.test(el.tagName)) return { name: t, weak: false }
    const ti = el.getAttribute('title')
    if (ti && ti.trim()) return { name: ti.trim(), weak: false }
    const img = el.querySelector('img[alt]')
    if (img && img.alt.trim()) return { name: img.alt.trim(), weak: false }
    const ph = el.getAttribute('placeholder')
    if (ph && ph.trim()) return { name: ph.trim(), weak: true }
    return { name: '', weak: false }
  }
  for (const el of document.querySelectorAll('button, [role=button], a[href], input:not([type=hidden]), select, textarea')) {
    if (!visible(el)) continue
    const { name, weak } = nameOf(el)
    const tag = el.tagName.toLowerCase() + (el.type && el.tagName === 'INPUT' ? `[${el.type}]` : '')
    if (!name) out.push({ kind: 'unnamed-control', detail: `${tag} ${el.outerHTML.replace(/\s+/g, ' ').replace(/class="[^"]*"/g, '').slice(0, 110)}` })
    else if (weak) out.push({ kind: 'placeholder-only-name', detail: `${tag} "${name.slice(0, 24)}"` })
  }
  const ids = {}
  for (const e of document.querySelectorAll('[id]')) ids[e.id] = (ids[e.id] || 0) + 1
  for (const [k, v] of Object.entries(ids)) if (v > 1) out.push({ kind: 'duplicate-id', detail: k })
  const hs = [...document.querySelectorAll('h1,h2,h3,h4,h5,h6')].filter(visible).map((h) => +h.tagName[1])
  const h1s = hs.filter((x) => x === 1).length
  if (h1s === 0) out.push({ kind: 'no-h1', detail: '' })
  if (h1s > 1) {
    const texts = [...document.querySelectorAll('h1')].filter(visible).map((h) => `"${(h.textContent || '').trim().slice(0, 22)}"${h.className ? '.' + String(h.className).split(/\s+/).filter(Boolean)[0] : ''}`)
    out.push({ kind: 'multiple-h1', detail: texts.join(' + ') })
  }
  for (let i = 1; i < hs.length; i++) {
    if (hs[i] - hs[i - 1] > 1) {
      out.push({ kind: 'heading-skip', detail: `h${hs[i - 1]}->h${hs[i]}` })
      break
    }
  }
  if (!document.querySelector('main, [role=main]')) out.push({ kind: 'no-main-landmark', detail: '' })
  return out
}

function probeTap() {
  const out = []
  for (const el of document.querySelectorAll('button, a[href], input:not([type=hidden]):not([type=checkbox]):not([type=radio]), select, [role=button]')) {
    const r = el.getBoundingClientRect()
    const cs = getComputedStyle(el)
    if (!r.width || !r.height || cs.visibility === 'hidden' || cs.display === 'none') continue
    if (el.tagName === 'A' && cs.display === 'inline') continue // inline text links are exempt
    if (r.width <= 2 && r.height <= 2) continue // visually hidden until focused (skip links)
    if (r.height < 34 || r.width < 34) out.push(`${(el.getAttribute('aria-label') || el.innerText || el.placeholder || el.tagName).trim().slice(0, 24)} ${Math.round(r.width)}x${Math.round(r.height)}`)
  }
  return [...new Set(out)]
}

function probeScreen() {
  const t = document.body.innerText || ''
  if (/Something went wrong|Route not found|Application error/i.test(t)) return 'error-screen'
  if (t.trim().length < 40) return 'blank'
  return ''
}

async function probeFocus(page) {
  const bad = []
  const seen = new Set()
  await page.evaluate(() => document.activeElement && document.activeElement.blur())
  for (let i = 0; i < 40; i++) {
    await page.keyboard.press('Tab')
    const r = await page.evaluate(() => {
      const el = document.activeElement
      if (!el || el === document.body) return null
      const cs = getComputedStyle(el)
      const rect = el.getBoundingClientRect()
      const ok = (cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) > 0) || (cs.boxShadow && cs.boxShadow !== 'none')
      const label = (el.getAttribute('aria-label') || el.innerText || el.placeholder || '').trim().slice(0, 24)
      return { key: `${el.tagName}|${label}|${Math.round(rect.x)},${Math.round(rect.y)}`, ok, label, tag: el.tagName.toLowerCase(), vis: rect.width > 0 && rect.height > 0 }
    })
    if (!r) continue
    if (seen.has(r.key)) break
    seen.add(r.key)
    if (!r.ok && r.vis) bad.push(`${r.tag} "${r.label}"`)
  }
  return { stops: seen.size, bad }
}

// Shapes the e2e mock leaves out (it answers `{}`), which crash pages that read nested fields.
// Registered after mockPlatformApi so it wins. Add entries as the sweep finds mock gaps.
// `match` is a URL substring or RegExp (first hit wins); body is `json` or raw `text` (+ `contentType`).
const hwReport = {
  collected_at_rfc3339: '2026-09-27T06:00:00Z',
  sources: ['sysfs', 'dmi', 'libvirt'],
  dmi: { product_uuid: '4c4c4544-0042-4210-8052-b8c04f4b3733', product_serial: 'ABC1234', sys_vendor: 'Dell Inc.', product_name: 'PowerEdge R650', board_vendor: 'Dell Inc.', board_name: '0PJ2XD', bios_version: '2.14.1', bios_date: '03/12/2026' },
  cpu_topology: { logical_cpus: 64, sockets: 2, socket_package_ids: [0, 1], physical_cores: 32, threads_per_core_max: 2, cores_per_socket: [16, 16] },
  numa_nodes: [{ node_id: 0, cpu_list: '0-15,32-47', memory_total_kb: 134217728 }, { node_id: 1, cpu_list: '16-31,48-63', memory_total_kb: 134217728 }],
  cpuinfo_vendor_id: 'GenuineIntel',
  cpuinfo_model_name: 'Intel(R) Xeon(R) Gold 6338 CPU @ 2.00GHz',
  libvirt: { cpu_model: 'Icelake-Server', cpu_sockets: 2, cpu_cores: 16, cpu_threads: 2, numa_nodes: 2, memory_mb: 262144 },
  libvirt_logical_cpus_derived: 64,
  consistency_notes: [],
}
const guestIp = { name: 'enp1s0', mac: '52:54:00:ab:cd:01', ip_type: 'ipv4', address: '192.168.122.45', prefix: 24, source: 'agent', dhcp_hostname: 'chrome-e2e-vm' }
const pressure = { some: 0.42, full: 0.05, total: 1839201 }
const cgroupVm = (n, mem) => ({ vm_name: n, cgroup_path: `/machine.slice/machine-qemu-1-${n}.scope`, memory_current_bytes: mem, memory_max_bytes: null, cpu_usage_usec: 91234567, available: true })

const EXTRA_MOCKS = [
  // ---- /storage (daemon libvirt pools; the platform proxy path shares the suffix so anchor on the host)
  { match: /^https?:\/\/[^/]+\/api\/v1\/storage\/pools(\?|$)/, json: [
    { name: 'default', uuid: '5c1b7a3e-1111-4a2b-9c3d-000000000001', state: 'running', capacity_gb: 1800.5, allocation_gb: 612.25, available_gb: 1188.25, autostart: true },
    { name: 'images', uuid: '5c1b7a3e-1111-4a2b-9c3d-000000000002', state: 'running', capacity_gb: 3576.0, allocation_gb: 1204.8, available_gb: 2371.2, autostart: true },
    { name: 'iso', uuid: '5c1b7a3e-1111-4a2b-9c3d-000000000003', state: 'inactive', capacity_gb: 200.0, allocation_gb: 48.5, available_gb: 151.5, autostart: false },
  ] },
  { match: /^https?:\/\/[^/]+\/api\/v1\/storage\/pools\/[^/]+\/volumes(\?|$)/, json: [
    { name: 'chrome-e2e-vm.qcow2', pool: 'default', capacity_gb: 40, allocation_gb: 12.7, path: '/var/lib/libvirt/images/chrome-e2e-vm.qcow2', vol_type: 'file' },
    { name: 'win10-msedge.qcow2', pool: 'default', capacity_gb: 80, allocation_gb: 41.3, path: '/var/lib/libvirt/images/win10-msedge.qcow2', vol_type: 'file' },
  ] },
  // ---- /vms/:name (daemon VmDetails + metrics; the e2e mock answers with the controller VM shape)
  { match: /^https?:\/\/[^/]+\/api\/v1\/metrics\/[^/?]+(\?|$)/, json: {
    name: 'chrome-e2e-vm', state: 'running', running: true, cpu_time_ns: 912345678000, vcpus: 4, memory_total_mb: 8192, memory_used_mb: 3072, memory_pct: 37.5,
    disk_rd_bytes: 1234567890, disk_wr_bytes: 987654321, disk_rd_ops: 40213, disk_wr_ops: 31877, net_rx_bytes: 456789012, net_tx_bytes: 123456789,
    vcpus_detail: [0, 1, 2, 3].map((v) => ({ vcpu: v, cpu_time_ns: 228000000000 + v * 1000000, state: 'running' })),
    disks: [{ device: 'vda', rd_bytes: 1234567890, wr_bytes: 987654321, rd_ops: 40213, wr_ops: 31877 }],
    nets: [{ device: 'vnet0', rx_bytes: 456789012, tx_bytes: 123456789, rx_packets: 345678, tx_packets: 234567 }],
    cgroup: null, libvirt_connection: 'system',
  } },
  { match: /^https?:\/\/[^/]+\/api\/v1\/vms\/[^/?]+(\?|$)/, json: {
    name: 'chrome-e2e-vm', uuid: '7d1f0c2a-3b4e-4c5d-8e9f-0a1b2c3d4e5f', state: 'running', vcpus: 4, memory_mb: 8192, os_type: 'hvm', arch: 'x86_64', autostart: true, persistent: true,
    interfaces: [{ mac_address: '52:54:00:ab:cd:01', source: 'default', model: 'virtio' }],
    disks: [
      { device: 'disk', source: '/var/lib/libvirt/images/chrome-e2e-vm.qcow2', driver: 'qcow2', target: 'vda', bus: 'virtio', cache: 'none', readonly: false, shareable: false },
      { device: 'cdrom', source: '', driver: 'raw', target: 'sda', bus: 'sata', readonly: true },
    ],
    filesystems: [], libvirt_connection: 'system', guest_ip: '192.168.122.45',
  } },
  { match: /\/api\/v1\/vms\/[^/?]+\/boot(\?|$)/, json: { boot_devices: ['hd', 'cdrom'], firmware: 'uefi', secure_boot: false } },
  { match: /\/api\/v1\/vms\/[^/?]+\/cputune(\?|$)/, json: { shares: 1024, period: 100000, quota: -1, vcpupin: [{ vcpu: 0, cpuset: '0-3' }, { vcpu: 1, cpuset: '4-7' }] } },
  { match: /\/api\/v1\/vms\/[^/?]+\/memtune(\?|$)/, json: { hard_limit_kb: 9437184, soft_limit_kb: 8388608, swap_hard_limit_kb: 10485760 } },
  { match: /\/api\/v1\/vms\/[^/?]+\/managed-save\/status/, json: { name: 'chrome-e2e-vm', has_managed_save: false } },
  { match: /\/api\/v1\/vms\/[^/?]+\/snapshots(\?|$)/, json: [
    { name: 'clean-install', vm_name: 'chrome-e2e-vm', creation_time: 1789900000, state: 'shutoff', description: 'Fresh OS install', parent: '', is_current: false },
    { name: 'before-upgrade', vm_name: 'chrome-e2e-vm', creation_time: 1790000000, state: 'running', description: 'Pre-upgrade checkpoint', parent: 'clean-install', is_current: true },
  ] },
  { match: /\/api\/v1\/vms\/[^/?]+\/tags(\?|$)/, json: { tags: ['e2e', 'chrome'] } },
  { match: /\/api\/v1\/vms\/[^/?]+\/interfaces(\?|$)/, json: { addresses: [guestIp], queried_at: '2026-09-27T09:30:00Z', network_gateways: { default: '192.168.122.1' } } },
  { match: /\/api\/v1\/vms\/[^/?]+\/hostname(\?|$)/, json: { hostname: 'chrome-e2e-vm' } },
  { match: /\/api\/v1\/vms\/[^/?]+\/guest-observability(\?|$)/, json: {
    hostname: 'chrome-e2e-vm', os_type: 'linux', os_version: 'Ubuntu 24.04',
    ip_addresses: [guestIp],
    filesystems: [{ mountpoint: '/', name: 'vda1', fs_type: 'ext4', total_bytes: 42949672960, used_bytes: 13421772800 }],
  } },
  { match: /\/api\/v1\/vms\/[^/?]+\/guest-health(\?|$)/, json: {
    vm_name: 'chrome-e2e-vm', state: 'running', agent_reachable: true, metrics_available: true, issues: [], healthy: true, os_pretty_name: 'Ubuntu 24.04.1 LTS', cloud_init_status: 'done',
  } },
  // ---- /platform/blueprints
  { match: '/api/v1/fleet/shortcuts', json: {
    summary: '2 shortcuts ready', blueprint_count: 2, total_vms_covered: 6, runbook_count: 3, executions_24h: 5,
    shortcuts: [
      { id: 'bp-1', name: 'Nightly backup', description: 'Back up all production VMs', actions: ['backup'], vm_count: 4, action_count: 1 },
      { id: 'bp-2', name: 'Weekend power-down', description: 'Stop dev VMs on Friday evening', actions: ['stop', 'snapshot'], vm_count: 2, action_count: 2 },
    ],
  } },
  { match: /\/api\/v1\/blueprints(\?|$)/, json: [
    { id: 'bp-1', name: 'Nightly backup', description: 'Back up all production VMs', actions: ['backup'], vm_ids: ['v1', 'v2', 'v3', 'v4'], created_at: '2026-09-01T10:00:00Z' },
    { id: 'bp-2', name: 'Weekend power-down', description: 'Stop dev VMs on Friday evening', actions: ['stop', 'snapshot'], vm_ids: ['v5', 'v6'], created_at: '2026-09-10T16:30:00Z' },
  ] },
  // ---- /platform/fleet-snapshots
  { match: '/api/v1/fleet/snapshot-schedules', json: [
    { id: 'fs-1', name: 'Nightly production', cron_expr: '0 2 * * *', project: 'production', tag_filter: 'tier=prod', disk_only: false, quiesce: true, retain_count: 14, enabled: true },
    { id: 'fs-2', name: 'Weekly dev', cron_expr: '0 3 * * 0', project: 'dev', tag_filter: '', disk_only: true, quiesce: false, retain_count: 4, enabled: false },
  ] },
  // ---- /platform/zeus/security/{compliance,policies}
  { match: '/api/v1/zeus-firewall/compliance/', json: {
    summary: '2 findings across 6 targets', critical_count: 1,
    findings: [
      { title: 'SSH exposed to 0.0.0.0/0', detail: 'Port 22/tcp is reachable from any source on kvm-lab-01', severity: 'critical', target_id: 'h-1' },
      { title: 'Stale temporary rule', detail: 'Temporary rule for 8080/tcp expired 3h ago but is still active', severity: 'warning', target_id: 'h-2' },
    ],
  } },
  { match: /\/zeus-firewall\/approvals(\?|$)/, json: [
    { id: 'ap-1', target_kind: 'host', target_id: '00000000-0000-4000-8000-000000000001', profile: 'ProductionServer', plan_json: {}, status: 'pending', requested_by: 'sus', created_at: '2026-09-27T08:00:00Z' },
  ] },
  { match: /\/zeus-firewall\/policies(\?|$)/, json: [
    { id: 'pol-1', name: 'production-default', spec_yaml: 'profile: ProductionServer\nrules:\n  - allow: tcp/443\n' },
    { id: 'pol-2', name: 'dr-standby', spec_yaml: 'profile: DRServer\nrules:\n  - allow: tcp/22 from 10.0.0.0/8\n' },
  ] },
  // ---- /platform/networks
  { match: /\/platform\/controller\/api\/v1\/networks(\?|$)/, json: [
    { id: 'net-1', name: 'default', backend: 'libvirt-nat', vlan_id: null, bridge: 'virbr0', segment_id: 'seg-1' },
    { id: 'net-2', name: 'prod-vlan120', backend: 'linux-bridge', vlan_id: 120, bridge: 'br120', segment_id: 'seg-1' },
    { id: 'net-3', name: 'dmz', backend: 'open-vswitch', vlan_id: 200, bridge: 'ovsbr0', segment_id: null },
  ] },
  { match: '/api/v1/network/segments/overview', json: {
    summary: '2 segments, 12 VMs',
    segments: [
      { id: 'seg-1', name: 'production', tier: 'prod', cidr: '10.20.0.0/24', east_west_default: 'deny', firewall_profile: 'ProductionServer', gitops_namespace: 'prod', network_count: 2, vm_count: 8, micro_seg_grade: 'A', micro_seg_score: 92 },
      { id: 'seg-2', name: 'development', tier: 'dev', cidr: '10.30.0.0/24', east_west_default: 'allow', firewall_profile: null, gitops_namespace: 'dev', network_count: 1, vm_count: 4, micro_seg_grade: 'C', micro_seg_score: 61 },
    ],
  } },
  { match: '/api/v1/network/ipam/pools', json: [
    { id: 'pool-1', segment_id: 'seg-1', segment_name: 'production', cidr: '10.20.0.0/24', gateway: '10.20.0.1', next_offset: 14, reservation_count: 12 },
    { id: 'pool-2', segment_id: 'seg-2', segment_name: 'development', cidr: '10.30.0.0/24', gateway: null, next_offset: 6, reservation_count: 4 },
  ] },
  // ---- /settings (identity / SSO panels)
  { match: '/api/v1/system/auth/ldap-settings', json: {
    enabled: false, url: 'ldaps://ad.example.com:636', base_dn: 'DC=example,DC=com', user_filter: '(sAMAccountName={username})', bind_dn: 'CN=svc-machina,OU=Service,DC=example,DC=com', bind_password: '', bind_password_set: true,
    user_dn_template: '', username_attribute: 'sAMAccountName', use_tls: true, insecure_tls: false, member_attribute: 'memberOf',
    admin_group_substrings: ['Machina-Admins'], operator_group_substrings: ['Machina-Operators'], readonly_group_substrings: ['Machina-Viewers'], config_path: '/etc/machina/config.toml', preset: null,
  } },
  { match: '/api/v1/system/auth/oidc-settings', json: {
    enabled: false, issuer_url: 'https://login.example.com/realms/machina', client_id: 'machina', client_secret: '', client_secret_set: true, redirect_url: 'https://kvm-lab-01:5092/api/v1/auth/oidc/callback',
    scopes: ['openid', 'profile', 'email'], username_claim: 'preferred_username', groups_claim: 'groups', linux_username_claim: '', admin_groups: ['machina-admins'], operator_groups: ['machina-operators'], default_role: 'viewer',
    button_label: 'Sign in with SSO', require_local_user_for_session_libvirt: false, config_path: '/etc/machina/config.toml', login_path: '/api/v1/auth/oidc/login', callback_path: '/api/v1/auth/oidc/callback',
  } },
  { match: '/api/v1/system/auth/saml-settings', json: {
    enabled: false, configured: false, sp_entity_id: 'https://kvm-lab-01:5092/saml/metadata', sp_acs_url: 'https://kvm-lab-01:5092/api/v1/auth/saml/acs', idp_entity_id: '', idp_metadata_url: '', idp_metadata_xml: '', idp_metadata_xml_set: false,
    name_id_format: 'urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress', button_label: 'Sign in with SAML', admin_groups: ['machina-admins'], operator_groups: [], default_role: 'viewer', notes: '', config_path: '/etc/machina/config.toml', metadata_path: '/api/v1/auth/saml/metadata',
  } },
  // ---- /node (host cockpit)
  { match: /\/api\/v1\/node(\?|$)/, json: { hostname: 'kvm-lab-01', hypervisor: 'QEMU', hypervisor_version: '8.2.2', lib_version: '10.0.0', cpu_model: 'x86_64', cpu_cores: 16, cpu_threads: 2, cpu_sockets: 2, memory_mb: 262144, numa_nodes: 2, active_vms: 7, defined_vms: 12 } },
  { match: /\/api\/v1\/health(\?|$)/, json: { status: 'ok', libvirt: true } },
  { match: '/api/v1/host/stats', json: { cpu_percent: 23.4, memory_total_mb: 262144, memory_used_mb: 96256, memory_percent: 36.7, swap_total_mb: 8192, swap_used_mb: 512, disk_total_gb: 3840, disk_used_gb: 1210, disk_percent: 31.5, load_1: 3.2, load_5: 2.9, load_15: 2.4, uptime_secs: 1123456, processes: 842 } },
  {
    match: '/api/v1/host/system-info',
    json: {
      hostname: 'kvm-lab-01', timezone: 'UTC', kernel_version: '6.8.0-45-generic', architecture: 'x86-64', os_name: 'Ubuntu', os_version: '24.04', os_pretty_name: 'Ubuntu 24.04.1 LTS',
      boot_time: '2026-09-14T09:31:00Z', rtc_time: '2026-09-27T09:31:00Z', ntp_service: 'active', system_clock_synchronized: true, systemd_version: '255', boot_duration: '38.2s',
      critical_chain_top: ['multi-user.target @21.4s', 'libvirtd.service @18.9s +2.1s'], logged_in_users: 2, pretty_hostname: 'KVM Lab 01', transient_hostname: '', icon_name: 'computer-server',
      chassis: 'server', deployment: 'production', location: 'rack A3', machine_id: '3f2a9c1d8b7e4a5f9d6c0e1b2a3c4d5e', boot_id: '9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d',
      hardware_model: 'PowerEdge R650', firmware_version: '2.14.1', local_time: 'Sun 2026-09-27 09:31:00 UTC', universal_time: 'Sun 2026-09-27 09:31:00 UTC', rtc_in_local_tz: 'no',
      systemd_version_full: 'systemd 255 (255.4-1ubuntu8)', systemd_analyze_blame_top: ['2.1s libvirtd.service', '1.4s networkd-wait-online.service'],
      loginctl_users_text: 'UID USER SESSIONS\n1000 sus 2', loginctl_sessions_text: 'SESSION UID USER SEAT\n1 1000 sus seat0', systemctl_show_manager: 'Version=255', mount_units_text: '/ /boot /var/lib/libvirt',
      failed_units_text: '0 loaded units listed.', networkd_dependencies_text: 'systemd-networkd.service', hostnamectl_status_text: ' Static hostname: kvm-lab-01', timedatectl_status_text: 'Time zone: UTC (UTC, +0000)',
      timedatectl_show_text: 'Timezone=UTC', systemctl_is_system_running: 'running', systemctl_show_environment_text: 'LANG=C.UTF-8', systemctl_list_sockets_text: 'libvirtd.socket', systemctl_list_timers_text: 'apt-daily.timer',
      systemctl_list_jobs_text: 'No jobs running.', systemctl_status_networkd_text: 'active (running)', systemctl_status_resolved_text: 'active (running)', resolved_dependencies_text: 'systemd-resolved.service',
      resolvectl_status_text: 'Global\n  DNS Servers: 1.1.1.1', resolvectl_statistics_text: 'Transactions: 1204', bootctl_status_text: 'System: Firmware: UEFI 2.7', enabled_service_unit_files_text: 'libvirtd.service enabled',
      running_service_units_text: 'libvirtd.service running', loginctl_list_seats_text: 'seat0', journalctl_list_boots_text: '-1 9a8b7c6d Sun 2026-09-14', systemd_analyze_verify_text: '', default_target_dependencies_text: 'multi-user.target',
      product_name: 'PowerEdge R650', sys_vendor: 'Dell Inc.', bios_version: '2.14.1', bios_date: '03/12/2026', board_name: '0PJ2XD', serial_number: 'ABC1234', cpu_model: 'Intel(R) Xeon(R) Gold 6338 CPU @ 2.00GHz', virtualization: 'none',
    },
  },
  { match: '/api/v1/host/filesystems', json: [
    { source: '/dev/nvme0n1p2', fstype: 'ext4', mount_point: '/', size_bytes: 479559442432, used_bytes: 121634816000, avail_bytes: 333424640000, use_percent: 27 },
    { source: '/dev/nvme1n1', fstype: 'xfs', mount_point: '/var/lib/libvirt', size_bytes: 3840755982336, used_bytes: 1299227607040, avail_bytes: 2541528375296, use_percent: 34 },
  ] },
  { match: '/api/v1/host/processes', json: [
    { pid: 4121, user: 'libvirt-qemu', cpu_percent: 41.2, rss_kb: 8388608, command: 'qemu-system-x86', args: '/usr/bin/qemu-system-x86_64 -name guest=chrome-e2e-vm -machine q35' },
    { pid: 1290, user: 'root', cpu_percent: 2.1, rss_kb: 262144, command: 'machina-daemon', args: '/usr/local/bin/machina-daemon' },
    { pid: 988, user: 'root', cpu_percent: 0.4, rss_kb: 61440, command: 'libvirtd', args: '/usr/sbin/libvirtd --timeout 120' },
  ] },
  { match: '/api/v1/host/package-updates', json: { backend: 'apt', probed: true, pending_count: 12, summary: '12 packages can be upgraded', hint: null, error: null, reboot_required: false } },
  { match: '/api/v1/host/net-counters', json: [
    { iface: 'eno1', rx_bytes: 918273645123, rx_packets: 812345678, tx_bytes: 512345678901, tx_packets: 402345678 },
    { iface: 'virbr0', rx_bytes: 12345678, rx_packets: 23456, tx_bytes: 87654321, tx_packets: 34567 },
  ] },
  { match: '/api/v1/host/net-rates', json: { sample_interval_ms: 1000, interfaces: [{ iface: 'eno1', rx_bytes_per_sec: 182000, tx_bytes_per_sec: 94000, rx_packets_per_sec: 210, tx_packets_per_sec: 140 }] } },
  { match: '/api/v1/host/passwd-users', json: [
    { username: 'root', uid: 0, gid: 0, gecos: 'root', home: '/root', shell: '/bin/bash', system_account: true },
    { username: 'sus', uid: 1000, gid: 1000, gecos: 'Sus', home: '/home/sus', shell: '/bin/bash', system_account: false },
  ] },
  { match: '/api/v1/host/groups', json: [
    { name: 'root', gid: 0, members: [] },
    { name: 'libvirt', gid: 110, members: ['sus'] },
    { name: 'sudo', gid: 27, members: ['sus'] },
  ] },
  { match: '/api/v1/host/security-summary', json: { network_backend: 'networkd', firewall_backend: 'nftables', ufw_status_line: 'Status: inactive', firewalld_default_zone: null, selinux_mode: null } },
  { match: '/api/v1/host/libvirt-boot', json: { needs_attention: false, detail: null, systemd_unit: 'libvirtd.service' } },
  { match: '/api/v1/host/hardware-inventory/history', json: { path: '/var/lib/machina/hw-inventory.jsonl', entries: [hwReport] } },
  { match: '/api/v1/host/hardware-inventory', json: hwReport },
  {
    match: '/api/v1/host/linux-observability',
    json: {
      pressure: { cpu: pressure, memory: pressure, io: pressure, available: true },
      disk_io: [{ device: 'nvme0n1', read_bytes: 91827364512, write_bytes: 51234567890, read_ios: 2345678, write_ios: 1234567 }],
      smart: [{ device: '/dev/nvme0n1', passed: true, summary: 'PASSED', probed: true }],
      cgroup: { unified_path: '/sys/fs/cgroup', memory_current_bytes: 103079215104, memory_max_bytes: null, cpu_usage_usec: 912345678, available: true },
      thermal: [{ sensor: 'coretemp', label: 'Package id 0', temp_celsius: 54, critical_celsius: 100 }],
      vm_cgroups: [cgroupVm('chrome-e2e-vm', 8589934592), cgroupVm('win10-msedge', 4294967296)],
      bpf: { available: true, bpftool_path: '/usr/sbin/bpftool', program_count: 14, map_count: 22, cgroup_program_count: 6, tracepoint_count: 3, notes: [] },
    },
  },
  { match: '/api/v1/host/linux-audit', json: { available: true, source: 'ausearch', events: [{ timestamp: '2026-09-27T08:01:12Z', event_type: 'USER_LOGIN', summary: 'login by sus from 10.0.0.5', raw: 'type=USER_LOGIN msg=audit(1790000000.1:412): pid=1234 uid=0 res=success' }], avc_count: 0 } },
  // ---- /audit, /capabilities
  {
    match: /\/api\/v1\/audit\?/,
    json: [
      { timestamp: '2026-09-27T08:12:41Z', action: 'vm.start', target: 'chrome-e2e-vm', result: 'ok', actor: 'sus' },
      { timestamp: '2026-09-27T08:10:03Z', action: 'auth.login', target: 'sus', result: 'success', actor: 'sus' },
      { timestamp: '2026-09-27T07:58:19Z', action: 'vm.delete', target: 'old-test-vm', result: 'denied: forbidden', actor: 'viewer' },
    ],
  },
  {
    match: '/api/v1/capabilities',
    json: {
      host_arch: 'x86_64',
      host_cpu_model: 'Intel Xeon Gold 6338',
      guests: [
        { os_type: 'hvm', arch: 'x86_64', machines: ['pc-q35-8.2', 'pc-i440fx-8.2', 'q35', 'pc', 'microvm', 'isapc'] },
        { os_type: 'hvm', arch: 'aarch64', machines: ['virt', 'virt-8.2'] },
      ],
      spice_available: true,
    },
  },
  {
    match: '/api/v1/openapi.json',
    json: {
      info: { title: 'Machina Daemon API', version: '1.0.0', description: 'REST API for the Machina single-host hypervisor manager.' },
      paths: {
        '/api/v1/vms': {
          get: { summary: 'List virtual machines', tags: ['vms'], responses: { '200': { description: 'OK' } } },
          post: { summary: 'Create a virtual machine', tags: ['vms'], requestBody: { content: { 'application/json': { schema: { type: 'object' } } } } },
        },
        '/api/v1/vms/{name}': { get: { summary: 'Get a VM', tags: ['vms'], parameters: [{ name: 'name', in: 'path', required: true, schema: { type: 'string' } }] } },
        '/api/v1/host/stats': { get: { summary: 'Host CPU / memory / load', tags: ['host'] } },
        '/api/v1/storage/pools': { get: { summary: 'List storage pools', tags: ['storage'] } },
      },
    },
  },
  {
    match: '/api/v1/sysinfo',
    contentType: 'application/xml',
    text: "<sysinfo type='smbios'><bios><entry name='vendor'>Dell Inc.</entry><entry name='version'>2.14.1</entry></bios><system><entry name='manufacturer'>Dell Inc.</entry><entry name='product'>PowerEdge R650</entry><entry name='serial'>ABC1234</entry></system></sysinfo>",
  },
  {
    match: '/api/v1/system/observability-settings',
    json: {
      otlp: { enabled: false, endpoint: '', interval_secs: 30, export_metrics: true, export_logs: false, export_traces: false, authorization: '', authorization_set: false },
      metrics_history: { remote_write_url: '', remote_write_authorization: '', remote_write_authorization_set: false },
      audit: { sign_lines: false },
      config_path: '/etc/machina/config.toml',
    },
  },
  {
    match: '/api/v1/system/os-users/capability',
    json: { canCreateOsUsers: false, canDeleteOsUsers: false, reason: 'mock', libvirtGroupAvailable: true, libvirtGroupName: 'libvirt' },
  },
  {
    match: '/api/v1/integrations/status',
    json: {
      kubevirt: { exec_enabled: false, default_namespace: 'default', default_storage_class: 'local-path', routes: {} },
      automation: { worker_interval_secs: 30, last_tick_unix: 1790000000, alert_rules_total: 4, alert_rules_enabled: 3, alerts_unacknowledged: 1, routes: {} },
      k8s: { kubeconfig_auto_selected: null, kubectl_args_prefix: [], inventory_history_enabled: false, routes: {} },
      run_as_user: { enabled: false, mode: 'off', impersonation_active: false, status_url: '/api/v1/system/run-as-user' },
    },
  },
]

async function extraMocks(page) {
  // `vite preview` proxies every /api* path to the (absent) daemon, so /api-docs never reaches the SPA; serve index.html.
  await page.route(`${BASE}/api-docs`, async (route) => {
    if (route.request().resourceType() !== 'document') return route.fallback()
    const res = await page.request.get(`${BASE}/`)
    return route.fulfill({ status: 200, contentType: 'text/html', body: await res.text() })
  })
  await page.route('**/api/v1/**', async (route) => {
    const url = route.request().url()
    const hit = EXTRA_MOCKS.find((m) => (m.match instanceof RegExp ? m.match.test(url) : url.includes(m.match)))
    if (!hit) return route.fallback()
    return hit.text !== undefined ? route.fulfill({ contentType: hit.contentType || 'text/plain', body: hit.text }) : route.fulfill({ json: hit.json })
  })
}

// ---- runner ------------------------------------------------------------------------------------

const chromePath = process.env.UX_AUDIT_CHROME || (fs.existsSync('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome') ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : undefined)

async function auditOne(browser, route, modeName) {
  const m = MODE[modeName]
  const ctx = await browser.newContext({ viewport: { width: m.width, height: m.height }, ignoreHTTPSErrors: true })
  await ctx.addInitScript((theme) => {
    localStorage.setItem('machina-theme', theme)
    localStorage.setItem('machina-theme-migrated-apple-light-v1', '1')
  }, m.theme)
  const page = await ctx.newPage()
  const findings = []
  const errs = []
  page.on('pageerror', (e) => errs.push(e.message.slice(0, 140)))
  const boundary = []
  page.on('console', (m) => {
    // AppErrorBoundary swallows render errors, so pageerror never fires; keep its message as the detail.
    if (m.type() === 'error' && /AppErrorBoundary caught/.test(m.text())) boundary.push(m.text().replace(/\s+/g, ' ').slice(0, 150))
  })
  try {
    await mockPlatformApi(page)
    await extraMocks(page)
    await page.goto(BASE + route, { waitUntil: 'domcontentloaded', timeout: 30000 })
    await page.waitForTimeout(1600)
    const screen = await page.evaluate(probeScreen)
    if (screen) findings.push({ kind: screen, detail: boundary[0] || '' })
    for (const e of errs.slice(0, 2)) findings.push({ kind: 'page-error', detail: e })
    for (const c of (await page.evaluate(probeContrast)).slice(0, 6)) findings.push({ kind: 'contrast', detail: `"${c.text}" ${c.ratio} ${c.sel}` })
    for (const c of await page.evaluate(probeClip)) findings.push({ kind: 'clip', detail: c })
    if (process.env.UX_AUDIT_DEBUG_SHOT) await page.screenshot({ path: process.env.UX_AUDIT_DEBUG_SHOT, fullPage: false })
    if (process.env.UX_AUDIT_DEBUG_DUMP) {
      console.log(await page.evaluate(() => {
        const vw = innerWidth
        const out = []
        for (const el of document.querySelectorAll('*')) {
          const r = el.getBoundingClientRect()
          if (r.right > vw + 5 && r.width > 15 && r.width < 120 && r.top < 400) {
            out.push(`${el.tagName.toLowerCase()} class="${String(el.className).slice(0, 140)}" right=${Math.round(r.right)} top=${Math.round(r.top)} w=${Math.round(r.width)} parent=${el.parentElement?.tagName.toLowerCase()}.${String(el.parentElement?.className || '').slice(0, 90)}`)
          }
        }
        return out.slice(0, 10).join('\n')
      }))
    }
    if (modeName !== 'dark') for (const f of await page.evaluate(probeA11y)) findings.push(f)
    if (modeName === 'phone') {
      const tap = await page.evaluate(probeTap)
      if (tap.length) findings.push({ kind: 'tap-target', detail: `${tap.length}: ${tap.slice(0, 3).join(' | ')}` })
    }
    if (m.focus) {
      const f = await probeFocus(page)
      for (const b of f.bad.slice(0, 4)) findings.push({ kind: 'no-focus-indicator', detail: b })
    }
  } catch (e) {
    findings.push({ kind: 'audit-failed', detail: String(e).slice(0, 120) })
  }
  await ctx.close()
  return { route, mode: modeName, findings }
}

async function main() {
  const browser = await chromium.launch({ headless: true, executablePath: chromePath, args: ['--no-sandbox'] })
  const jobs = []
  for (const mode of MODES) for (const route of routes) jobs.push([route, mode])
  const results = []
  let next = 0
  let done = 0
  await Promise.all(
    Array.from({ length: CONCURRENCY }, async () => {
      while (next < jobs.length) {
        const [route, mode] = jobs[next++]
        results.push(await auditOne(browser, route, mode))
        if (++done % 25 === 0) process.stderr.write(`  ${done}/${jobs.length}\n`)
      }
    }),
  )
  await browser.close()
  results.sort((a, b) => (a.route + a.mode).localeCompare(b.route + b.mode))
  const report = { generatedAt: new Date().toISOString(), base: BASE, routes: routes.length, modes: MODES, results }
  if (OUT) {
    fs.mkdirSync(path.dirname(path.resolve(OUT)), { recursive: true })
    fs.writeFileSync(OUT, JSON.stringify(report, null, 1))
  }
  summarize(report)
}

const count = (report) => {
  const by = {}
  for (const r of report.results) for (const f of r.findings) by[f.kind] = (by[f.kind] || 0) + 1
  return by
}

function summarize(report) {
  const now = count(report)
  const kinds = new Set(Object.keys(now))
  let base = null
  if (BASELINE) {
    base = JSON.parse(fs.readFileSync(BASELINE, 'utf8'))
    for (const k of Object.keys(count(base))) kinds.add(k)
  }
  const baseCount = base ? count(base) : {}
  console.log(`\n== UX audit: ${report.results.length} runs (${report.routes} routes x ${report.modes.join('/')})`)
  console.log('kind'.padEnd(24) + (base ? 'baseline'.padStart(10) + 'now'.padStart(8) + 'delta'.padStart(8) : 'count'.padStart(8)))
  for (const k of [...kinds].sort()) {
    const n = now[k] || 0
    const b = baseCount[k] || 0
    console.log(k.padEnd(24) + (base ? String(b).padStart(10) + String(n).padStart(8) + String(n - b >= 0 ? '+' + (n - b) : n - b).padStart(8) : String(n).padStart(8)))
  }
  const hard = report.results.filter((r) => r.findings.some((f) => ['page-error', 'error-screen', 'blank', 'audit-failed'].includes(f.kind)))
  console.log(`\nroutes with hard failures (page-error / error-screen / blank / audit-failed): ${hard.length}`)
  for (const r of hard.slice(0, 15)) console.log(`  ${r.route} [${r.mode}] ${r.findings.filter((f) => ['page-error', 'error-screen', 'blank', 'audit-failed'].includes(f.kind)).map((f) => f.kind + (f.detail ? ' ' + f.detail : '')).join('; ')}`)
  if (base) {
    const key = (r, f) => `${r.route}|${r.mode}|${f.kind}|${f.detail}`
    const before = new Set(base.results.flatMap((r) => r.findings.map((f) => key(r, f))))
    const fresh = report.results.flatMap((r) => r.findings.filter((f) => !before.has(key(r, f))).map((f) => `${r.route} [${r.mode}] ${f.kind} ${f.detail}`))
    console.log(`\nnew findings vs baseline: ${fresh.length}`)
    for (const l of fresh.slice(0, 25)) console.log('  ' + l)
  }
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})

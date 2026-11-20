#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Seed docs/ux-wiring-live-manifest.json from nav sources + scraped tabs/filters + hand-curated actions.
 */
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const OUT = path.join(ROOT, 'docs/ux-wiring-live-manifest.json')
const NAV_TS = path.join(ROOT, 'web/src/utils/platformNav.ts')
const APP_TSX = path.join(ROOT, 'web/src/App.tsx')
const PLATFORM_PAGES = path.join(ROOT, 'web/src/pages/platform')
const SETTINGS_HUB = path.join(PLATFORM_PAGES, 'PlatformSettingsHub.tsx')

const POWER_ONLY = new Set([
  '/platform/applications', '/platform/networks', '/platform/content', '/platform/templates',
  '/platform/migration', '/platform/tasks', '/platform/activity', '/platform/blueprints',
  '/platform/projects', '/platform/zeus', '/platform/zeus/security/firewall',
  '/platform/maintenance', '/platform/recommendations', '/platform/topology',
  '/platform/events', '/platform/enroll', '/platform/reports', '/platform/storage',
  '/fleet-cloud', '/k8s', '/fleet',
])

const ADVANCED_ONLY = new Set([
  '/platform/developer', '/platform/zeus/security/policies', '/platform/policy',
  '/platform/observability', '/platform/users', '/platform/enterprise', '/platform/webhooks',
  '/platform/api-keys', '/platform/zeus/security/ports', '/platform/zeus/security/services',
  '/platform/zeus/security/activity', '/platform/zeus/security/compliance',
  '/platform/zeus/security/k8s', '/platform/zeus/security/cloud', '/platform/zeus/security/connectivity',
])

function tierForPath(p) {
  if (ADVANCED_ONLY.has(p) || [...ADVANCED_ONLY].some((x) => p.startsWith(`${x}/`))) return 'advanced'
  if (POWER_ONLY.has(p) || p.startsWith('/fleet-cloud') || p.startsWith('/k8s') || p.startsWith('/fleet')) return 'power'
  return 'normal'
}

function parsePlatformPaths() {
  const src = fs.readFileSync(NAV_TS, 'utf8')
  const fromSidebar = [...src.matchAll(/to: '(\/platform[^']*)'/g)].map((m) => m[1])
  const fromLabels = [...src.matchAll(/'(\/platform[^']+)':/g)].map((m) => m[1])
  const appSrc = fs.readFileSync(APP_TSX, 'utf8')
  const fromApp = [...appSrc.matchAll(/<Route path="([^"]+)" element=\{<Platform/g)].map((m) => {
    const routePath = m[1]
    return routePath === '' ? '/platform' : `/platform/${routePath}`
  })
  return [...new Set([...fromSidebar, ...fromLabels, ...fromApp])].sort()
}

function parseRouteComponents() {
  const src = fs.readFileSync(APP_TSX, 'utf8')
  const map = new Map()
  for (const m of src.matchAll(/<Route path="([^"]+)" element=\{<([A-Za-z0-9]+)/g)) {
    const routePath = m[1]
    const component = m[2]
    const full = routePath === '' ? '/platform' : routePath.startsWith('/') ? routePath : `/platform/${routePath}`
    map.set(component, full)
  }
  return map
}

function walkTsx(dir) {
  const out = []
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, ent.name)
    if (ent.isDirectory()) out.push(...walkTsx(p))
    else if (ent.name.endsWith('.tsx')) out.push(p)
  }
  return out
}

function extractLabelsFromArrayBlock(block) {
  const labels = []
  for (const m of block.matchAll(/label:\s*['"]([^'"]+)['"]/g)) labels.push(m[1])
  return labels
}

function extractTabLabels(src) {
  const labels = new Set()
  for (const m of src.matchAll(/const\s+[A-Z0-9_]+\s*(?::[^=]+)?=\s*\[([\s\S]*?)\]/g)) {
    for (const label of extractLabelsFromArrayBlock(m[1])) labels.add(label)
  }
  for (const m of src.matchAll(/<DetailTabs[\s\S]*?primary=\{\[([\s\S]*?)\]\}/g)) {
    for (const label of extractLabelsFromArrayBlock(m[1])) labels.add(label)
  }
  for (const m of src.matchAll(/more=\{\[([\s\S]*?)\]\}/g)) {
    for (const label of extractLabelsFromArrayBlock(m[1])) labels.add(label)
  }
  return [...labels]
}

function extractFilterPillLabels(src) {
  const labels = new Set()
  for (const m of src.matchAll(/<PlatformFilterPills[\s\S]*?options=\{\[([\s\S]*?)\]\}/g)) {
    for (const label of extractLabelsFromArrayBlock(m[1])) labels.add(label)
  }
  return [...labels]
}

function extractTabDefs(src) {
  const tabs = []
  const seen = new Set()
  for (const m of src.matchAll(/\{\s*id:\s*'([^']+)',\s*label:\s*'([^']+)'/g)) {
    const id = m[1]
    if (seen.has(id)) continue
    seen.add(id)
    tabs.push({ id, label: m[2] })
  }
  return tabs
}

function scrapePageActions() {
  const routeComponents = parseRouteComponents()
  const componentToFile = new Map()
  for (const file of walkTsx(PLATFORM_PAGES)) {
    const base = path.basename(file, '.tsx')
    componentToFile.set(base, file)
  }

  const byPath = new Map()
  for (const [component, routePath] of routeComponents.entries()) {
    const file = componentToFile.get(component)
    if (!file) continue
    const src = fs.readFileSync(file, 'utf8')
    const tabDefs = extractTabDefs(src)
    const tabs = tabDefs.map(({ label }) => ({ kind: 'tab', label }))
    const pills = extractFilterPillLabels(src).map((label) => ({ kind: 'filterPill', label }))
    if (tabs.length === 0 && pills.length === 0 && tabDefs.length === 0) continue
    byPath.set(routePath, { tabs, pills, tabDefs })
  }
  return byPath
}

function scrapeSettingsSections() {
  const src = fs.readFileSync(SETTINGS_HUB, 'utf8')
  const labels = []
  for (const m of src.matchAll(/\{\s*id:\s*'[^']+',\s*label:\s*'([^']+)'/g)) labels.push(m[1])
  return labels.map((label) => ({ kind: 'settingsSection', label }))
}

const ACTION_OVERRIDES = {
  '/platform/backups': {
    headingPattern: 'Backup|Time Machine',
    actions: [
      { kind: 'tab', label: 'Timeline' },
      { kind: 'tab', label: 'Destinations' },
    ],
  },
  '/platform/developer': {
    headingPattern: 'Developer',
    tier: 'advanced',
    actions: [
      { kind: 'tab', label: 'API Console' },
      { kind: 'click', role: 'button', name: 'Controller (fleet)' },
      { kind: 'click', role: 'button', name: 'Host (daemon)' },
    ],
  },
  '/platform/settings': {
    headingPattern: 'Settings|General',
  },
  '/platform/tasks': {
    headingPattern: 'Tasks|Orchestration',
  },
  '/platform/zeus/security/policies': {
    headingPattern: 'Policy Studio',
    tier: 'advanced',
  },
  '/platform/integrations': {
    headingPattern: 'Apps & Integrations',
  },
  '/api-docs': {
    shell: 'classic',
    headingPattern: 'Machina Host|Machina API|API',
  },
}

const CLASSIC_ROUTES = [
  '/', '/vms', '/fleet', '/storage', '/networks', '/disk-images', '/snapshots',
  '/node', '/events', '/capabilities', '/devices', '/nwfilters', '/secrets',
  '/backups', '/host-networking', '/host-ssh', '/audit', '/import', '/api-docs', '/services',
  '/system-check', '/logs', '/settings', '/jobs', '/create', '/ssh', '/mission-control',
  '/admin/sessions',
]

const CLASSIC_DYNAMIC_ROUTES = [
  { path: '/vms/:name', headingPattern: 'VM|Virtual Machine|Daily access', resolve: 'classicVm' },
  { path: '/vms/:name/consolehub', headingPattern: 'Console|ConsoleHub', resolve: 'classicVm' },
  { path: '/storage/:pool', headingPattern: 'Storage|Pool', resolve: 'storagePool' },
]

const FLEET_CLOUD_ROUTES = [
  '/fleet-cloud', '/fleet-cloud/instances', '/fleet-cloud/images', '/fleet-cloud/volumes',
  '/fleet-cloud/volume-snapshots', '/fleet-cloud/flavors', '/fleet-cloud/server-groups',
  '/fleet-cloud/networking', '/fleet-cloud/topology', '/fleet-cloud/floating-ips',
  '/fleet-cloud/heat', '/fleet-cloud/load-balancers', '/fleet-cloud/identity',
  '/fleet-cloud/keypairs', '/fleet-cloud/security-groups', '/fleet-cloud/migrations',
  '/fleet-cloud/create',
]

const FLEET_CLOUD_DETAIL_ROUTES = [
  '/fleet-cloud/instances/:id',
  '/fleet-cloud/instances/:id/interfaces',
  '/fleet-cloud/instances/:id/console',
  '/fleet-cloud/images/:id',
  '/fleet-cloud/volumes/:id',
  '/fleet-cloud/floating-ips/:id',
  '/fleet-cloud/flavors/:id',
  '/fleet-cloud/hypervisors/:id',
  '/fleet-cloud/server-groups/:id',
  '/fleet-cloud/networks/:id',
  '/fleet-cloud/subnets/:id',
  '/fleet-cloud/routers/:id',
  '/fleet-cloud/ports/:id',
  '/fleet-cloud/volume-transfers/:id',
  '/fleet-cloud/volume-snapshots/:id',
  '/fleet-cloud/heat/:name/:id',
  '/fleet-cloud/load-balancers/:id',
  '/fleet-cloud/identity/projects/:id',
  '/fleet-cloud/identity/users/:id',
  '/fleet-cloud/security-groups/:id',
]

const K8S_ROUTES = ['/k8s', '/k8s/workloads', '/k8s/kata']

/** Query param name for tab state on specific platform routes. */
const TAB_PARAM_KEY = {
  '/platform/observability': 'lens',
  '/platform/vms': 'lens',
}

const MACHINE_FINDER_LENSES = ['table', 'topology', 'timeline', 'heatmap', 'migration']

const SETTINGS_SECTIONS = [
  'general', 'zeus', 'ai-providers', 'security', 'network', 'users', 'stage-manager',
  'keychain', 'policy', 'api-keys', 'webhooks', 'reports', 'console', 'resources',
  'updates', 'integrations', 'support', 'about',
]

const PLATFORM_QUERY_SWEEPS = [
  { path: '/platform/backups?tab=destinations', headingPattern: 'Backup|Destination|Time Machine' },
  { path: '/platform/maintenance?tab=mission', headingPattern: 'Maintenance|Mission' },
  { path: '/platform/developer?tab=console', headingPattern: 'Developer|API Console' },
  { path: '/platform/zeus?tab=security', headingPattern: 'Zeus|Security' },
  { path: '/platform/notifications?tab=rules', headingPattern: 'Notification|Alert' },
  { path: '/platform/gpu?tab=placement', headingPattern: 'GPU|CUDA' },
  { path: '/platform/datacenter?tab=racks', headingPattern: 'Datacenter|Rack' },
  { path: '/platform/cloud-init?tab=profiles', headingPattern: 'Cloud|Init' },
  { path: '/platform/vm-builder?tab=compose', headingPattern: 'VM Builder|Compose' },
  { path: '/platform/create-iso?tab=wizard', headingPattern: 'ISO|Create' },
  { path: '/platform/soc?tab=siem', headingPattern: 'SOC|SIEM' },
  { path: '/platform/zeus/incidents?tab=active', headingPattern: 'Incident' },
  { path: '/platform/zeus/approvals?tab=pending', headingPattern: 'Approval' },
  { path: '/platform/zeus/rightsizing?tab=candidates', headingPattern: 'Rightsiz|Recommend' },
  { path: '/platform/zeus/security/hunt?tab=queries', headingPattern: 'Threat|Hunt' },
  { path: '/platform/zeus/security/enforcement?tab=rules', headingPattern: 'Enforcement|Runtime' },
]

const HEADING_DEFAULTS = {
  '/platform': 'Dashboard|Production Cluster|Platform',
  '/platform/vms': 'Finder|Virtual Machines',
  '/platform/hosts': 'Hosts',
  '/platform/storage': 'Storage|Disk',
  '/platform/networks': 'Networks',
  '/fleet-cloud': 'Fleet Cloud|Overview',
  '/k8s': 'Kubernetes|KubeVirt|Cluster',
}

function entry(shell, p, extra = {}) {
  const basePath = p.split('?')[0]
  const base = {
    id: `${shell}:${p}`,
    shell,
    path: p,
    tier: tierForPath(basePath),
    headingPattern: HEADING_DEFAULTS[basePath] ?? '.+',
    requires: null,
    actions: [],
    resolve: extra.resolve ?? null,
  }
  const ov = ACTION_OVERRIDES[basePath] ?? {}
  return { ...base, ...extra, ...ov, path: p, shell }
}

function addEntry(entries, seen, e) {
  if (seen.has(e.id)) return
  seen.add(e.id)
  entries.push(e)
}

function tabSweepEntries(routePath, tabDefs, scraped) {
  if (!tabDefs?.length) return []
  const paramKey = TAB_PARAM_KEY[routePath] ?? 'tab'
  const defaultTab = tabDefs[0]?.id
  const out = []
  for (const { id } of tabDefs) {
    if (id === defaultTab) continue
    const sweepPath = `${routePath}?${paramKey}=${encodeURIComponent(id)}`
    out.push(entry('platform', sweepPath, {
      headingPattern: HEADING_DEFAULTS[routePath] ?? '.+',
      actions: [],
    }))
  }
  return out
}

function mergeActions(pathKey, scraped, settingsSections) {
  const ov = ACTION_OVERRIDES[pathKey]?.actions ?? []
  const auto = []
  const s = scraped.get(pathKey)
  if (s) auto.push(...s.tabs, ...s.pills)
  if (pathKey === '/platform/settings') auto.push(...settingsSections)
  const seen = new Set()
  const merged = []
  for (const a of [...ov, ...auto]) {
    const key = `${a.kind}:${a.label ?? a.name ?? ''}`
    if (seen.has(key)) continue
    seen.add(key)
    merged.push(a)
  }
  return merged
}

function main() {
  const scraped = scrapePageActions()
  const settingsSections = scrapeSettingsSections()
  const entries = []
  const seen = new Set()

  for (const p of parsePlatformPaths()) {
    addEntry(entries, seen, entry('platform', p, { actions: mergeActions(p, scraped, settingsSections) }))
    const s = scraped.get(p)
    if (s?.tabDefs?.length) {
      for (const e of tabSweepEntries(p, s.tabDefs, scraped)) addEntry(entries, seen, e)
    }
  }

  for (const lens of MACHINE_FINDER_LENSES) {
    addEntry(entries, seen, entry('platform', `/platform/vms?lens=${lens}`, {
      headingPattern: 'Machine Finder|Finder',
    }))
  }

  for (const section of SETTINGS_SECTIONS) {
    if (section === 'general') continue
    addEntry(entries, seen, entry('platform', `/platform/settings?section=${section}`, {
      headingPattern: 'Settings|General|Security|Zeus',
      tier: 'normal',
    }))
  }

  for (const q of PLATFORM_QUERY_SWEEPS) {
    addEntry(entries, seen, entry('platform', q.path, { headingPattern: q.headingPattern }))
  }

  for (const p of CLASSIC_ROUTES) {
    if (p === '/') addEntry(entries, seen, entry('classic', p, { headingPattern: 'Machina|Dashboard|VMs' }))
    else addEntry(entries, seen, entry('classic', p))
  }

  for (const d of CLASSIC_DYNAMIC_ROUTES) {
    addEntry(entries, seen, entry('classic', d.path, {
      headingPattern: d.headingPattern,
      resolve: d.resolve,
    }))
  }

  for (const p of FLEET_CLOUD_ROUTES) {
    addEntry(entries, seen, entry('fleet-cloud', p, { tier: 'power' }))
  }

  for (const p of FLEET_CLOUD_DETAIL_ROUTES) {
    addEntry(entries, seen, entry('fleet-cloud', p, { tier: 'power' }))
  }

  for (const p of K8S_ROUTES) {
    addEntry(entries, seen, entry('k8s', p, { requires: 'k8s', tier: 'power' }))
  }

  // Dynamic platform detail sweeps (resolved at runtime in live-ux-wiring.spec.ts).
  for (const p of [
    '/platform/hosts/:id',
    '/platform/vms/:id',
    '/platform/vms/:id/consolehub',
    '/platform/zeus/machines/:hostId',
    '/platform/zeus/security/firewall/:id',
  ]) {
    addEntry(entries, seen, entry('platform', p, { resolve: 'platformResource' }))
  }

  entries.sort((a, b) => a.path.localeCompare(b.path))

  const manifest = {
    version: 2,
    generated_at: new Date().toISOString(),
    entries,
  }

  fs.writeFileSync(OUT, JSON.stringify(manifest, null, 2) + '\n')
  const withActions = entries.filter((e) => (e.actions?.length ?? 0) > 0).length
  console.log(`Wrote ${OUT} (${entries.length} entries, ${withActions} with actions)`)
}

main()

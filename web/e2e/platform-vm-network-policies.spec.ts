// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect as baseExpect, type Page, type Route } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

// Under a full parallel run, lazy chunks and toasts can take longer than the 5 s default.
const expect = baseExpect.configure({ timeout: 15_000 })

const PAGE = '/platform/zyra/security/network-policies'

const dbPolicy = {
  name: 'db-from-web',
  kind: 'CiliumNetworkPolicy',
  description: 'Only web VMs reach the database',
  labels: {},
  annotations: {},
  specs: [
    {
      endpointSelector: { matchLabels: { app: 'db' } },
      ingress: [{ fromEndpoints: [{ matchLabels: { app: 'web' } }], toPorts: [{ ports: [{ port: '5432', protocol: 'TCP' }] }] }],
    },
  ],
  yaml: 'apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: db-from-web\n',
  selected_vms: ['db-1'],
  enabled: true,
}

const endpoints = [
  { name: 'web-1', host: 'host-1', identity: 4101, labels: { app: 'web' }, addresses: ['10.0.0.5'], ingress_enforced: false, egress_enforced: true, policies: ['web-to-services'] },
  { name: 'db-1', host: 'host-1', identity: 4102, labels: { app: 'db' }, addresses: ['10.0.0.9'], ingress_enforced: true, egress_enforced: false, policies: ['db-from-web'] },
]

const flow = {
  ts: '2026-10-04T08:00:00Z',
  iface: 'vnet0',
  vm: 'web-1',
  direction: 'egress',
  src: '10.0.0.5',
  src_port: 40100,
  dst: '10.0.0.9',
  dst_port: 22,
  src_vm: 'web-1',
  dst_vm: 'db-1',
  src_labels: { app: 'web' },
  dst_labels: { app: 'db' },
  src_identity: 4101,
  dst_identity: 4102,
  proto: 'TCP',
  tcp_flags: 'SYN',
  bytes: 60,
  verdict: 'DROPPED',
  drop_reason: 'policy-denied',
  policy: 'db-from-web',
}

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

const recent = new Date(Date.now() - 60_000).toISOString().replace(/\.\d+Z$/, 'Z')

const edges = [
  {
    src: 'web-1', dst: 'db-1', src_vm: 'web-1', dst_vm: 'db-1', direction: 'egress', proto: 'TCP', port: 5432,
    verdict: 'FORWARDED', count: 120, bytes: 7200, first_seen: recent, last_seen: recent, policy: 'db-from-web',
    l7: [{ kind: 'http', request: 'GET api/users/{id}', count: 40, denied: 2, status: { '2xx': 35, '5xx': 3 }, latency_n: 38, latency_ms_total: 760, latency_ms_max: 95 }],
  },
  {
    src: 'web-1', dst: 'db-1', src_vm: 'web-1', dst_vm: 'db-1', direction: 'egress', proto: 'TCP', port: 22,
    verdict: 'DROPPED', drop_reason: 'default-deny', count: 4, bytes: 240, first_seen: recent, last_seen: recent,
  },
  {
    src: 'web-1', dst: '10.0.0.1', src_vm: 'web-1', dst_entity: 'host', direction: 'egress', proto: 'UDP', port: 53,
    verdict: 'FORWARDED', count: 9, bytes: 540, first_seen: recent, last_seen: recent,
  },
]

const alerts = [
  { ts: recent, kind: 'port_scan', severity: 'high', src: 'web-1', src_vm: 'web-1', dst: 'db-1', detail: 'web-1 probed 31 ports on db-1 within a minute', count: 31 },
]

const learned = {
  yaml: 'apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: learned-db\nspec:\n  endpointSelector:\n    matchLabels: {app: db}\n',
  policies: [{ name: 'learned-db', vms: ['db-1'], ingress_rules: 1, egress_rules: 0 }],
  edges_used: 3,
  edges_skipped: 1,
  notes: [],
}

const replayResult = {
  evaluated: 3,
  unchanged: 2,
  would_break: [{ src: 'web-1', dst: 'db-1', proto: 'TCP', port: 5432, request: 'GET api/users/{id}', flows: 40, last_seen: recent, before: 'ALLOWED', after: 'DENIED: default deny at ingress of db-1' }],
  would_allow: [],
  flows_breaking: 40,
}

async function mockNetpol(page: Page) {
  const policies = [dbPolicy]
  const applied: string[] = []
  const quarantines: Array<Record<string, unknown>> = []
  const grants: Array<Record<string, unknown>> = []
  const feeds: Array<Record<string, unknown>> = []
  const blankNet = { isolation: 'inherit', allow_host: true, egress_restricted: false, egress_allow: [], egress_ips: {} }
  const projectNet: Record<string, Record<string, unknown>> = {
    '*': { ...blankNet, project: '*', isolation: 'open' },
    shop: { ...blankNet, project: 'shop' },
  }
  const projectVms: Record<string, string[]> = { shop: ['web-1', 'db-1'], lab: ['lab-1'] }
  const evidenceBody = '{"kind":"machina.io/segmentation-evidence/v1","scope":"fleet","digest":"abc123"}'
  await page.route('**/vms/*/quarantine', (route) => {
    const req = route.request()
    const vm = decodeURIComponent(new URL(req.url()).pathname.split('/').slice(-2)[0])
    if (req.method() === 'DELETE') {
      const i = quarantines.findIndex((q) => q.vm === vm)
      if (i >= 0) quarantines.splice(i, 1)
      return json(route, { released: i >= 0, vm })
    }
    const b = req.postDataJSON() as { secs: number; allow_host_ssh?: boolean; reason?: string }
    const q = {
      vm,
      since: recent,
      until: recent,
      remaining_secs: b.secs,
      allow: b.allow_host_ssh ? [{ direction: 'ingress', peer: 'host', proto: 'tcp', port: 22 }] : [],
      reason: b.reason ?? '',
      by: 'sus',
      taps: ['vnet0'],
    }
    quarantines.push(q)
    return json(route, q)
  })
  await page.route('**/flows/edges**', (route) => json(route, { items: edges }))
  await page.route('**/flows/alerts**', (route) => json(route, { items: alerts }))
  await page.route('**/flows/stream**', (route) =>
    route.fulfill({
      status: 200,
      headers: { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' },
      body: `event: flow\ndata: ${JSON.stringify(flow)}\n\n`,
    }),
  )
  await page.route('**/vm-network-policies**', async (route) => {
    const req = route.request()
    const path = new URL(req.url()).pathname.replace(/^.*\/vm-network-policies/, '')
    const method = req.method()
    if (path === '' && method === 'GET') return json(route, { items: policies, warnings: [] })
    if (path === '' && method === 'POST') {
      const yaml = String((req.postDataJSON() as { yaml?: string }).yaml ?? '')
      const name = /name:\s*(\S+)/.exec(yaml)?.[1] ?? 'unnamed'
      applied.push(name)
      if (!policies.some((p) => p.name === name)) {
        policies.push({ ...dbPolicy, name, description: 'Web VMs reach services', selected_vms: ['web-1'], yaml })
      }
      return json(route, { applied: [name], warnings: [], sync: { ok: true, vms: 2, rules: 3, peers: 1, warnings: [] } })
    }
    if (path === '/validate') {
      const yaml = String((req.postDataJSON() as { yaml?: string }).yaml ?? '')
      const services = yaml.includes('toServices')
      return json(route, {
        valid: true,
        errors: [],
        warnings: [],
        compile_warnings: services ? ['web-to-services spec.egress[1]: toServices[0] selects no service'] : [],
        policies: [
          {
            ...dbPolicy,
            name: services ? 'web-to-services' : 'db-from-web',
            selected_vms: services ? ['web-1'] : ['db-1'],
            specs: services
              ? [{ endpointSelector: { matchLabels: { app: 'web' } }, egress: [{ toServices: [{ k8sService: { serviceName: 'shop-lb', namespace: 'shop' } }] }] }]
              : dbPolicy.specs,
          },
        ],
        rules: services ? 2 : 1,
        endpoints,
        selectors: [{ policy: 'web-to-services', path: 'spec.endpointSelector', selector: 'app=web', vms: ['web-1'] }],
      })
    }
    if (path === '/trace') {
      const q = req.postDataJSON() as { from: string; to: string; port?: number }
      const allowed = q.port === 5432
      return json(route, {
        allowed,
        from: { input: q.from, vm: q.from, identity: 4101, kind: 'vm' },
        to: { input: q.to, vm: q.to, identity: 4102, kind: 'vm' },
        protocol: 'TCP',
        port: q.port ?? 0,
        egress: { direction: 'egress', vm: q.from, enforced: false, verdict: 'no-policy' },
        ingress: allowed
          ? { direction: 'ingress', vm: q.to, enforced: true, verdict: 'allowed', rule: 'db-from-web spec.ingress[0]' }
          : { direction: 'ingress', vm: q.to, enforced: true, verdict: 'default-deny' },
        summary: allowed ? 'ALLOWED' : `DENIED: default deny at ingress of ${q.to}`,
      })
    }
    if (path === '/status') {
      return json(route, {
        policies: policies.length,
        managed_by: 'local',
        last_sync: { at: '2026-10-04T08:00:00Z', ok: true, vms: 2, rules: 3, peers: 1, warnings: [] },
        edge: { owner: 'daemon', enforcing: false, flow_log: true, taps: [] },
        enforcement: { mode: 'observe' },
        bpfd_available: true,
        cilium: null,
      })
    }
    if (path === '/learn') return json(route, learned)
    if (path === '/replay') return json(route, replayResult)
    if (path === '/draft' && method === 'POST') {
      const prompt = String((req.postDataJSON() as { prompt?: string }).prompt ?? '')
      if (!/web/.test(prompt)) {
        return json(route, { error: 'could not draft a policy from that description: make it nice — not understood' }, 422)
      }
      return json(route, {
        yaml: 'apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: nl-only-web-to-db\nspec:\n  description: only web can reach db\n',
        source: 'rules',
        notes: ['nl-only-web-to-db: only VMs app=web can reach VMs app=db on 5432/TCP'],
        unparsed: [],
        preview: { valid: true, errors: [], warnings: [], compile_warnings: [], policies: [], rules: 1, endpoints, selectors: [] },
        replay: replayResult,
      })
    }
    if (path === '/endpoints') return json(route, { items: endpoints })
    if (path === '/selectors') return json(route, { items: [{ policy: 'db-from-web', path: 'spec.endpointSelector', selector: 'app=db', vms: ['db-1'] }] })
    if (path === '/fqdn-cache') return json(route, { items: [] })
    if (path === '/auth') return json(route, { items: [] })
    if (path === '/quarantines') return json(route, { items: quarantines })
    if (path === '/jit' && method === 'GET') return json(route, { items: grants })
    if (path === '/jit' && method === 'POST') {
      const b = req.postDataJSON() as { from: string; to: string; port: number; protocol: string; secs: number; reason?: string }
      const g = { name: `jit-${b.from}-to-${b.to}-${b.port}-x`, from: b.from, to: b.to, port: b.port, protocol: b.protocol, expires_at: recent, remaining_secs: b.secs, reason: b.reason ?? '', granted_by: 'sus' }
      grants.push(g)
      policies.push({ ...dbPolicy, name: g.name, description: `Temporary access ${b.from} → ${b.to}:${b.port}/${b.protocol}`, selected_vms: [b.to, b.from] })
      return json(route, { granted: g, policy: g.name, sync: { ok: true, vms: 2, rules: 3, peers: 1, warnings: [] } })
    }
    if (path === '/threat-feeds') {
      const blocked = feeds.some((f) => f.block)
        ? [{ address: '203.0.113.7', domain: 'evil.example', feed: 'lab', vm: 'web-1', expires_in_secs: 600 }]
        : []
      return json(route, { feeds, blocked, watched_vms: feeds.length ? 2 : 0 })
    }
    if (path.startsWith('/threat-feeds/')) {
      const name = decodeURIComponent(path.split('/')[2])
      const i = feeds.findIndex((f) => f.name === name)
      if (method === 'DELETE') {
        if (i >= 0) feeds.splice(i, 1)
        return json(route, { removed: i >= 0, name })
      }
      const b = req.postDataJSON() as { domains?: string[]; url?: string; block: boolean }
      const f = { name, source: b.url ?? '', block: b.block, domains: b.domains?.length ?? 0, updated: recent }
      if (i >= 0) feeds.splice(i, 1, f)
      else feeds.push(f)
      return json(route, f)
    }
    if (path === '/projects') {
      const def = projectNet['*']
      const isolated = (n: string) => {
        const own = projectNet[n]?.isolation
        return own === 'isolated' || (own !== 'open' && def.isolation === 'isolated')
      }
      return json(route, {
        default: def,
        items: Object.keys(projectVms).map((n) => ({
          project: n,
          explicit: n in projectNet,
          settings: projectNet[n] ?? { ...blankNet, project: n },
          isolated: isolated(n),
          allow_host: true,
          vms: projectVms[n],
          policies: isolated(n) ? [`project-isolation-${n}-a1b2c3`] : [],
          cross_host_nat: n === 'lab' ? { project: 'lab', hosts: ['hv1', 'hv2'], subnets: ['192.168.122.0/24'] } : null,
          egress_gaps: Object.keys((projectNet[n]?.egress_ips ?? {}) as Record<string, string>).length ? [{ project: n, vm: 'db-1', host: 'hv2', blocked: Boolean(projectNet[n]?.egress_ip_required) }] : [],
        })),
        warnings: ['Project lab has VMs on hv1, hv2 behind per-host NAT (192.168.122.0/24)'],
      })
    }
    if (path.endsWith('/preview')) {
      return json(route, { project: 'shop', describe: 'Project shop: isolated', summary: 'would break 2 recorded connections (9 flows) and newly allow 0', replay: { evaluated: 4, unchanged: 2, would_break: [{}, {}], would_allow: [], flows_breaking: 9 } })
    }
    if (path.startsWith('/projects/')) {
      const name = decodeURIComponent(path.split('/')[2])
      if (new URL(req.url()).searchParams.get('propose') === '1') {
        return json(route, { pending: { id: 'p1' }, preview: 'would break 2 recorded connections (9 flows) and newly allow 0' })
      }
      if (method === 'DELETE') {
        delete projectNet[name]
        return json(route, { deleted: name })
      }
      projectNet[name] = { ...(req.postDataJSON() as Record<string, unknown>), project: name }
      return json(route, { project: projectNet[name] })
    }
    if (path === '/egress-ips') {
      const rules = Object.values(projectNet).flatMap((n) =>
        Object.values((n.egress_ips ?? {}) as Record<string, string>).map((ip) => ({ project: n.project, egress_ip: ip, sources: ['10.0.0.5'] })),
      )
      return json(route, { items: [{ hostname: 'hv1', rules, exclude: [], active: rules.length > 0, skipped: [], error: null }], errors: [] })
    }
    if (path === '/evidence') {
      const q = new URL(req.url()).searchParams
      const md = q.get('format') === 'md'
      const scoped = q.get('project')
        ? JSON.stringify({ kind: 'machina.io/segmentation-evidence/v1', scope: 'fleet', digest: 'def456', matrix: [
            { from: q.get('project'), to: q.get('project'), allowed: ['TCP/22'], denied: [] },
            { from: q.get('project'), to: '(other projects)', allowed: [], denied: ['TCP/22', 'TCP/443'] },
          ] })
        : evidenceBody
      return route.fulfill({
        status: 200,
        headers: { 'content-type': md ? 'text/markdown' : 'application/json' },
        body: md ? '# Segmentation evidence\n' : scoped,
      })
    }
    if (method === 'DELETE') {
      const name = decodeURIComponent(path.slice(1))
      policies.splice(policies.findIndex((p) => p.name === name), 1)
      const gi = grants.findIndex((g) => g.name === name)
      if (gi >= 0) grants.splice(gi, 1)
      return json(route, { deleted: name })
    }
    return json(route, { error: `unmocked ${method} ${path}` }, 404)
  })
  const vmProjects: Record<string, string> = { 'web-1': 'shop', 'db-1': 'shop', 'lab-1': 'lab', 'spare-1': '' }
  await page.route(/\/platform\/controller\/api\/v1\/vms(\/[^/?]+)?(\?.*)?$/, async (route) => {
    const req = route.request()
    const id = new URL(req.url()).pathname.split('/vms/')[1]
    if (req.method() === 'PATCH' && id) {
      const name = id.replace(/^id-/, '')
      const project = (req.postDataJSON() as { project: string }).project
      for (const list of Object.values(projectVms)) {
        const i = list.indexOf(name)
        if (i >= 0) list.splice(i, 1)
      }
      if (project) (projectVms[project] ??= []).push(name)
      vmProjects[name] = project
      return json(route, { id, name, project })
    }
    if (id) return route.fallback()
    return json(route, Object.entries(vmProjects).map(([name, project]) => ({ id: `id-${name}`, name, project, desired_state: 'running', observed_state: 'running', lifecycle_phase: 'ready', last_error: '', managed: true, vcpus: 1, memory_mib: 512, ha_enabled: false })))
  })
  return { applied }
}

test('VM network policies: list, metrics and policy detail', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(PAGE)
  await expect(page.getByRole('heading', { name: 'VM Network Policies' })).toBeVisible({ timeout: 15_000 })
  const table = page.getByRole('table', { name: 'VM network policies' })
  await expect(table.getByText('db-from-web')).toBeVisible()
  await expect(table.getByText('db-1')).toBeVisible()
  await expect(page.getByText('Absent · native')).toBeVisible()
  await expect(page.getByText('Observe (audit)')).toBeVisible()
  await table.getByRole('button', { name: 'db-from-web' }).click()
  await expect(page.getByText(/from VMs app=web on 5432\/TCP/)).toBeVisible()
})

test('VM network policies: toServices template validates and applies', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  const m = await mockNetpol(page)
  await page.goto(`${PAGE}?tab=editor`)
  await expect(page.getByText(/toServices and authentication are enforced natively/)).toBeVisible({ timeout: 15_000 })
  await page.getByLabel('Template').selectOption('services')
  await expect(page.getByLabel('Policy YAML')).toHaveValue(/k8sService:/)
  await page.getByRole('button', { name: 'Validate & preview' }).click()
  await expect(page.getByText('Valid', { exact: true })).toBeVisible()
  await expect(page.getByText(/selects no service/)).toBeVisible()
  await expect(page.getByText(/service:shop\/shop-lb/)).toBeVisible()
  await page.getByRole('button', { name: 'Apply' }).click()
  await expect(page.getByText(/Applied web-to-services/)).toBeVisible()
  await expect(page.getByRole('table', { name: 'VM network policies' }).getByText('web-to-services')).toBeVisible()
  expect(m.applied).toEqual(['web-to-services'])
})

test('VM network policies: policy tester and endpoints', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?tab=tester`)
  await page.locator('#tr-from').fill('web-1')
  await page.locator('#tr-to').fill('db-1')
  await page.locator('#tr-port').fill('5432')
  await page.getByRole('button', { name: 'Trace' }).click()
  await expect(page.getByText('ALLOWED', { exact: true })).toBeVisible()
  await expect(page.getByText(/db-from-web spec\.ingress\[0\]/)).toBeVisible()
  await page.locator('#tr-port').fill('22')
  await page.getByRole('button', { name: 'Trace' }).click()
  await expect(page.getByText('DENIED: default deny at ingress of db-1')).toBeVisible()
  await expect(page.getByText('default-deny', { exact: true })).toBeVisible()

  await page.goto(`${PAGE}?tab=endpoints`)
  const ep = page.getByRole('table', { name: 'Policy endpoints' })
  await expect(ep.getByText('web-1')).toBeVisible({ timeout: 15_000 })
  await expect(ep.getByText('4102')).toBeVisible()
})

test('VM network policies: flow terminal streams a dropped flow', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?tab=flows`)
  const term = page.getByLabel('Packet flow terminal')
  await expect(term).toBeVisible({ timeout: 15_000 })
  await expect(term.getByText(/machinactl flow observe --follow/)).toBeVisible()
  await expect(term.getByText(/DROPPED/).first()).toBeVisible({ timeout: 10_000 })
  await expect(term.getByText(/web-1/).first()).toBeVisible()
})

test('VM network policies: service map with L7 metrics', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?tab=map`)
  const map = page.getByRole('img', { name: 'Service map' })
  await expect(map).toBeVisible({ timeout: 15_000 })
  await expect(map.getByText('host · 10.0.0.1')).toBeVisible()
  await page.getByRole('button', { name: /web-1 → db-1/ }).click()
  const l7 = page.getByRole('table', { name: 'L7 metrics' })
  await expect(l7.getByText('GET api/users/{id}')).toBeVisible()
  await expect(l7.getByText('20 ms')).toBeVisible()
  await expect(l7.getByText('8% 5xx')).toBeVisible()
  await page.getByLabel('Denied only').check()
  await expect(page.getByText(/1 links/)).toBeVisible()
})

test('VM network policies: learn, replay and open in editor', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?tab=learn`)
  await page.getByRole('button', { name: 'Generate' }).click()
  await expect(page.getByText('learned-db', { exact: true })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/1 observed connection\(s\) \(40 flows\) would be blocked/)).toBeVisible()
  await page.getByRole('button', { name: 'Open in editor' }).click()
  await expect(page.getByLabel('Policy YAML')).toHaveValue(/learned-db/)
  await page.getByRole('button', { name: 'Replay history' }).click()
  await expect(page.getByRole('table', { name: 'Would break' }).getByText('GET api/users/{id}')).toBeVisible()
})

test('VM network policies: draft from plain English, then apply', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  const m = await mockNetpol(page)
  await page.goto(`${PAGE}?tab=editor`)
  const box = page.getByLabel('Policy in plain English')
  await box.fill('make it nice')
  await page.getByRole('button', { name: 'Draft policy' }).click()
  await expect(page.getByText(/not understood/).first()).toBeVisible({ timeout: 15_000 })
  await box.fill('Only web servers can reach the db on port 5432')
  await page.getByRole('button', { name: 'Draft policy' }).click()
  await expect(page.getByLabel('Policy YAML')).toHaveValue(/nl-only-web-to-db/)
  await expect(page.getByText(/only VMs app=web can reach VMs app=db/)).toBeVisible()
  await expect(page.getByText(/Would break \d+ connection/)).toBeVisible()
  await expect(page.getByRole('button', { name: 'Request approval' })).toHaveCount(0)
  await page.getByRole('button', { name: 'Apply', exact: true }).click()
  await expect.poll(() => m.applied).toContain('nl-only-web-to-db')
})

test('VM network policies: alerts', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?tab=alerts`)
  const t = page.getByRole('table', { name: 'Flow alerts' })
  await expect(t.getByText('Port scan')).toBeVisible({ timeout: 15_000 })
  await expect(t.getByText(/probed 31 ports/)).toBeVisible()
})

test('VM network policies: quarantine from an alert, then release', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?tab=alerts`)
  const t = page.getByRole('table', { name: 'Flow alerts' })
  await t.getByRole('button', { name: 'Quarantine web-1' }).click({ timeout: 15_000 })
  await expect(page.getByLabel('VM', { exact: true })).toHaveValue('web-1')
  await page.getByLabel('For').selectOption('900')
  await page.getByLabel('Reason').fill('port scan')
  page.once('dialog', (d) => void d.accept())
  await page.getByRole('button', { name: 'Quarantine', exact: true }).click()
  await expect(page.getByText('web-1 quarantined for 15 minutes')).toBeVisible()
  const q = page.getByRole('table', { name: 'Quarantined VMs' })
  await expect(q.getByText('15m left')).toBeVisible()
  await expect(q.getByText('ingress host tcp/22')).toBeVisible()
  await expect(q.getByText('port scan')).toBeVisible()
  const ep = page.getByRole('table', { name: 'Policy endpoints' })
  await expect(ep.getByText('quarantined · 15m left')).toBeVisible()
  await expect(ep.getByRole('button', { name: 'Quarantine db-1' })).toBeVisible()
  page.once('dialog', (d) => void d.accept())
  await q.getByRole('button', { name: 'Release' }).click()
  await expect(page.getByText('Released web-1')).toBeVisible()
  await expect(page.getByText('No VM is quarantined.')).toBeVisible()
})

test('VM network policies: grant temporary access, then revoke it', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(PAGE)
  await expect(page.getByText('No temporary access.')).toBeVisible({ timeout: 15_000 })
  await page.getByLabel('From').fill('web-1')
  await page.getByLabel('To', { exact: true }).fill('db-1')
  await page.getByLabel('Port', { exact: true }).fill('5432')
  await page.getByLabel('For').selectOption('900')
  await page.getByLabel('Reason').fill('migration')
  await page.getByRole('button', { name: 'Grant access' }).click()
  await expect(page.getByText(/Granted web-1 → db-1:5432\/tcp/)).toBeVisible()
  const t = page.getByRole('table', { name: 'Temporary access' })
  await expect(t.getByText('web-1 → db-1:5432/tcp')).toBeVisible()
  await expect(t.getByText('15m left')).toBeVisible()
  await expect(t.getByText('migration')).toBeVisible()
  await expect(page.getByRole('table', { name: 'VM network policies' }).getByText('jit-web-1-to-db-1-5432-x')).toBeVisible()
  page.once('dialog', (d) => void d.accept())
  await t.getByRole('button', { name: 'Revoke' }).click()
  await expect(page.getByText('Revoked web-1 → db-1:5432/tcp')).toBeVisible()
  await expect(page.getByText('No temporary access.')).toBeVisible()
})

test('VM network policies: add a blocking threat feed, then remove it', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?tab=alerts`)
  await expect(page.getByText(/No threat feeds/)).toBeVisible({ timeout: 15_000 })
  await page.getByLabel('Name').fill('lab')
  await page.getByLabel('Source').selectOption('list')
  await page.getByLabel('Domains').fill('evil.example\nc2.example')
  await page.getByLabel('Block').check()
  await page.getByRole('button', { name: 'Save feed' }).click()
  await expect(page.getByText('Threat feed lab set — blocking')).toBeVisible()
  const t = page.getByRole('table', { name: 'Threat feeds' })
  await expect(t.getByText('inline list')).toBeVisible()
  await expect(t.getByText('Block', { exact: true })).toBeVisible()
  await expect(page.getByText('Watching DNS of 2 VMs.')).toBeVisible()
  const blocked = page.getByRole('table', { name: 'Blocked addresses' })
  await expect(blocked.getByText('203.0.113.7')).toBeVisible()
  await expect(blocked.getByText('evil.example')).toBeVisible()
  page.once('dialog', (d) => void d.accept())
  await t.getByRole('button', { name: 'Remove' }).click()
  await expect(page.getByText('Removed lab')).toBeVisible()
  await expect(page.getByText(/No threat feeds/)).toBeVisible()
})

test('VM network policies: delete asks for confirmation', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(PAGE)
  const table = page.getByRole('table', { name: 'VM network policies' })
  await expect(table.getByText('db-from-web')).toBeVisible({ timeout: 15_000 })
  page.once('dialog', (d) => void d.accept())
  await table.getByRole('button', { name: 'Delete' }).click()
  await expect(page.getByText('Deleted db-from-web')).toBeVisible()
  await expect(page.getByText(/No VM network policies/)).toBeVisible()
})

test('VM network policies: isolate a project, allow egress and set an egress IP', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?scope=fleet&tab=projects`)
  const t = page.getByRole('table', { name: 'Project networking' })
  await expect(t.getByText('shop')).toBeVisible({ timeout: 15_000 })
  await expect(t.getByText('lab', { exact: true })).toBeVisible()
  await page.getByLabel('Isolation of shop').selectOption('isolated')
  await expect(page.getByText('Project shop: isolated')).toBeVisible()
  await expect(t.locator('span', { hasText: /^Isolated$/ })).toBeVisible()
  await t.getByRole('button', { name: 'Manage…' }).first().click()
  await page.getByLabel('Destination').fill('*.stripe.com')
  await page.getByLabel('Ports').fill('443')
  await page.getByRole('button', { name: 'Allow', exact: true }).click()
  await expect(page.getByText('Project shop: allowed *.stripe.com')).toBeVisible()
  await expect(page.getByLabel('Limit egress of shop')).toBeChecked()
  await expect(t.getByText('*.stripe.com (443)')).toBeVisible()
  await page.getByLabel('Egress IP host').fill('hv1')
  await page.getByLabel('Egress IP', { exact: true }).fill('198.51.100.7')
  await page.getByRole('button', { name: 'Set egress IP' }).click()
  await expect(page.getByText('Project shop: egress IP set')).toBeVisible()
  await expect(page.getByRole('table', { name: 'Egress IPs on hosts' }).getByText(/shop: 1 VM address\(es\) → 198\.51\.100\.7/)).toBeVisible()
  await page.getByLabel('Default isolation').selectOption('isolated')
  await expect(page.getByText('Projects are isolated by default')).toBeVisible()
  page.once('dialog', (d) => void d.accept())
  await t.getByRole('button', { name: 'Reset' }).click()
  await expect(page.getByText('Project shop follows the default')).toBeVisible()
})

test('VM network policies: export segmentation evidence', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(PAGE)
  await expect(page.getByRole('heading', { name: 'VM Network Policies' })).toBeVisible({ timeout: 15_000 })
  const [json] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export evidence as JSON' }).click()])
  expect(json.suggestedFilename()).toMatch(/^segmentation-evidence-\d{8}T\d{6}\.json$/)
  const fs = await import('node:fs/promises')
  expect(await fs.readFile((await json.path())!, 'utf8')).toBe('{"kind":"machina.io/segmentation-evidence/v1","scope":"fleet","digest":"abc123"}')
  await expect(page.getByText('Segmentation evidence downloaded')).toBeVisible()
  const [md] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export evidence as Markdown' }).click()])
  expect(md.suggestedFilename()).toMatch(/\.md$/)
})

test('VM network policies: assign a VM, preview, propose and a per-project matrix', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await mockNetpol(page)
  await page.goto(`${PAGE}?scope=fleet&tab=projects`)
  const t = page.getByRole('table', { name: 'Project networking' })
  await expect(t.getByText('shop')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByLabel('Project warnings').getByText(/per-host NAT/)).toBeVisible()
  await expect(t.getByText('Cross-host NAT')).toBeVisible()

  await page.getByLabel('VM', { exact: true }).selectOption('spare-1')
  await page.getByLabel('Project', { exact: true }).fill('shop')
  await page.getByRole('button', { name: 'Assign' }).click()
  await expect(page.getByText('spare-1 is in project shop')).toBeVisible()
  await t.getByRole('button', { name: 'Manage…' }).first().click()
  await expect(page.getByLabel('VMs of shop').getByText('spare-1')).toBeVisible()
  await page.getByRole('button', { name: 'Remove spare-1 from shop' }).click()
  await expect(page.getByText('spare-1 left its project')).toBeVisible()

  const m = page.getByRole('table', { name: 'Matrix of shop' })
  await expect(m.getByText('(other projects)')).toBeVisible()
  await expect(m.getByText('Segmented')).toBeVisible()
  const [dl] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export evidence of shop as JSON' }).click()])
  expect(dl.suggestedFilename()).toMatch(/^segmentation-evidence-shop-\d{8}T\d{6}\.json$/)

  await page.getByLabel('Egress IP host').fill('hv1')
  await page.getByLabel('Egress IP', { exact: true }).fill('198.51.100.7, 2001:db8::7')
  await page.getByRole('button', { name: 'Set egress IP' }).click()
  await expect(page.getByText('Project shop: egress IP set')).toBeVisible()
  await expect(t.getByText('1 VM without it')).toBeVisible()
  await page.getByLabel('Require the egress IP of shop').click()
  await expect(t.getByText('1 VM blocked')).toBeVisible()

  await page.getByLabel('Preview changes').check()
  let shown = ''
  page.once('dialog', (d) => { shown = d.message(); void d.dismiss() })
  await page.getByLabel('Isolation of shop').selectOption('isolated')
  await expect.poll(() => shown).toContain('would break 2 recorded connections')
  await page.getByLabel('Preview changes').uncheck()
  await page.getByLabel('Ask a second admin').check()
  await page.getByLabel('Isolation of shop').selectOption('isolated')
  await expect(page.getByText(/Sent for approval: would break 2/)).toBeVisible()
})

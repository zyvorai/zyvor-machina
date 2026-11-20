// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect as baseExpect, type Page, type Route } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const expect = baseExpect.configure({ timeout: 15_000 })

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

const template = {
  instances: [
    { name: 'web', count: 2, cpu_cores: 2, memory: '2Gi', anti_affinity: true },
    { name: 'db', count: 1, cpu_cores: 4, memory: '8Gi', backup: { interval_hours: 24, retain: 7 } },
  ],
  policies: [
    { from: 'internet', to: 'web', ports: [443], protocol: 'tcp' },
    { from: 'web', to: 'db', ports: [5432], protocol: 'tcp' },
  ],
}

const plan = (over: Record<string, unknown> = {}) => ({
  name: 'shop',
  project: 'dev',
  errors: [],
  vms: [
    { name: 'shop-web-1', group: 'web', vcpus: 2, memory_mib: 2048, disk_gib: 10, image: null, host: 'h1', monthly_usd: 36.5, action: 'create' },
    { name: 'shop-web-2', group: 'web', vcpus: 2, memory_mib: 2048, disk_gib: 10, image: null, host: 'h2', monthly_usd: 36.5, action: 'create' },
    { name: 'shop-db', group: 'db', vcpus: 4, memory_mib: 8192, disk_gib: 10, image: null, host: 'h1', monthly_usd: 87.6, action: 'create' },
  ],
  totals: { vms: 3, vcpus: 8, memory_mib: 12288, storage_gib: 30 },
  monthly_usd: 160.6,
  quota: { ok: true, detail: "Fits project 'dev' quota." },
  placement: { ok: true, detail: 'Placement: 2 on h1, 1 on h2.' },
  policies: template.policies,
  policy_yaml: 'kind: VmNetworkPolicy\nmetadata:\n  name: stack-shop-db\n',
  replay: null,
  replay_summary: 'would break 0 recorded connections (0 flows) and newly allow 0',
  diff: null,
  ...over,
})

const stack = {
  id: 'st-1',
  project_id: 'p1',
  name: 'shop',
  status: 'created',
  last_error: null,
  template_json: template,
  resources_json: [
    { kind: 'instance', id: 'v1', name: 'shop-web-1' },
    { kind: 'netpol', id: '00000000-0000-0000-0000-000000000000', name: 'stack-shop-db' },
  ],
  drift_json: { in_sync: false, open: 1, items: [{ kind: 'instance', name: 'shop-web-2', detail: 'missing', fixed: false }] },
  checked_at: '2026-10-05T09:00:00Z',
  auto_heal: false,
  updated_at: '2026-10-05T08:00:00Z',
}

async function mockStacks(page: Page) {
  const calls: { path: string; method: string; body: unknown }[] = []
  const record = (route: Route) => {
    const req = route.request()
    const path = new URL(req.url()).pathname.replace(/^.*\/api\/v1/, '')
    const body = req.postData() ? req.postDataJSON() : null
    if (req.method() !== 'GET') calls.push({ path, method: req.method(), body })
    return { req, path, body }
  }
  await page.route(/\/api\/v1\/stacks(\/.*)?$/, (route) => {
    const { req, path } = record(route)
    if (path === '/stacks' && req.method() === 'GET') {
      return json(route, [stack, { ...stack, id: 'st-0', name: 'legacy', template_json: { vms: [{ name: 'a' }] }, drift_json: {}, checked_at: null }])
    }
    if (path === '/stacks/draft') return json(route, { template, source: 'rules', notes: [], plan: plan() })
    if (path === '/stacks/plan') {
      const b = route.request().postDataJSON() as { stack_id?: string }
      return json(route, b.stack_id
        ? plan({ vms: [...plan().vms.slice(0, 2).map((v) => ({ ...v, action: 'keep' })), { ...plan().vms[2], action: 'create', name: 'shop-web-3' }], diff: { create: ['shop-web-3'], delete: [], resize: [], relabel: [], policies_upsert: [], policies_delete: [] } })
        : plan({ quota: { ok: false, detail: "Project 'dev' VM count quota exceeded (2)" } }))
    }
    if (path === '/stacks/propose') {
      return json(route, { id: 'act-1', action_type: 'stack.deploy', label: 'Deploy stack shop', review: '3 new VMs, $160.60/month in total, 2 policies.', risk: 'Review required', object_ref: {}, status: 'pending', requested_by: 'sus', created_at: '2026-10-05T09:00:00Z', source: 'stacks' })
    }
    if (path === '/stacks/st-1/drift') return json(route, stack.drift_json)
    if (path === '/stacks/st-1/converge') return json(route, { in_sync: true, open: 0, items: [{ kind: 'instance', name: 'shop-web-2', detail: 'missing; recreated', fixed: true }] })
    if (path === '/stacks/st-1/auto-heal') return json(route, { ...stack, auto_heal: true })
    if (path === '/stacks/st-1') return json(route, stack)
    return json(route, {}, 404)
  })
  await page.route(/\/api\/v1\/ai\/actions\/act-1\/execute$/, (route) => {
    record(route)
    return json(route, { message: 'Deploying stack shop', stack_id: 'st-2' })
  })
  return calls
}

test('Fleet Cloud stacks: describe, plan, propose and approve', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  const calls = await mockStacks(page)
  await page.goto('/fleet-cloud/heat')

  const rows = page.getByRole('table', { name: 'Stacks' })
  await expect(rows.getByRole('row').filter({ hasText: 'shop' }).getByLabel('Drift')).toHaveText('1 drifted')
  await expect(rows.getByRole('row').filter({ hasText: 'legacy' })).toContainText('—')

  const composer = page.getByRole('region', { name: 'Compose stack' })
  await composer.getByLabel('New stack name').fill('shop')
  await composer.getByLabel('Describe the stack').fill('2 web servers and a postgres database with daily backups')
  await composer.getByRole('button', { name: 'Draft plan' }).click()
  await expect.poll(() => calls.find((c) => c.path === '/stacks/draft')?.body).toEqual({
    name: 'shop',
    prompt: '2 web servers and a postgres database with daily backups',
  })

  const preview = composer.getByRole('region', { name: 'Stack plan' })
  await expect(preview.getByLabel('Monthly cost')).toHaveText('$160.60/month')
  await expect(preview.getByRole('table', { name: 'Planned VMs' }).getByRole('row')).toHaveCount(4)
  await expect(preview.getByLabel('Placement')).toContainText('2 on h1, 1 on h2')
  await expect(preview.getByLabel('Replay')).toContainText('would break 0')
  await expect(preview.getByRole('img', { name: 'Policy graph' })).toContainText('internet')
  await expect(preview.getByRole('img', { name: 'Policy graph' })).toContainText('5432')

  await composer.getByText('Edit template').click()
  await composer.getByRole('button', { name: 'Plan again' }).click()
  await expect(preview.getByLabel('Quota')).toContainText('quota exceeded')
  await expect(composer.getByRole('button', { name: 'Propose for approval' })).toBeDisabled()
  await composer.getByLabel('Describe the stack').fill('2 web servers')
  await composer.getByRole('button', { name: 'Draft plan' }).click()
  await expect(composer.getByRole('button', { name: 'Propose for approval' })).toBeEnabled()

  await composer.getByRole('button', { name: 'Propose for approval' }).click()
  await expect.poll(() => (calls.find((c) => c.path === '/stacks/propose')?.body as { name?: string })?.name).toBe('shop')
  await expect(composer.getByRole('status', { name: 'Approval' })).toContainText('3 new VMs')
  await composer.getByRole('button', { name: 'Approve and deploy' }).click()
  await expect.poll(() => calls.map((c) => c.path)).toContain('/ai/actions/act-1/execute')
})

test('Fleet Cloud stack detail: drift, converge, auto-heal and template update', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  const calls = await mockStacks(page)
  await page.goto('/fleet-cloud/heat/shop/st-1')

  await page.getByRole('button', { name: 'Drift', exact: true }).click()
  const panel = page.getByRole('region', { name: 'Stack drift' })
  await expect(panel.getByRole('table', { name: 'Drift' })).toContainText('missing')
  await panel.getByRole('button', { name: 'Converge now' }).click()
  await expect(panel.getByRole('table', { name: 'Drift' })).toContainText('missing; recreated')
  await panel.getByLabel('Auto-heal').check()
  await expect.poll(() => calls.find((c) => c.path === '/stacks/st-1/auto-heal')?.body).toEqual({ enabled: true })

  await page.getByRole('button', { name: 'Template', exact: true }).click()
  const editor = page.getByRole('region', { name: 'Stack template' })
  const text = JSON.stringify({ ...template, instances: [{ ...template.instances[0], count: 3 }, template.instances[1]] }, null, 2)
  await editor.getByLabel('Template (JSON)').fill(text)
  await editor.getByRole('button', { name: 'Plan update' }).click()
  await expect.poll(() => (calls.find((c) => c.path === '/stacks/plan')?.body as { stack_id?: string })?.stack_id).toBe('st-1')
  const table = editor.getByRole('table', { name: 'Planned VMs' })
  await expect(table.getByRole('row').filter({ hasText: 'shop-web-3' })).toContainText('create')
  await expect(table.getByRole('row').filter({ hasText: 'shop-web-1' })).toContainText('keep')
  await editor.getByRole('button', { name: 'Apply now' }).click()
  await expect.poll(() => calls.find((c) => c.method === 'PUT' && c.path === '/stacks/st-1')?.body).toEqual({
    template: JSON.parse(text),
  })
})

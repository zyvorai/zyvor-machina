// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect as baseExpect, type Page, type Route } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const expect = baseExpect.configure({ timeout: 15_000 })

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

type Call = { path: string; method: string; body: unknown }

const spec = {
  targets: ['vm-1'],
  steps: [{ kind: 'latency', delay_ms: 200, jitter_ms: 20, secs: 60 }, { kind: 'kill', recover_secs: 180 }],
  probes: [{ kind: 'tcp', target: '10.0.0.5:80' }],
  abort: { min_success_pct: 50, window_secs: 20 },
  baseline_secs: 15,
  recovery_secs: 15,
}
const experiment = {
  id: 'e1', name: 'web-latency', description: '', spec, created_by: 'sus', created_at: '', updated_at: '', last_status: null, last_run_at: null,
}
const phases = [
  { name: 'Baseline', kind: 'baseline', started_s: 0, ended_s: 15, samples: 8, ok: 8, success_pct: 100, p50_ms: 1, p95_ms: 2 },
  { name: 'Latency 200±20 ms for 60s', kind: 'latency', started_s: 15, ended_s: 75, samples: 30, ok: 30, success_pct: 100, p50_ms: 201, p95_ms: 230 },
  { name: 'Kill', kind: 'kill', started_s: 75, ended_s: 140, samples: 30, ok: 4, success_pct: 13.3, p50_ms: null, p95_ms: null, recovered_s: 62 },
]

async function mockChaos(page: Page, calls: Call[], opts: { status?: 'running' | 'failed' } = {}) {
  await page.route('**/api/v1/vms', (r) => r.request().method() === 'GET'
    ? json(r, [{ id: 'vm-1', name: 'web-1', host_id: 'h1', desired_state: 'running', observed_state: 'running' }, { id: 'vm-2', name: 'db-1', host_id: 'h1', desired_state: 'running', observed_state: 'running' }])
    : r.fallback())
  await page.route('**/api/v1/hosts', (r) => r.request().method() === 'GET' ? json(r, [{ id: 'h1', hostname: 'kvm-a', state: 'online' }]) : r.fallback())
  await page.route(/\/api\/v1\/chaos\/.*/, (route) => {
    const req = route.request()
    const path = new URL(req.url()).pathname.replace(/^.*\/api\/v1/, '')
    if (req.method() !== 'GET') calls.push({ path, method: req.method(), body: req.postData() ? req.postDataJSON() : null })
    if (path === '/chaos/experiments' && req.method() === 'POST') return json(route, { ...experiment, ...req.postDataJSON() })
    if (path === '/chaos/experiments') return json(route, [{ ...experiment, last_status: opts.status ?? null }])
    if (path === '/chaos/experiments/e1/run') return json(route, { run_id: 'r1', status: 'running' })
    if (path === '/chaos/experiments/e1') return json(route, { experiment, runs: [{ id: 'r1' }], max_secs: 300 })
    if (path === '/chaos/faults') return json(route, { items: opts.status === 'running'
      ? [{ id: 'chaos:r1:0:vm1', vm: 'web-1', host: 'kvm-a', taps: ['vnet3'], delay_ms: 200, jitter_ms: 20, loss_pct: 0, partition: [], remaining_secs: 74 }]
      : [] })
    if (path === '/chaos/runs/r1/abort') return json(route, { aborting: true })
    if (path === '/chaos/runs/r1') {
      const running = opts.status === 'running'
      return json(route, {
        run: {
          id: 'r1', experiment_id: 'e1', status: running ? 'running' : 'failed', started_by: 'sus', started_at: '', finished_at: null,
          abort_reason: running ? '' : 'probe success 13% over the last 20 s is below 50%',
          report: running ? {} : {
            experiment: 'web-latency', targets: ['web-1'], probes: ['tcp 10.0.0.5:80'], phases, verdict: 'Aborted at step 2', duration_s: 140,
            findings: ['tcp 10.0.0.5:80 failed while web-1 was down: nothing else serves it'],
          },
        },
        live: running ? { phase: 1, phases: phases.slice(0, 2), samples: [] } : null,
      })
    }
    return json(route, { error: 'unexpected' }, 404)
  })
}

test.describe('Fleet Cloud game days', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page)
  })

  test('builds an experiment with steps and a probe', async ({ page }) => {
    const calls: Call[] = []
    await mockChaos(page, calls)
    await page.goto('/fleet-cloud/chaos')
    await page.getByRole('button', { name: 'New experiment' }).click()
    await page.getByLabel('Experiment name').fill('db-partition')
    await page.getByRole('checkbox', { name: 'db-1' }).check()
    await page.getByLabel('Step kind').selectOption('partition')
    await page.getByRole('button', { name: 'Add step' }).click()
    await page.getByLabel('Blocked networks').fill('10.0.0.0/24')
    await page.getByRole('listitem', { name: 'Step 1' }).getByLabel('Seconds', { exact: true }).fill('30')
    await page.getByLabel('Step kind').selectOption('disk')
    await page.getByRole('button', { name: 'Add step' }).click()
    await page.getByLabel('Probe kind').selectOption('http')
    await page.getByRole('button', { name: 'Add probe' }).click()
    await page.getByLabel('Probe URL').fill('http://10.0.0.6/health')
    await page.getByRole('button', { name: 'Save experiment' }).click()
    await expect.poll(() => calls[0]?.body).toEqual({
      name: 'db-partition',
      description: '',
      spec: {
        targets: ['vm-2'],
        steps: [
          { kind: 'partition', cidrs: ['10.0.0.0/24'], peers: [], secs: 30 },
          { kind: 'disk', read_iops: 50, write_iops: 50, secs: 60 },
        ],
        probes: [{ kind: 'http', url: 'http://10.0.0.6/health', expect_status: 200 }],
        abort: { min_success_pct: 50, window_secs: 20 },
        baseline_secs: 15,
        recovery_secs: 15,
      },
    })
  })

  test('runs only after typing the name, and reports the result', async ({ page }) => {
    const calls: Call[] = []
    await mockChaos(page, calls)
    await page.goto('/fleet-cloud/chaos')
    await expect(page.getByText('Latency 200 ms ± 20 ms for 60 s → Kill the VMs, wait up to 180 s for them to come back')).toBeVisible()
    await page.getByRole('button', { name: 'Run web-latency' }).click()
    const start = page.getByRole('button', { name: 'Start game day' })
    await expect(start).toBeDisabled()
    await page.getByRole('dialog').getByRole('textbox').fill('web-latency')
    await start.click()
    await expect.poll(() => calls.map((c) => c.path)).toEqual(['/chaos/experiments/e1/run'])
    expect(calls[0].body).toEqual({ confirm: 'web-latency' })
    await expect(page.getByRole('heading', { name: 'Run: Failed' })).toBeVisible()
    await expect(page.getByText('probe success 13% over the last 20 s is below 50%')).toBeVisible()
    await expect(page.getByText('back after 62 s')).toBeVisible()
    await expect(page.getByText('nothing else serves it')).toBeVisible()
  })

  test('shows live faults and aborts a running game day', async ({ page }) => {
    const calls: Call[] = []
    await mockChaos(page, calls, { status: 'running' })
    await page.goto('/fleet-cloud/chaos')
    await expect(page.getByText('web-1 on kvm-a: 200 ms delay · ends in 74 s')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Run web-latency' })).toBeDisabled()
    await page.getByRole('button', { name: 'Show last run of web-latency' }).click()
    await expect(page.getByRole('heading', { name: 'Run: Running' })).toBeVisible()
    await page.getByRole('button', { name: 'Abort and lift faults' }).click()
    await expect.poll(() => calls.map((c) => c.path)).toEqual(['/chaos/runs/r1/abort'])
  })
})

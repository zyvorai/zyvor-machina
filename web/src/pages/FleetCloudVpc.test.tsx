// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import type { ReactNode } from 'react'
import FleetCloudVpc from './FleetCloudVpc'

const api = vi.hoisted(() => ({ listVpcs: vi.fn(), listSubnets: vi.fn(), listInstanceGroups: vi.fn(), listLaunchTemplates: vi.fn(), retrySubnet: vi.fn(), getCloudPlan: vi.fn(), createLaunchTemplate: vi.fn() }))
const toastError = vi.hoisted(() => vi.fn())
vi.mock('../api/cloud', () => api)
vi.mock('../api/nativeProjects', () => ({ listProjectRegistry: async () => [{ id: 'a', name: 'Project A', enabled: true }, { id: 'b', name: 'Project B', enabled: true }] }))
vi.mock('../api/platform', () => ({ platformFetch: async () => [{ id: 'h', hostname: 'host', state: 'online' }] }))
vi.mock('../components/PageLayout', () => ({ default: ({ children }: { children: ReactNode }) => <main>{children}</main> }))
vi.mock('../components/FleetCloudSubNav', () => ({ default: () => null }))
vi.mock('../components/FleetCloudFooter', () => ({ default: () => null }))
vi.mock('../contexts/ToastContext', () => ({ useToastContext: () => ({ error: toastError }) }))

afterEach(cleanup)
beforeEach(() => {
  vi.resetAllMocks()
  api.listVpcs.mockResolvedValue([{ id: 'va', name: 'VPC A', cidr: '10.20.0.0/16' }])
  api.listSubnets.mockResolvedValue([{ id: 's', name: 'apps', cidr: '10.20.1.0/24', status: 'error', last_error: 'Agent unavailable' }])
  api.listInstanceGroups.mockResolvedValue([]); api.listLaunchTemplates.mockResolvedValue([])
  api.retrySubnet.mockResolvedValue({ task_id: 'task' })
  api.getCloudPlan.mockResolvedValue({ forwarding_active: false, warnings: ['Peering is a plan only'], routes: [], peerings: [] })
})
it('shows failed provisioning, retries the subnet and keeps forwarding status explicit', async () => {
  render(<FleetCloudVpc />)
  const retry = await screen.findByRole('button', { name: 'Retry apps' })
  expect(screen.getByRole('alert').textContent).toContain('Agent unavailable')
  fireEvent.click(retry)
  await waitFor(() => expect(api.retrySubnet).toHaveBeenCalledWith('s'))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Inspect plan' }).hasAttribute('disabled')).toBe(false))
  fireEvent.click(screen.getByRole('button', { name: 'Inspect plan' }))
  expect(await screen.findByText('Forwarding active: no')).toBeTruthy()
  expect(screen.getByText('Peering is a plan only')).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Create group' }).hasAttribute('disabled')).toBe(true)
})
it('does not show a previous project response after switching projects', async () => {
  let resolveA!: (value: unknown[]) => void
  const pending = new Promise<unknown[]>(resolve => { resolveA = resolve })
  api.listVpcs.mockImplementation((project: string) => project === 'a' ? pending : Promise.resolve([{ id: 'vb', name: 'VPC B', cidr: '10.21.0.0/16' }]))
  render(<FleetCloudVpc />)
  await waitFor(() => expect(api.listVpcs).toHaveBeenCalledWith('a'))
  fireEvent.change(screen.getByLabelText('Project'), { target: { value: 'b' } })
  await screen.findByRole('option', { name: 'VPC B · 10.21.0.0/16' })
  resolveA([{ id: 'va', name: 'VPC A', cidr: '10.20.0.0/16' }])
  await waitFor(() => expect(api.listVpcs).toHaveBeenCalledWith('b'))
  expect(screen.queryByRole('option', { name: 'VPC A · 10.20.0.0/16' })).toBeNull()
})
it('a group whose stored policy is malformed shows a notice instead of crashing the page', async () => {
  api.listInstanceGroups.mockResolvedValue([
    { id: 'g1', name: 'broken', policy_json: 'not json', paused: false, last_error: '' },
    { id: 'g2', name: 'web', policy_json: JSON.stringify({ min: 0, max: 4, desired: 2, target_cpu: null, cooldown_secs: 300 }), paused: false, last_error: '' },
  ])
  render(<FleetCloudVpc />)
  expect(await screen.findByText(/broken · its scaling policy could not be read/)).toBeTruthy()
  // the healthy group next to it still renders with its controls
  expect(screen.getByText('web · 2 desired · active')).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Add instance' })).toBeTruthy()
})
it('polls subnets only while one is pending', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true })
  try {
    api.listSubnets.mockResolvedValue([{ id: 's', name: 'apps', cidr: '10.20.1.0/24', status: 'ready', last_error: '' }])
    render(<FleetCloudVpc />)
    await screen.findByText(/apps · 10.20.1.0\/24/)
    const afterLoad = api.listSubnets.mock.calls.length
    await vi.advanceTimersByTimeAsync(20000)
    expect(api.listSubnets.mock.calls.length).toBe(afterLoad) // nothing pending: no polling
    cleanup()
    api.listSubnets.mockReset()
    api.listSubnets.mockResolvedValue([{ id: 's', name: 'apps', cidr: '10.20.1.0/24', status: 'pending', last_error: '' }])
    render(<FleetCloudVpc />)
    await screen.findByText('pending')
    const pendingLoad = api.listSubnets.mock.calls.length
    await vi.advanceTimersByTimeAsync(11000)
    expect(api.listSubnets.mock.calls.length).toBeGreaterThan(pendingLoad) // pending: keeps polling
  } finally {
    vi.useRealTimers()
  }
})
it('reports invalid template JSON without calling the API', async () => {
  render(<FleetCloudVpc />)
  await screen.findByRole('button', { name: 'Retry apps' })
  fireEvent.change(screen.getByLabelText('Template name'), { target: { value: 'tpl' } })
  fireEvent.change(screen.getByLabelText('VM spec JSON'), { target: { value: '{ not json' } })
  fireEvent.click(screen.getByRole('button', { name: 'Save template' }))
  await waitFor(() => expect(toastError).toHaveBeenCalled())
  expect(String(toastError.mock.calls[0][0])).toContain('not valid JSON')
  expect(api.createLaunchTemplate).not.toHaveBeenCalled()
})
it('shows empty states for a project with nothing in it', async () => {
  api.listVpcs.mockResolvedValue([])
  api.listSubnets.mockResolvedValue([])
  render(<FleetCloudVpc />)
  expect(await screen.findByText(/No VPCs in this project yet/)).toBeTruthy()
  expect(screen.getByText('No templates yet.')).toBeTruthy()
  expect(screen.getByText('No instance groups yet.')).toBeTruthy()
})

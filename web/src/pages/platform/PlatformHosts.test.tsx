// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import PlatformHosts from './PlatformHosts'
import type { PlatformHost } from '../../api/platform'

vi.mock('../../api/platform', () => ({
  listPlatformHosts: vi.fn(),
  getFleetLinuxHealth: vi.fn(async () => ({ hosts: [] })),
  getPlatformHostDetail: vi.fn(async () => { throw new Error('no detail') }),
  enqueueValidateHost: vi.fn(async () => ({ task_id: 't' })),
  syncAllHosts: vi.fn(async () => []),
  syncHost: vi.fn(), hostMaintenance: vi.fn(), fenceHost: vi.fn(), deleteHost: vi.fn(),
}))
vi.mock('../../contexts/ToastContext', () => ({ useToastContext: () => ({ success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() }) }))
vi.mock('../../components/platform/HostEnrollWizard', () => ({ default: ({ open }: { open: boolean }) => (open ? <div data-testid="wizard-open" /> : null) }))
import * as api from '../../api/platform'

const host = (o: Partial<PlatformHost> = {}): PlatformHost => ({
  id: 'h1', hostname: 'NLDW4-4-04-32', address: '175.110.122.71', state: 'online', maintenance_mode: false, agent_grpc_addr: '127.0.0.1:50051',
  vm_count: 1, cpu_percent: 55, memory_used_mib: 700, memory_total_mib: 1000, fenced: false, schedulable: true,
  validation_status: 'passed', last_heartbeat_at: new Date().toISOString(), site: '', rack: '', ...o,
})
const view = () => render(<MemoryRouter><PlatformHosts /></MemoryRouter>)

beforeEach(() => { try { localStorage.clear() } catch { /* ignore */ } })
afterEach(() => { cleanup(); vi.clearAllMocks() })

describe('PlatformHosts', () => {
  it('one host: cards by default, the fleet strip, and the "second machine" card', async () => {
    vi.mocked(api.listPlatformHosts).mockResolvedValue([host()])
    view()
    await waitFor(() => expect(screen.getByTestId('fleet-strip')).toBeTruthy())
    expect(screen.getByTestId('fleet-online').textContent).toContain('1 / 1')
    expect(screen.getByTestId('host-fleet-panels')).toBeTruthy()
    expect(screen.getByTestId('first-run-card').textContent).toContain('Add your second machine')
    expect(screen.queryByTestId('attention-rail')).toBeNull()
    fireEvent.click(screen.getAllByTestId('add-machine')[0])
    expect(screen.getByTestId('wizard-open')).toBeTruthy()
  })
  it('two or more hosts open on the cloud map; the choice is remembered', async () => {
    vi.mocked(api.listPlatformHosts).mockResolvedValue([host(), host({ id: 'h2', hostname: 'zz-node1', agent_grpc_addr: '10.0.0.2:50051' })])
    view()
    await waitFor(() => expect(screen.getByTestId('hosts-cloud-map')).toBeTruthy())
    expect(screen.queryByTestId('first-run-card')).toBeNull()
    fireEvent.click(screen.getByRole('tab', { name: /List/ }))
    expect(screen.getByTestId('hosts-list')).toBeTruthy()
    expect(localStorage.getItem('machina.hosts.display')).toBe('list')
  })
  it('shows what needs attention and Re-check queues validation', async () => {
    vi.mocked(api.listPlatformHosts).mockResolvedValue([host(), host({ id: 'h2', hostname: 'zz-node1', validation_status: 'failed', agent_grpc_addr: '10.0.0.2:50051' })])
    view()
    await waitFor(() => expect(screen.getByTestId('attention-rail')).toBeTruthy())
    expect(screen.getByTestId('fleet-attention').textContent).toContain('1')
    fireEvent.click(screen.getByTestId('recheck-zz-node1'))
    await waitFor(() => expect(api.enqueueValidateHost).toHaveBeenCalledWith('h2'))
  })
  it('selecting a host opens the drawer', async () => {
    vi.mocked(api.listPlatformHosts).mockResolvedValue([host()])
    view()
    await waitFor(() => expect(screen.getByTestId('host-fleet-card-h1')).toBeTruthy())
    fireEvent.click(screen.getByTestId('host-fleet-card-h1'))
    expect(screen.getByTestId('host-drawer')).toBeTruthy()
  })
  it('no hosts: an empty state with Add machine', async () => {
    vi.mocked(api.listPlatformHosts).mockResolvedValue([])
    view()
    await waitFor(() => expect(screen.getByText('No hosts enrolled')).toBeTruthy())
    expect(screen.getAllByTestId('add-machine').length).toBeGreaterThan(0)
  })
})

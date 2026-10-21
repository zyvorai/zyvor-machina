// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import PlatformFailureDomains from './PlatformFailureDomains'
import PlatformDesktopPools from './PlatformDesktopPools'
import { parseWaveItems } from './PlatformMigrationWaves'

const toast = { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() }

vi.mock('../../contexts/ToastContext', () => ({ useToastContext: () => toast }))
vi.mock('../../api/platform', () => ({
  listPlatformVms: vi.fn(),
  listPlatformHosts: vi.fn(),
  listBackupTargets: vi.fn(),
}))
vi.mock('../../api/platformFleetFeatures', () => ({
  getFailureDomains: vi.fn(),
  getWitnessStatus: vi.fn(),
  listDesktopPools: vi.fn(),
  getDesktopPool: vi.fn(),
  createDesktopPool: vi.fn(),
  scaleDesktopPool: vi.fn(),
  reconcileDesktopPool: vi.fn(),
  checkoutDesktop: vi.fn(),
  releaseDesktop: vi.fn(),
  deleteDesktopPool: vi.fn(),
}))
import * as platform from '../../api/platform'
import * as api from '../../api/platformFleetFeatures'

beforeEach(() => {
  vi.clearAllMocks()
})
afterEach(() => cleanup())

describe('parseWaveItems', () => {
  it('accepts a list of named items with object configs', () => {
    const r = parseWaveItems('[{"name":" web-01 ","config":{"command":"vsphere"}}]')
    expect(r).toEqual({ items: [{ name: 'web-01', config: { command: 'vsphere' } }] })
  })

  it('refuses bad JSON, empty lists and malformed items with a reason', () => {
    expect(parseWaveItems('not json')).toEqual({ error: 'Items must be valid JSON.' })
    expect(parseWaveItems('[]')).toEqual({ error: 'Items must be a non-empty JSON array.' })
    expect(parseWaveItems('{"name":"x"}')).toEqual({ error: 'Items must be a non-empty JSON array.' })
    expect(parseWaveItems('[{"config":{}}]')).toEqual({ error: 'Every item needs a name.' })
    expect(parseWaveItems('[{"name":"a","config":[1]}]')).toEqual({ error: 'Item "a" needs a config object.' })
    expect(parseWaveItems('[{"name":"a"}]')).toEqual({ error: 'Item "a" needs a config object.' })
  })
})

describe('PlatformFailureDomains', () => {
  it('shows each rack and says whether rack loss is covered', async () => {
    vi.mocked(api.getFailureDomains).mockResolvedValue({
      rack_loss_tolerant: false,
      unassigned_hosts: 1,
      findings: ['Losing dc1/r2 leaves 262144 MiB of HA memory without room (short by 65536 MiB).'],
      domains: [
        {
          domain: 'dc1/r1', hosts: 2, hosts_online: 2, vms: 4, ha_vms: 2, memory_total_mib: 1, memory_used_mib: 1,
          ha_memory_to_recover_mib: 1024, headroom_outside_mib: 4096, hosts_outside: 2, spare_after_loss_mib: 3072, survivable: true,
        },
        {
          domain: 'dc1/r2', hosts: 2, hosts_online: 2, vms: 9, ha_vms: 8, memory_total_mib: 1, memory_used_mib: 1,
          ha_memory_to_recover_mib: 262144, headroom_outside_mib: 196608, hosts_outside: 2, spare_after_loss_mib: -65536, survivable: false,
        },
      ],
    })
    vi.mocked(api.getWitnessStatus).mockResolvedValue({
      configured: true, mode: 'any', witnesses: ['witness.example:8443'], reachable: 0, allowed: false,
    })
    render(<MemoryRouter><PlatformFailureDomains /></MemoryRouter>)
    await waitFor(() => expect(screen.getByText('Rack loss is not covered')).toBeTruthy())
    expect(screen.getByText('dc1/r1')).toBeTruthy()
    expect(screen.getByText('dc1/r2')).toBeTruthy()
    expect(screen.getByText(/short by 64\.0 GiB/)).toBeTruthy()
    expect(screen.getByText(/Losing dc1\/r2/)).toBeTruthy()
    expect(screen.getByText(/HA is paused: 0 of 1 witness/)).toBeTruthy()
  })

  it('explains how to define racks when there are none', async () => {
    vi.mocked(api.getFailureDomains).mockResolvedValue({ rack_loss_tolerant: false, unassigned_hosts: 3, findings: [], domains: [] })
    vi.mocked(api.getWitnessStatus).mockResolvedValue({ configured: false, mode: 'any', witnesses: [], reachable: 0, allowed: true })
    render(<MemoryRouter><PlatformFailureDomains /></MemoryRouter>)
    await waitFor(() => expect(screen.getByText('No racks labelled')).toBeTruthy())
  })
})

describe('PlatformDesktopPools', () => {
  it('lists pools and offers only stopped VMs as templates', async () => {
    vi.mocked(api.listDesktopPools).mockResolvedValue([
      {
        id: 'p1', name: 'sales', template_vm_id: 't1', prefix: 'sales', size: 5, clone_mode: 'linked',
        persistent: false, created_by: 'admin', created_at: '', updated_at: '',
      },
    ])
    vi.mocked(platform.listPlatformVms).mockResolvedValue([
      { id: 'a', name: 'golden-win11', observed_state: 'shutoff' },
      { id: 'b', name: 'busy-vm', observed_state: 'running' },
    ] as never)
    render(<MemoryRouter><PlatformDesktopPools /></MemoryRouter>)
    await waitFor(() => expect(screen.getByText('sales')).toBeTruthy())
    expect(screen.getByText('5 desktops · non-persistent · linked clones')).toBeTruthy()
    expect(screen.getByRole('option', { name: 'golden-win11' })).toBeTruthy()
    expect(screen.queryByRole('option', { name: 'busy-vm' })).toBeNull()
  })
})

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import HostFleetCard from './HostFleetCard'
import type { PlatformHost } from '../../../api/platform'

afterEach(cleanup)
const NOW = Date.parse('2026-10-07T12:00:00Z')
const host = (o: Partial<PlatformHost> = {}): PlatformHost => ({
  id: 'h1', hostname: 'NLDW4-4-04-32', address: '175.110.122.71', state: 'online', maintenance_mode: false, agent_grpc_addr: '127.0.0.1:50051',
  vm_count: 1, cpu_percent: 55, memory_used_mib: 7000, memory_total_mib: 10000, fenced: false, schedulable: true,
  validation_status: 'passed', last_heartbeat_at: '2026-10-07T11:59:58Z', site: 'ams', rack: 'r1', tags: ['gpu'], ...o,
})
const view = (h: PlatformHost) => render(<MemoryRouter><HostFleetCard host={h} now={NOW} /></MemoryRouter>)

describe('HostFleetCard', () => {
  it('shows real data and never the old hard-coded Network OK', () => {
    view(host())
    expect(screen.getByText('NLDW4-4-04-32')).toBeTruthy()
    expect(screen.getByTestId('host-card-ip').textContent).toBe('175.110.122.71')
    expect(screen.getByTestId('host-card-role').textContent).toBe('Controller')
    expect(screen.getByTestId('host-card-heartbeat').textContent).toContain('just now')
    expect(screen.getByTestId('host-card-validation').textContent).toBe('Validated')
    expect(screen.getByTestId('host-card-labels').textContent).toContain('ams / r1')
    expect(screen.getByText('gpu')).toBeTruthy()
    expect(screen.queryByText('Network')).toBeNull()
    expect(screen.queryByText('OK')).toBeNull()
    expect(screen.getByTestId('host-card-state').textContent).toBe('Healthy')
  })
  it('shows the transport and agent version only when the controller reports them', () => {
    view(host())
    expect(screen.queryByTestId('host-card-transport')).toBeNull()
    expect(screen.queryByTestId('host-card-agent')).toBeNull()
    cleanup()
    view(host({ transport: 'mtls', agent_version: '0.1.0' }))
    expect(screen.getByTestId('host-card-transport').textContent).toContain('mTLS')
    expect(screen.getByTestId('host-card-agent').textContent).toContain('0.1.0')
  })
  it('says what is wrong instead of Healthy', () => {
    view(host({ validation_status: 'failed', agent_grpc_addr: '10.0.0.5:50051' }))
    expect(screen.getByTestId('host-card-state').textContent).toBe('Needs attention')
    expect(screen.getByTestId('host-card-validation').textContent).toBe('Validation failed')
    expect(screen.getByTestId('host-card-role').textContent).toBe('Node')
  })
  it('an offline host shows its state and no load rings', () => {
    view(host({ state: 'offline', cpu_percent: 0 }))
    expect(screen.getByTestId('host-card-state').textContent).toBe('offline')
    expect(screen.getAllByRole('img').every((e) => /unknown/.test(e.getAttribute('aria-label') || ''))).toBe(true)
  })
})

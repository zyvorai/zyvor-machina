// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import HostDrawer from './HostDrawer'
import type { PlatformHost } from '../../../api/platform'

vi.mock('../../../api/platform', () => ({
  getPlatformHostDetail: vi.fn(),
  enqueueValidateHost: vi.fn(async () => ({ task_id: 't' })),
  hostMaintenance: vi.fn(async () => ({ task_id: 't' })),
  syncHost: vi.fn(async () => ({ task_id: 't' })),
  fenceHost: vi.fn(async () => ({ fenced: true })),
  deleteHost: vi.fn(async () => ({ deleted: true })),
}))
import * as api from '../../../api/platform'

// jsdom has no layout: treat every attached element as rendered so the dialog's focus trap works.
Object.defineProperty(HTMLElement.prototype, 'offsetParent', { configurable: true, get() { return (this as HTMLElement).parentNode } })

const host = (o: Partial<PlatformHost> = {}): PlatformHost => ({
  id: 'h1', hostname: 'zz-node1', address: '192.168.122.122', state: 'online', maintenance_mode: false, agent_grpc_addr: '192.168.122.122:50051',
  vm_count: 0, cpu_percent: 5, memory_used_mib: 100, memory_total_mib: 1000, fenced: false, schedulable: true,
  validation_status: 'failed', last_heartbeat_at: null, site: '', rack: '', ...o,
})

const mount = (h = host(), run = vi.fn(async (_l: string, fn: () => Promise<unknown>) => { await fn(); return true }), onClose = vi.fn()) => {
  render(<MemoryRouter><HostDrawer host={h} busy={false} run={run} onClose={onClose} /></MemoryRouter>)
  return { run, onClose }
}

beforeEach(() => {
  vi.mocked(api.getPlatformHostDetail).mockResolvedValue({
    ...host(), agent_console_addr: '', libvirt_uri: 'qemu:///system', agent_version: '0.1.0', cpu_model: 'EPYC', libvirt_version: '10.0', qemu_version: '8.2', notes: '',
    validation_report: [
      { name: 'libvirt_uri', passed: true, message: 'URI qemu:///system' },
      { name: 'agent_reachable', passed: false, message: 'Cannot connect to agent', remediation: 'Check agent logs: journalctl -u machina-agent' },
    ],
  })
})
afterEach(() => { cleanup(); vi.clearAllMocks() })

describe('HostDrawer', () => {
  it('shows the failing check with its fix, loaded lazily', async () => {
    mount()
    await waitFor(() => expect(screen.getByTestId('check-agent_reachable')).toBeTruthy())
    expect(screen.getByTestId('check-agent_reachable').textContent).toContain('journalctl -u machina-agent')
    expect(screen.getByTestId('drawer-failing').textContent).toContain('1 check failing')
    expect(screen.getByText('EPYC')).toBeTruthy()
  })
  it('Re-check queues validation', async () => {
    mount()
    fireEvent.click(screen.getByTestId('drawer-recheck'))
    await waitFor(() => expect(api.enqueueValidateHost).toHaveBeenCalledWith('h1'))
  })
  it('maintenance enters with the evacuate choice and exits when already in it', async () => {
    mount()
    fireEvent.click(screen.getByLabelText('Move VMs off'))
    fireEvent.click(screen.getByText('Enter maintenance'))
    await waitFor(() => expect(api.hostMaintenance).toHaveBeenCalledWith('h1', 'enter', false))
    cleanup()
    mount(host({ maintenance_mode: true }))
    fireEvent.click(screen.getByText('Exit maintenance'))
    await waitFor(() => expect(api.hostMaintenance).toHaveBeenCalledWith('h1', 'exit'))
  })
  it('fence asks first; nothing happens until confirmed', async () => {
    mount()
    fireEvent.click(screen.getByText('Fence'))
    expect(api.fenceHost).not.toHaveBeenCalled()
    fireEvent.click(screen.getByText('Fence host'))
    await waitFor(() => expect(api.fenceHost).toHaveBeenCalledWith('h1'))
  })
  it('remove needs the host name typed, then deletes and closes', async () => {
    const { onClose } = mount()
    fireEvent.click(screen.getByTestId('drawer-remove'))
    const confirmBtn = screen.getByText('Remove host') as HTMLButtonElement
    expect(confirmBtn.disabled).toBe(true)
    expect(api.deleteHost).not.toHaveBeenCalled()
    fireEvent.change(screen.getByPlaceholderText('zz-node1'), { target: { value: 'zz-node1' } })
    expect(confirmBtn.disabled).toBe(false)
    await act(async () => { fireEvent.click(confirmBtn) })
    await waitFor(() => expect(api.deleteHost).toHaveBeenCalledWith('h1'))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
  })
  it('a failed removal does not close the drawer', async () => {
    const run = vi.fn(async () => false)
    const { onClose } = mount(host(), run)
    fireEvent.click(screen.getByTestId('drawer-remove'))
    fireEvent.change(screen.getByPlaceholderText('zz-node1'), { target: { value: 'zz-node1' } })
    await act(async () => { fireEvent.click(screen.getByText('Remove host')) })
    expect(run).toHaveBeenCalled()
    expect(onClose).not.toHaveBeenCalled()
  })
})

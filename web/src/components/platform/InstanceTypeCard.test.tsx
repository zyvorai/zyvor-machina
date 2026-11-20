// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import InstanceTypeCard from './InstanceTypeCard'

const api = vi.hoisted(() => ({ listFlavors: vi.fn(), changeVmType: vi.fn() }))
vi.mock('../../api/flavors', () => ({ listFlavors: api.listFlavors }))
vi.mock('../../api/nativeVms', () => ({ changeVmType: api.changeVmType }))

const flavors = [
  { id: 'f-small', name: 'small', vcpus: 1, memory_mib: 2048, disk_gib: 20, description: '', is_public: true },
  { id: 'f-large', name: 'large', vcpus: 4, memory_mib: 8192, disk_gib: 80, description: '', is_public: true },
]
const vm = (over = {}) => ({ id: 'v1', name: 'web-1', vcpus: 1, memory_mib: 2048, flavor_id: 'f-small', observed_state: 'running', ...over })

afterEach(cleanup)
beforeEach(() => { vi.resetAllMocks(); api.listFlavors.mockResolvedValue(flavors) })

it('shows the current type and offers only the other types', async () => {
  render(<InstanceTypeCard vm={vm()} />)
  expect(await screen.findByText('small')).toBeTruthy()
  expect(screen.getByRole('option', { name: /large — 4 vCPU · 8 GiB/ })).toBeTruthy()
  expect(screen.queryByRole('option', { name: /^small —/ })).toBeNull()
})

it('warns that a running machine restarts and only then queues the change', async () => {
  api.changeVmType.mockResolvedValue({ task_id: 't' })
  const onQueued = vi.fn()
  render(<InstanceTypeCard vm={vm()} onQueued={onQueued} />)
  await screen.findByText('small')
  fireEvent.change(screen.getByLabelText('New instance type'), { target: { value: 'f-large' } })
  fireEvent.click(screen.getByRole('button', { name: 'Change type…' }))
  expect(screen.getByRole('group', { name: 'Confirm instance type change' }).textContent).toContain('shut down cleanly')
  expect(api.changeVmType).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Shut down and resize' }))
  await waitFor(() => expect(api.changeVmType).toHaveBeenCalledWith('v1', 'f-large'))
  await waitFor(() => expect(onQueued).toHaveBeenCalled())
  expect(await screen.findByText(/Change queued/)).toBeTruthy()
})

it('says a stopped machine stays stopped', async () => {
  render(<InstanceTypeCard vm={vm({ observed_state: 'shutoff' })} />)
  await screen.findByText('small')
  fireEvent.change(screen.getByLabelText('New instance type'), { target: { value: 'f-large' } })
  fireEvent.click(screen.getByRole('button', { name: 'Change type…' }))
  expect(screen.getByRole('group', { name: 'Confirm instance type change' }).textContent).toContain('stays stopped')
})

it('shows the server reason when the change is refused', async () => {
  api.changeVmType.mockRejectedValue(new Error('This machine is managed by an instance group'))
  render(<InstanceTypeCard vm={vm()} />)
  await screen.findByText('small')
  fireEvent.change(screen.getByLabelText('New instance type'), { target: { value: 'f-large' } })
  fireEvent.click(screen.getByRole('button', { name: 'Change type…' }))
  fireEvent.click(screen.getByRole('button', { name: 'Shut down and resize' }))
  expect((await screen.findByRole('alert')).textContent).toContain('instance group')
})

it('labels a machine with no recorded flavor as a custom size', async () => {
  render(<InstanceTypeCard vm={vm({ flavor_id: null, vcpus: 3, memory_mib: 6144 })} />)
  expect(await screen.findByText('Custom size')).toBeTruthy()
  expect(screen.getByText(/3 vCPU · 6 GiB/)).toBeTruthy()
})

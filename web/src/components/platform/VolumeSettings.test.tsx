// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import VolumeSettings from './VolumeSettings'

const api = vi.hoisted(() => ({ setVolumeIoLimits: vi.fn(), setVolumeDeleteOnTermination: vi.fn() }))
vi.mock('../../api/nativeVolumes', () => api)

const vol = (over = {}) => ({
  id: 'vol1', project_id: null, name: 'data', size_gib: 10, volume_class: 'silver', status: 'in-use',
  attached_vm_id: 'v1', attached_device: 'vdb', atlas_backed: false, delete_on_termination: false, ...over,
})

afterEach(cleanup)
beforeEach(() => vi.resetAllMocks())

it('sends only the limits that were filled in', async () => {
  api.setVolumeIoLimits.mockResolvedValue(vol())
  render(<VolumeSettings volume={vol()} />)
  fireEvent.change(screen.getByLabelText('Read IOPS'), { target: { value: '500' } })
  fireEvent.click(screen.getByRole('button', { name: 'Apply limits' }))
  await waitFor(() => expect(api.setVolumeIoLimits).toHaveBeenCalledWith('vol1', { read_iops: 500 }))
  expect(await screen.findByText(/applied to the running disk/)).toBeTruthy()
})

it('rejects a bad number and an empty form without calling the API', async () => {
  render(<VolumeSettings volume={vol()} />)
  fireEvent.click(screen.getByRole('button', { name: 'Apply limits' }))
  expect(await screen.findByText('Enter at least one limit')).toBeTruthy()
  fireEvent.change(screen.getByLabelText('Write IOPS'), { target: { value: '-3' } })
  fireEvent.click(screen.getByRole('button', { name: 'Apply limits' }))
  expect(await screen.findByText(/Write IOPS must be a whole number/)).toBeTruthy()
  expect(api.setVolumeIoLimits).not.toHaveBeenCalled()
})

it('toggles delete-on-termination and reverts when the change fails', async () => {
  api.setVolumeDeleteOnTermination.mockRejectedValue(new Error('nope'))
  render(<VolumeSettings volume={vol()} />)
  const box = screen.getByRole('checkbox') as HTMLInputElement
  fireEvent.click(box)
  await waitFor(() => expect(api.setVolumeDeleteOnTermination).toHaveBeenCalledWith('vol1', true))
  await waitFor(() => expect(box.checked).toBe(false))
})

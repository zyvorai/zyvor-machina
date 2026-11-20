// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { listVmSchedules, createVmSchedule, deleteVmSchedule } from './platformVmSchedules'

vi.mock('./platform', () => ({
  platformFetch: vi.fn(),
}))

import { platformFetch } from './platform'
const mockFetch = vi.mocked(platformFetch)

beforeEach(() => { mockFetch.mockReset() })

const VM_ID = 'aaaabbbb-0000-0000-0000-000000000001'
const SCHED_ID = 'ccccdddd-0000-0000-0000-000000000002'

describe('listVmSchedules', () => {
  it('calls GET on the right path', async () => {
    mockFetch.mockResolvedValue([])
    await listVmSchedules(VM_ID)
    expect(mockFetch).toHaveBeenCalledWith(`/api/v1/vms/${VM_ID}/schedules`)
  })

  it('returns the resolved value', async () => {
    const schedules = [{ id: SCHED_ID, action: 'snapshot' }]
    mockFetch.mockResolvedValue(schedules)
    const result = await listVmSchedules(VM_ID)
    expect(result).toEqual(schedules)
  })
})

describe('createVmSchedule', () => {
  it('calls POST with JSON body', async () => {
    mockFetch.mockResolvedValue({ id: SCHED_ID })
    await createVmSchedule(VM_ID, { action: 'snapshot', interval_minutes: 1440, retention: 5 })
    expect(mockFetch).toHaveBeenCalledWith(
      `/api/v1/vms/${VM_ID}/schedules`,
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ action: 'snapshot', interval_minutes: 1440, retention: 5 }),
      }),
    )
  })
})

describe('deleteVmSchedule', () => {
  it('calls DELETE on the schedule sub-path', async () => {
    mockFetch.mockResolvedValue({ ok: true })
    await deleteVmSchedule(VM_ID, SCHED_ID)
    expect(mockFetch).toHaveBeenCalledWith(
      `/api/v1/vms/${VM_ID}/schedules/${SCHED_ID}`,
      expect.objectContaining({ method: 'DELETE' }),
    )
  })
})

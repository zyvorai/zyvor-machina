// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import DigestScheduleControls from './DigestScheduleControls'
import * as api from '../../api/ai'

vi.mock('../../api/ai', () => ({
  getDigestSchedule: vi.fn(),
  setDigestSchedule: vi.fn(),
  sendDigestNow: vi.fn(),
}))

const off = { enabled: false, frequency: 'daily', hour_utc: 8, weekday: 1, last_sent_at: null } as const

describe('DigestScheduleControls', () => {
  afterEach(() => cleanup())

  beforeEach(() => {
    vi.mocked(api.getDigestSchedule).mockResolvedValue({ ...off })
    vi.mocked(api.setDigestSchedule).mockReset()
    vi.mocked(api.sendDigestNow).mockReset()
  })

  it('starts off and says nothing was sent', async () => {
    render(<DigestScheduleControls />)
    await waitFor(() => expect(screen.getByText(/Not sent yet/)).toBeTruthy())
    expect((screen.getByLabelText('Send this digest automatically') as HTMLInputElement).checked).toBe(false)
  })

  it('saves the chosen schedule', async () => {
    vi.mocked(api.setDigestSchedule).mockResolvedValue({ ...off, enabled: true, frequency: 'weekly', hour_utc: 6, weekday: 3 })
    render(<DigestScheduleControls />)
    await waitFor(() => expect((screen.getByText('Save') as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByLabelText('Send this digest automatically'))
    fireEvent.change(screen.getByLabelText('Digest frequency'), { target: { value: 'weekly' } })
    fireEvent.change(screen.getByLabelText('Digest weekday'), { target: { value: '3' } })
    fireEvent.change(screen.getByLabelText('Digest hour (UTC)'), { target: { value: '6' } })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(screen.getByText('Schedule saved')).toBeTruthy())
    expect(api.setDigestSchedule).toHaveBeenCalledWith({ enabled: true, frequency: 'weekly', hour_utc: 6, weekday: 3 })
  })

  it('shows the error when a non-admin tries to save', async () => {
    vi.mocked(api.setDigestSchedule).mockRejectedValue(new Error('admin role required'))
    render(<DigestScheduleControls />)
    await waitFor(() => expect((screen.getByText('Save') as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(screen.getByText(/admin role required/)).toBeTruthy())
  })
})

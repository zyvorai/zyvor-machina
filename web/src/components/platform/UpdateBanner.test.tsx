// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import UpdateBanner from './UpdateBanner'

vi.mock('../../api/platform', () => ({ getUpdateCheck: vi.fn() }))
import { getUpdateCheck } from '../../api/platform'

afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear() })

describe('UpdateBanner', () => {
  it('shows nothing when no check URL is configured (air-gapped)', async () => {
    vi.mocked(getUpdateCheck).mockResolvedValue({ enabled: false, current: '0.1.0', available: false })
    render(<UpdateBanner />)
    await waitFor(() => expect(getUpdateCheck).toHaveBeenCalled())
    expect(screen.queryByTestId('update-banner')).toBeNull()
  })
  it('shows nothing when the check fails', async () => {
    vi.mocked(getUpdateCheck).mockRejectedValue(new Error('offline'))
    render(<UpdateBanner />)
    await waitFor(() => expect(getUpdateCheck).toHaveBeenCalled())
    expect(screen.queryByTestId('update-banner')).toBeNull()
  })
  it('announces a newer version and stays dismissed for that version', async () => {
    vi.mocked(getUpdateCheck).mockResolvedValue({ enabled: true, current: '0.1.0', latest: '0.2.0', available: true, notes_url: 'https://example.com/notes' })
    render(<UpdateBanner />)
    await waitFor(() => expect(screen.getByTestId('update-banner')).toBeTruthy())
    expect(screen.getByText(/0\.2\.0 is available/)).toBeTruthy()
    fireEvent.click(screen.getByLabelText('Dismiss the update notice'))
    expect(screen.queryByTestId('update-banner')).toBeNull()
    cleanup()
    render(<UpdateBanner />)
    await waitFor(() => expect(getUpdateCheck).toHaveBeenCalledTimes(2))
    expect(screen.queryByTestId('update-banner')).toBeNull()
  })
})

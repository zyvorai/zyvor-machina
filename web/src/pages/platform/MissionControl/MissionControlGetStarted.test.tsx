// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import MissionControlGetStarted from './MissionControlGetStarted'

vi.mock('../../../api/day2', () => ({ listNotificationChannelRows: vi.fn() }))
vi.mock('../../../components/Reveal', () => ({ default: ({ children }: { children: unknown }) => <>{children}</> }))
import { listNotificationChannelRows } from '../../../api/day2'

afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear() })

const state = { hosts: [], vms: [], unprotected: 0, loading: false, error: null } as never

const show = () => render(<MemoryRouter><MissionControlGetStarted state={state} onCreateVm={() => {}} /></MemoryRouter>)

describe('MissionControlGetStarted notification step', () => {
  it('asks for a channel when none is set up', async () => {
    vi.mocked(listNotificationChannelRows).mockResolvedValue([])
    show()
    await waitFor(() => expect(screen.getByText('Get told when something breaks')).toBeTruthy())
    expect(screen.getByText('Add channel')).toBeTruthy()
  })
  it('marks it done once an enabled channel exists', async () => {
    vi.mocked(listNotificationChannelRows).mockResolvedValue([{ id: '1', name: 'ops', kind: 'slack', target: 'https://x', events: '[]', enabled: true }])
    show()
    await waitFor(() => expect(screen.getByText('Get told when something breaks')).toBeTruthy())
    expect(screen.queryByText('Add channel')).toBeNull()
  })
  it('does not nag when the channel list cannot be read', async () => {
    vi.mocked(listNotificationChannelRows).mockRejectedValue(new Error('403'))
    show()
    await waitFor(() => expect(listNotificationChannelRows).toHaveBeenCalled())
    expect(screen.queryByText('Get told when something breaks')).toBeNull()
  })
})

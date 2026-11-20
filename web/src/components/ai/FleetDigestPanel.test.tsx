// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import FleetDigestPanel from './FleetDigestPanel'

vi.mock('../platform/mac/PlatformMacUi', () => ({
  MacGlassPanel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}))
vi.mock('../../api/ai', () => ({
  getFleetDigest: vi.fn().mockResolvedValue({
    hours: 24, hosts_total: 3, hosts_not_online: 0, vms_total: 10, vms_running: 9, open_incidents: 1,
    pending_approvals: 2, actions_executed: 4, actions_failed: 0, actions_rejected: 1, actions_undone: 0,
    budget_status: null, budget_alerts: [], headline: 'attention', markdown: '',
  }),
  getDigestSchedule: vi.fn().mockResolvedValue({
    enabled: false, frequency: 'daily', hour_utc: 8, weekday: 1, last_sent_at: null,
  }),
  setDigestSchedule: vi.fn(),
  sendDigestNow: vi.fn(),
}))

describe('FleetDigestPanel', () => {
  it('shows the headline and counts', async () => {
    render(<FleetDigestPanel />)
    await waitFor(() => expect(screen.getByText('A few things need a look')).toBeTruthy())
    expect(screen.getByText('Pending approvals')).toBeTruthy()
  })
})

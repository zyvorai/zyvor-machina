// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import TaskFailuresPanel from './TaskFailuresPanel'

vi.mock('../../api/platform', () => ({ getTaskFailures: vi.fn(), acknowledgeTaskFailures: vi.fn() }))
vi.mock('../../contexts/ToastContext', () => ({ useToastContext: () => ({ success: vi.fn(), error: vi.fn(), warning: vi.fn() }) }))
import { acknowledgeTaskFailures, getTaskFailures } from '../../api/platform'

afterEach(() => { cleanup(); vi.clearAllMocks() })

const summary = {
  window_hours: 24,
  unacknowledged: 3,
  groups: [
    { operation: 'host.sync', cause: 'transport error: Connection refused', count: 2, last_at: '2026-10-07 10:05:00', sample_task_id: '2', hint: 'The agent is not reachable. Check that machina-agent is running.' },
    { operation: 'vm.start', cause: 'libvirt connect failed', count: 1, last_at: '2026-10-07 10:06:00', sample_task_id: '3', hint: null },
  ],
}

describe('TaskFailuresPanel', () => {
  it('shows each cause with its count and the suggested fix', async () => {
    vi.mocked(getTaskFailures).mockResolvedValue(summary)
    render(<TaskFailuresPanel />)
    await waitFor(() => expect(screen.getAllByTestId('task-failure-group')).toHaveLength(2))
    expect(screen.getByText('2×')).toBeTruthy()
    expect(screen.getByText(/machina-agent is running/)).toBeTruthy()
  })
  it('acknowledges one operation and reloads', async () => {
    vi.mocked(getTaskFailures).mockResolvedValue(summary)
    vi.mocked(acknowledgeTaskFailures).mockResolvedValue({ acknowledged: 2 })
    const changed = vi.fn()
    render(<TaskFailuresPanel onChanged={changed} />)
    await waitFor(() => screen.getAllByText('Acknowledge'))
    fireEvent.click(screen.getAllByText('Acknowledge')[0])
    await waitFor(() => expect(acknowledgeTaskFailures).toHaveBeenCalledWith('host.sync'))
    await waitFor(() => expect(changed).toHaveBeenCalled())
  })
  it('renders nothing when there is nothing to fix', async () => {
    vi.mocked(getTaskFailures).mockResolvedValue({ window_hours: 24, unacknowledged: 0, groups: [] })
    const { container } = render(<TaskFailuresPanel />)
    await waitFor(() => expect(getTaskFailures).toHaveBeenCalled())
    expect(container.querySelector('[data-testid="task-failures"]')).toBeNull()
  })
})

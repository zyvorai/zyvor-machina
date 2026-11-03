// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import ChangeReviewPanel from './ChangeReviewPanel'

vi.mock('../../api/ai', () => ({
  getZyraChangeReview: vi.fn().mockResolvedValue({
    action_id: 'a1', action_type: 'start_vm', risk: 'low', verdict: 'approve',
    summary: 'Low risk and reversible.', blast_radius: ['Machine `web-1` (shutoff)'],
    rollback: 'One-click undo.', reversible: true, checks: [], approved_streak: 3,
  }),
}))

describe('ChangeReviewPanel', () => {
  it('loads the review only when opened', async () => {
    const { getZyraChangeReview } = await import('../../api/ai')
    render(<ChangeReviewPanel actionId="a1" />)
    expect(getZyraChangeReview).not.toHaveBeenCalled()
    fireEvent.click(screen.getByText('What happens if I approve?'))
    await waitFor(() => expect(screen.getByText('Safe to approve')).toBeTruthy())
    expect(screen.getByText(/web-1/)).toBeTruthy()
  })
})

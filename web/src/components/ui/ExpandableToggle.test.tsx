// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom
// Copyright (c) 2026 ZyvorAI Labs Private Limited. All rights reserved.

import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import React from 'react'
import { ExpandableToggle } from './ExpandableToggle'

afterEach(cleanup)

describe('ExpandableToggle', () => {
  it('labels the collapsed state with the hidden count and reports aria-expanded/aria-controls', () => {
    const onToggle = vi.fn()
    render(<ExpandableToggle expanded={false} hidden={12} listId="my-list" onToggle={onToggle} noun="events" />)
    const btn = screen.getByRole('button', { name: 'Show 12 more events' })
    expect(btn.getAttribute('aria-expanded')).toBe('false')
    expect(btn.getAttribute('aria-controls')).toBe('my-list')
    fireEvent.click(btn)
    expect(onToggle).toHaveBeenCalledTimes(1)
  })

  it('labels the expanded state as "fewer"', () => {
    render(<ExpandableToggle expanded hidden={0} listId="my-list" onToggle={() => {}} noun="rows" />)
    const btn = screen.getByRole('button', { name: 'Show fewer rows' })
    expect(btn.getAttribute('aria-expanded')).toBe('true')
  })
})

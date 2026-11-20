// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom
// Copyright (c) 2026 ZyvorAI Labs Private Limited. All rights reserved.

import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import React, { useState } from 'react'
import { SegmentedControl } from './SegmentedControl'

afterEach(cleanup)

const OPTIONS = [
  { value: 'table', label: 'Table' },
  { value: 'grid', label: 'Grid' },
  { value: 'map', label: 'Map' },
] as const

function Harness() {
  const [v, setV] = useState<'table' | 'grid' | 'map'>('table')
  return <SegmentedControl ariaLabel="View" value={v} onChange={setV} options={OPTIONS} />
}

describe('SegmentedControl', () => {
  it('is a named radiogroup whose selected option is checked and tabbable', () => {
    render(<Harness />)
    expect(screen.getByRole('radiogroup', { name: 'View' })).toBeTruthy()
    const table = screen.getByRole('radio', { name: 'Table' })
    const grid = screen.getByRole('radio', { name: 'Grid' })
    expect(table.getAttribute('aria-checked')).toBe('true')
    expect(table.getAttribute('tabindex')).toBe('0')
    expect(grid.getAttribute('aria-checked')).toBe('false')
    expect(grid.getAttribute('tabindex')).toBe('-1')
  })

  it('selects on click', () => {
    render(<Harness />)
    fireEvent.click(screen.getByRole('radio', { name: 'Grid' }))
    expect(screen.getByRole('radio', { name: 'Grid' }).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByRole('radio', { name: 'Table' }).getAttribute('aria-checked')).toBe('false')
  })

  it('moves the selection and focus with the arrow keys, wrapping at both ends', () => {
    render(<Harness />)
    const table = screen.getByRole('radio', { name: 'Table' })
    table.focus()
    fireEvent.keyDown(table, { key: 'ArrowRight' })
    expect(document.activeElement).toBe(screen.getByRole('radio', { name: 'Grid' }))
    expect(screen.getByRole('radio', { name: 'Grid' }).getAttribute('aria-checked')).toBe('true')
    fireEvent.keyDown(document.activeElement as Element, { key: 'ArrowLeft' })
    fireEvent.keyDown(document.activeElement as Element, { key: 'ArrowLeft' })
    expect(document.activeElement).toBe(screen.getByRole('radio', { name: 'Map' }))
    fireEvent.keyDown(document.activeElement as Element, { key: 'Home' })
    expect(screen.getByRole('radio', { name: 'Table' }).getAttribute('aria-checked')).toBe('true')
  })
})

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom
// Copyright (c) 2026 ZyvorAI Labs Private Limited. All rights reserved.

import { afterEach, describe, expect, it } from 'vitest'
import { act, cleanup, renderHook } from '@testing-library/react'
import { useExpandable } from './useExpandable'

afterEach(cleanup)

const items = Array.from({ length: 8 }, (_, i) => `row-${i}`)

describe('useExpandable', () => {
  it('shows the top N collapsed, with a toggle and the hidden count', () => {
    const { result } = renderHook(() => useExpandable(items, 4))
    expect(result.current.shown).toEqual(['row-0', 'row-1', 'row-2', 'row-3'])
    expect(result.current.hidden).toBe(4)
    expect(result.current.expanded).toBe(false)
    expect(result.current.showToggle).toBe(true)
    expect(result.current.listId).toBeTruthy()
  })

  it('toggle flips expanded and reveals everything', () => {
    const { result } = renderHook(() => useExpandable(items, 4))
    act(() => result.current.toggle())
    expect(result.current.expanded).toBe(true)
    expect(result.current.shown).toHaveLength(8)
    expect(result.current.hidden).toBe(0)
    act(() => result.current.toggle())
    expect(result.current.expanded).toBe(false)
    expect(result.current.shown).toHaveLength(4)
  })

  it('hides the toggle when the list is at or under the limit', () => {
    const { result } = renderHook(() => useExpandable(items.slice(0, 4), 4))
    expect(result.current.showToggle).toBe(false)
  })
})

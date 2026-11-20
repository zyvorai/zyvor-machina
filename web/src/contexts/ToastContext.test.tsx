// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom
// Copyright (c) 2026 ZyvorAI Labs Private Limited. All rights reserved.

import { describe, it, expect, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import React from 'react'
import { ToastProvider, useToastContext } from './ToastContext'
import { useToast } from '../hooks/useToast'

describe('useToastContext', () => {
  it('throws when used outside ToastProvider', () => {
    // Suppress React error boundary noise in the test
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    expect(() => renderHook(() => useToastContext())).toThrow(
      'useToastContext must be used within ToastProvider',
    )
    spy.mockRestore()
  })

  it('returns success/error/warning/info functions when inside provider', () => {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <ToastProvider>{children}</ToastProvider>
    )
    const { result } = renderHook(() => useToastContext(), { wrapper })
    expect(typeof result.current.success).toBe('function')
    expect(typeof result.current.error).toBe('function')
    expect(typeof result.current.warning).toBe('function')
    expect(typeof result.current.info).toBe('function')
  })
})

describe('useToast', () => {
  it('addToast via success returns an id string', () => {
    const { result } = renderHook(() => useToast())
    let id: string
    act(() => {
      id = result.current.success('hello')
    })
    expect(typeof id!).toBe('string')
    expect(id!.length).toBeGreaterThan(0)
  })

  it('success toast appears in toasts list', () => {
    const { result } = renderHook(() => useToast())
    act(() => {
      result.current.success('hello world')
    })
    expect(result.current.toasts).toHaveLength(1)
    expect(result.current.toasts[0].message).toBe('hello world')
    expect(result.current.toasts[0].type).toBe('success')
  })

  it('error toast deduplicates within 8 seconds', () => {
    const { result } = renderHook(() => useToast())
    act(() => {
      result.current.error('connection failed')
      result.current.error('connection failed')
    })
    expect(result.current.toasts).toHaveLength(1)
  })

  it('removeToast removes the toast by id', () => {
    const { result } = renderHook(() => useToast())
    let id: string
    act(() => {
      id = result.current.success('to be removed')
    })
    act(() => {
      result.current.removeToast(id!)
    })
    expect(result.current.toasts).toHaveLength(0)
  })

  it('clearAll empties the toast list', () => {
    const { result } = renderHook(() => useToast())
    act(() => {
      result.current.success('a')
      result.current.warning('b')
      result.current.info('c')
    })
    expect(result.current.toasts).toHaveLength(3)
    act(() => {
      result.current.clearAll()
    })
    expect(result.current.toasts).toHaveLength(0)
  })
})

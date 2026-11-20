// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom
// Copyright (c) 2026 ZyvorAI Labs Private Limited. All rights reserved.

import { describe, it, expect } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import React, { useRef, useState } from 'react'
import { afterEach } from 'vitest'
import { useFocusTrap } from './useFocusTrap'
import { GlassModal } from '../components/glass/GlassModal'

afterEach(cleanup)

function TrapDialog({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null)
  useFocusTrap(ref, true, onClose)
  return (
    <div ref={ref} role="dialog" aria-label="Rename">
      {/* autoFocus is applied at commit, before any effect runs: the case that used to lose focus. */}
      <input autoFocus aria-label="name" />
      <button onClick={onClose}>Close</button>
    </div>
  )
}

function TrapHarness() {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button onClick={() => setOpen(true)}>Open</button>
      {open && <TrapDialog onClose={() => setOpen(false)} />}
    </>
  )
}

describe('useFocusTrap', () => {
  it('returns focus to the trigger when a dialog with an autoFocus child closes', () => {
    render(<TrapHarness />)
    const trigger = screen.getByRole('button', { name: 'Open' })
    trigger.focus()
    fireEvent.click(trigger)
    expect(screen.getByLabelText('name')).toBeTruthy()
    expect(document.activeElement).toBe(screen.getByLabelText('name'))

    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByLabelText('name')).toBeNull()
    expect(document.activeElement).toBe(trigger)
  })

  it('returns focus to the trigger when Escape closes it', () => {
    render(<TrapHarness />)
    const trigger = screen.getByRole('button', { name: 'Open' })
    trigger.focus()
    fireEvent.click(trigger)
    act(() => {
      fireEvent.keyDown(document, { key: 'Escape' })
    })
    expect(screen.queryByLabelText('name')).toBeNull()
    expect(document.activeElement).toBe(trigger)
  })
})

function ModalHarness() {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button onClick={() => setOpen(true)}>Open modal</button>
      <GlassModal open={open} onClose={() => setOpen(false)} title="Edit">
        <input autoFocus aria-label="field" />
      </GlassModal>
    </>
  )
}

describe('GlassModal focus', () => {
  it('moves focus in on open and back to the trigger on close, even with an autoFocus child', () => {
    render(<ModalHarness />)
    const trigger = screen.getByRole('button', { name: 'Open modal' })
    trigger.focus()
    fireEvent.click(trigger)
    expect(screen.getByRole('dialog')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(document.activeElement).toBe(trigger)
  })
})

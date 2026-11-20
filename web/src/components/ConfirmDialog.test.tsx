// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom
// Copyright (c) 2026 ZyvorAI Labs Private Limited. All rights reserved.

import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import React, { useState } from 'react'
import ConfirmDialog from './ConfirmDialog'

afterEach(cleanup)

// jsdom has no layout, so offsetParent is always null and useFocusTrap's visibility filter sees no
// focusable controls. Treat every attached element as rendered, as a real browser would.
Object.defineProperty(HTMLElement.prototype, 'offsetParent', {
  configurable: true,
  get() {
    return (this as HTMLElement).parentNode
  },
})

const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 5)) })

function Harness({ typeToMatch, onConfirm = () => {} }: { typeToMatch?: string; onConfirm?: () => void }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button onClick={() => setOpen(true)}>Delete VM</button>
      <ConfirmDialog
        open={open}
        title="Delete vm-1?"
        message="This cannot be undone."
        confirmLabel="Delete"
        typeToMatch={typeToMatch}
        onConfirm={() => { onConfirm(); setOpen(false) }}
        onCancel={() => setOpen(false)}
      />
    </>
  )
}

describe('ConfirmDialog accessibility', () => {
  it('names the dialog with a real heading and describes it with the message', () => {
    render(<Harness />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete VM' }))
    const dialog = screen.getByRole('dialog')
    const heading = screen.getByRole('heading', { name: 'Delete vm-1?' })
    expect(dialog.getAttribute('aria-labelledby')).toBe(heading.id)
    const described = document.getElementById(dialog.getAttribute('aria-describedby') || '')
    expect(described?.textContent).toBe('This cannot be undone.')
  })

  it('moves focus into the dialog and returns it to the trigger on Cancel', async () => {
    render(<Harness />)
    const trigger = screen.getByRole('button', { name: 'Delete VM' })
    trigger.focus()
    fireEvent.click(trigger)
    await settle()
    expect(screen.getByRole('dialog').contains(document.activeElement)).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(document.activeElement).toBe(trigger)
  })

  it('closes on Escape and gives focus back', async () => {
    render(<Harness />)
    const trigger = screen.getByRole('button', { name: 'Delete VM' })
    trigger.focus()
    fireEvent.click(trigger)
    await settle()
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(document.activeElement).toBe(trigger)
  })

  it('keeps focus on the autoFocus type-to-confirm input instead of jumping to Cancel', async () => {
    render(<Harness typeToMatch="vm-1" />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete VM' }))
    await settle()
    expect(document.activeElement).toBe(screen.getByRole('textbox'))
  })

  it('wraps Tab from the last control back to the first', async () => {
    render(<Harness />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete VM' }))
    await settle()
    const confirm = screen.getByRole('button', { name: 'Delete' })
    confirm.focus()
    fireEvent.keyDown(confirm, { key: 'Tab' })
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Cancel' }))
  })

  it('only confirms once the phrase matches', async () => {
    const onConfirm = vi.fn()
    render(<Harness typeToMatch="vm-1" onConfirm={onConfirm} />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete VM' }))
    await settle()
    const confirm = screen.getByRole('button', { name: 'Delete' }) as HTMLButtonElement
    expect(confirm.disabled).toBe(true)
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'vm-1' } })
    expect(confirm.disabled).toBe(false)
    fireEvent.click(confirm)
    expect(onConfirm).toHaveBeenCalledTimes(1)
  })
})

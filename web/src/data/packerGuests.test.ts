// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'

import { MACHINA_PACKER_SCRIPT_GUESTS } from './packerGuests'

describe('MACHINA_PACKER_SCRIPT_GUESTS', () => {
  it('includes Windows dockur profiles including Server SKUs', () => {
    const ids = MACHINA_PACKER_SCRIPT_GUESTS.map((g) => g.id)
    expect(ids).toContain('win10')
    expect(ids).toContain('win11')
    expect(ids).toContain('windows-server-2022')
    expect(ids).toContain('windows-server-2025')
  })

  it('marks Windows guests with windows family and os hints', () => {
    const win11 = MACHINA_PACKER_SCRIPT_GUESTS.find((g) => g.id === 'win11')
    const win10 = MACHINA_PACKER_SCRIPT_GUESTS.find((g) => g.id === 'win10')
    const s2022 = MACHINA_PACKER_SCRIPT_GUESTS.find((g) => g.id === 'windows-server-2022')
    const s2025 = MACHINA_PACKER_SCRIPT_GUESTS.find((g) => g.id === 'windows-server-2025')
    expect(win11?.family).toBe('windows')
    expect(win10?.family).toBe('windows')
    expect(s2022?.family).toBe('windows')
    expect(s2025?.family).toBe('windows')
    expect(win11?.osVariantHint).toBe('win11')
    expect(win10?.osVariantHint).toBe('win10')
    expect(s2022?.osVariantHint).toBe('win2k22')
    expect(s2025?.osVariantHint).toBe('win2k25')
  })
})

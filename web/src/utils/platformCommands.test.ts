// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { parsePlatformCommand } from './platformCommands'

describe('parsePlatformCommand', () => {
  it('parses import storage', () => {
    const cmd = parsePlatformCommand('import storage', 2)
    expect(cmd?.id).toBe('import-storage')
    expect(cmd?.label).toBe('Import storage')
    expect(cmd?.review).toMatch(/2/)
  })

  it('parses migrate vm with destination', () => {
    const cmd = parsePlatformCommand('migrate vm web-01 to host-2', 1)
    expect(cmd?.id).toBe('migrate-vm')
    expect(cmd?.prefill).toEqual({ name: 'web-01', target_host: 'host-2' })
  })

  it('returns null for non-command queries', () => {
    expect(parsePlatformCommand('Finder', 1)).toBeNull()
  })
})

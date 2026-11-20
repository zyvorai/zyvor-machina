// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { summarizeJsonValue } from './JsonInspector'

describe('summarizeJsonValue', () => {
  it('keeps nested firewall payloads compact', () => {
    expect(summarizeJsonValue([{ chain: 'INPUT' }, { chain: 'FORWARD' }])).toBe('2 items')
    expect(summarizeJsonValue({ rules: [], policy: 'accept' })).toBe('2 fields')
  })

  it('renders primitive values directly', () => {
    expect(summarizeJsonValue(false)).toBe('false')
    expect(summarizeJsonValue('nftables')).toBe('nftables')
  })
})

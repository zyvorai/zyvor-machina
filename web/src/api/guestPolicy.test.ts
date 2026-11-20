// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { formatGuestNetRules, parseGuestNetRules } from './guestPolicy'

describe('guest net rules', () => {
  it('parses direction, cidr, optional proto and port', () => {
    expect(parseGuestNetRules('egress 10.0.0.0/8 tcp 443\n# note\n\ningress 192.168.1.0/24')).toEqual([
      { direction: 'egress', cidr: '10.0.0.0/8', proto: 'tcp', port: 443 },
      { direction: 'ingress', cidr: '192.168.1.0/24' },
    ])
    expect(parseGuestNetRules('egress 1.1.1.1 any')).toEqual([{ direction: 'egress', cidr: '1.1.1.1' }])
  })

  it('rejects malformed lines', () => {
    expect(() => parseGuestNetRules('sideways 10.0.0.0/8')).toThrow()
    expect(() => parseGuestNetRules('egress')).toThrow()
    expect(() => parseGuestNetRules('egress 1.1.1.1 tcp 70000')).toThrow()
  })

  it('round-trips through the text form', () => {
    const rules = parseGuestNetRules('egress 10.0.0.0/8 tcp 443\ningress ::/0')
    expect(parseGuestNetRules(formatGuestNetRules(rules))).toEqual(rules)
  })
})

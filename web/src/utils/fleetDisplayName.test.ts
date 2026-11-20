// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import {
  formatFleetDisplayTitle,
  formatPlatformHostLabel,
  isGenericClusterName,
} from './fleetDisplayName'

describe('fleetDisplayName', () => {
  it('detects generic cluster names', () => {
    expect(isGenericClusterName('default')).toBe(true)
    expect(isGenericClusterName('Production Cluster')).toBe(true)
    expect(isGenericClusterName('Edge West')).toBe(false)
  })

  it('formats host label with hostname and address', () => {
    expect(formatPlatformHostLabel({ hostname: 'hypervisor-1', address: '10.0.0.5' })).toBe(
      'hypervisor-1 · 10.0.0.5',
    )
  })

  it('prefers management IP when hostname is localhost', () => {
    expect(formatPlatformHostLabel({ hostname: 'localhost', address: '10.0.0.1' })).toBe(
      '10.0.0.1',
    )
  })

  it('uses UI host as hint when enrollment is loopback-only', () => {
    expect(
      formatPlatformHostLabel(
        { hostname: 'localhost', address: '127.0.0.1' },
        { fallbackAddress: '212.8.248.187' },
      ),
    ).toBe('212.8.248.187')
  })

  it('labels loopback enrollment without a usable fallback', () => {
    expect(formatPlatformHostLabel({ hostname: 'localhost', address: '127.0.0.1' })).toBe(
      'Local hypervisor',
    )
  })

  it('uses host identity when cluster name is default', () => {
    expect(
      formatFleetDisplayTitle(
        { name: 'default' },
        [{ hostname: 'localhost', address: '10.0.0.1', state: 'online' }],
      ),
    ).toBe('10.0.0.1')
  })

  it('keeps a custom cluster name', () => {
    expect(
      formatFleetDisplayTitle(
        { name: 'Production East' },
        [{ hostname: 'node-a', address: '10.0.0.1', state: 'online' }],
      ),
    ).toBe('Production East')
  })
})

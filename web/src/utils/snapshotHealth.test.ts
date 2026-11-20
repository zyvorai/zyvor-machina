// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, it, expect } from 'vitest'
import { snapshotStateSeverity } from './snapshotHealth'

describe('snapshotStateSeverity', () => {
  it('maps error → error', () => expect(snapshotStateSeverity('error')).toBe('error'))
  it('maps crashed → error', () => expect(snapshotStateSeverity('crashed')).toBe('error'))
  it('maps ERROR upper-case → error', () => expect(snapshotStateSeverity('ERROR')).toBe('error'))
  it('maps blocking → warn', () => expect(snapshotStateSeverity('blocking')).toBe('warn'))
  it('maps paused → warn', () => expect(snapshotStateSeverity('paused')).toBe('warn'))
  it('maps shutoff → info', () => expect(snapshotStateSeverity('shutoff')).toBe('info'))
  it('maps running → info', () => expect(snapshotStateSeverity('running')).toBe('info'))
  it('maps disk-snapshot → info', () => expect(snapshotStateSeverity('disk-snapshot')).toBe('info'))
  it('maps empty string → info', () => expect(snapshotStateSeverity('')).toBe('info'))
})

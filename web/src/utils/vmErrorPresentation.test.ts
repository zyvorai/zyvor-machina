// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { vmErrorPresentation } from './vmErrorPresentation'

describe('vmErrorPresentation', () => {
  it('hides stale already-running resume failures', () => {
    const msg =
      'status: Internal, message: "Operation failed: Failed to resume VM \'chrome-e2e-vm\': error: Requested operation is not valid: domain is already running [code=operationinvalid (55)]"'
    expect(vmErrorPresentation(msg)).toBeNull()
  })

  it('still surfaces real domain-missing errors', () => {
    const e = vmErrorPresentation('Domain not found: no domain with matching name')
    expect(e?.error_code).toBe('domain_missing')
  })
})

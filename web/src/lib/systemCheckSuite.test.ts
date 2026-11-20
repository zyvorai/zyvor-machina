// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import {
  formatCheckReportMarkdown,
  summarizeCheckResults,
  type CheckResult,
} from './systemCheckSuite'

function row(
  id: string,
  status: CheckResult['status'],
): CheckResult {
  return {
    id,
    category: 'api',
    label: id,
    status,
    message: status,
    durationMs: 1,
  }
}

describe('summarizeCheckResults', () => {
  it('counts pass warn fail skip', () => {
    const s = summarizeCheckResults([
      row('a', 'pass'),
      row('b', 'pass'),
      row('c', 'warn'),
      row('d', 'fail'),
      row('e', 'skip'),
    ])
    expect(s).toEqual({ pass: 2, warn: 1, fail: 1, skip: 1, overall: 'fail' })
  })

  it('overall warn when no fail', () => {
    const s = summarizeCheckResults([row('a', 'pass'), row('b', 'warn')])
    expect(s.overall).toBe('warn')
  })

  it('overall pass when all pass or skip', () => {
    const s = summarizeCheckResults([row('a', 'pass'), row('b', 'skip')])
    expect(s.overall).toBe('pass')
  })
})

describe('formatCheckReportMarkdown', () => {
  it('includes summary line', () => {
    const md = formatCheckReportMarkdown([row('health', 'pass')], {
      pass: 1,
      warn: 0,
      fail: 0,
      skip: 0,
      overall: 'pass',
    })
    expect(md).toContain('# Machina System Check')
    expect(md).toContain('1 passed')
    expect(md).toContain('**health**')
  })
})

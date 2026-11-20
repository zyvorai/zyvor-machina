// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import {
  formatHttpErrorBody,
  formatUserError,
  friendlyErrorCode,
  sanitizeErrorText,
  unwrapJsonEnvelope,
} from './apiError'

describe('sanitizeErrorText', () => {
  it('replaces HTML error pages with a short explanation', () => {
    const html = '<!DOCTYPE html><html><body>503</body></html>'
    const out = sanitizeErrorText(html)
    expect(out).not.toContain('</html>')
    expect(out.toLowerCase()).toContain('html')
  })
})

describe('formatHttpErrorBody', () => {
  it('maps error_code only JSON to a friendly label', () => {
    const body = JSON.stringify({ error_code: 'operation_failed' })
    expect(formatHttpErrorBody(500, 'Internal Server Error', body)).toBe(
      friendlyErrorCode('operation_failed'),
    )
  })

  it('includes message and code without duplicating bare code', () => {
    const body = JSON.stringify({
      error: 'Neutron unavailable',
      error_code: 'operation_failed',
    })
    const out = formatHttpErrorBody(503, 'Service Unavailable', body)
    expect(out).toContain('Neutron unavailable')
    expect(out).toContain(friendlyErrorCode('operation_failed'))
  })

  it('handles HTML responses from proxies', () => {
    const html = '<html><head><title>503</title></head><body>down</body></html>'
    const out = formatHttpErrorBody(503, 'Service Unavailable', html)
    expect(out).not.toContain('</html>')
    expect(out.toLowerCase()).toContain('html')
  })

  it('truncates very long plain text', () => {
    const long = 'x'.repeat(500)
    const out = formatHttpErrorBody(500, 'Error', long)
    expect(out.length).toBeLessThan(450)
    expect(out.endsWith('…')).toBe(true)
  })
})

describe('formatUserError', () => {
  it('sanitizes Error messages that embed HTML', () => {
    const err = new Error('</html> (operation_failed)')
    const out = formatUserError(err)
    expect(out).not.toContain('</html>')
  })
})

describe('unwrapJsonEnvelope / sanitizeErrorText envelope handling', () => {
  it('shows the message inside a {"error": …} envelope instead of the JSON', () => {
    expect(unwrapJsonEnvelope('{"error":"pool default is busy"}')).toBe('pool default is busy')
    expect(sanitizeErrorText('{"error":"pool default is busy"}')).toBe('pool default is busy')
  })
  it('accepts a {"message": …} envelope and trims it', () => {
    expect(unwrapJsonEnvelope('  {"message":"  nope  "}  ')).toBe('nope')
  })
  it('leaves plain text, non-envelope JSON and malformed JSON alone', () => {
    expect(unwrapJsonEnvelope('Bad gateway')).toBe('Bad gateway')
    expect(unwrapJsonEnvelope('{"code":7}')).toBe('{"code":7}')
    expect(unwrapJsonEnvelope('{not json')).toBe('{not json')
    expect(unwrapJsonEnvelope('{"error":""}')).toBe('{"error":""}')
  })
  it('formatUserError unwraps an Error whose message is a raw envelope', () => {
    expect(formatUserError(new Error('{"error":"host is in maintenance"}'))).toBe('host is in maintenance')
  })
})

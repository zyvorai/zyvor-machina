// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { parseVmGraphicsFromXml } from './vmGraphics'

describe('parseVmGraphicsFromXml', () => {
  it('detects vnc and spice blocks', () => {
    const xml = `
      <graphics type='vnc' port='-1' autoport='yes' listen='127.0.0.1'/>
      <graphics type='spice' port='-1' autoport='yes' listen='0.0.0.0'/>
    `
    expect(parseVmGraphicsFromXml(xml)).toEqual({
      vnc: { listen: '127.0.0.1' },
      spice: { listen: '0.0.0.0' },
    })
  })

  it('returns empty when no graphics', () => {
    expect(parseVmGraphicsFromXml('<domain></domain>')).toEqual({})
  })
})

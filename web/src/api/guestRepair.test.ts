// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { extractBackupPath, summariseDoctorOutput } from './guestRepair'

describe('summariseDoctorOutput', () => {
  it('reads a boot score and finding messages from GuestKit JSON', () => {
    const out = JSON.stringify({ boot_score: 42, blockers: [{ title: 'fstab references a missing disk' }], warnings: ['GRUB timeout is 0'] })
    const s = summariseDoctorOutput(out)
    expect(s.score).toBe(42)
    expect(s.findings).toEqual(['fstab references a missing disk', 'GRUB timeout is 0'])
  })
  it('finds JSON surrounded by log lines and clamps the score', () => {
    const s = summariseDoctorOutput('inspecting...\n{"score": 140, "issues": ["x"]}\ndone')
    expect(s.score).toBe(100)
    expect(s.findings).toEqual(['x'])
  })
  it('falls back to raw text when there is no JSON', () => {
    const s = summariseDoctorOutput('plain text report')
    expect(s.score).toBeNull()
    expect(s.findings).toEqual([])
    expect(s.raw).toBe('plain text report')
  })
  it('de-duplicates findings', () => {
    expect(summariseDoctorOutput(JSON.stringify({ blockers: ['a', 'a'], warnings: ['a'] })).findings).toEqual(['a'])
  })

  it('reads the real GuestKit doctor shape (score and findings under bootability)', () => {
    const out = JSON.stringify({
      target: 'kvm',
      bootability: {
        score: 94.7368,
        blockers: [],
        warnings: [{ title: 'GRUB configuration', message: 'No GRUB configuration detected' }],
      },
    })
    const s = summariseDoctorOutput(out)
    expect(s.score).toBe(95)
    expect(s.findings).toEqual(['GRUB configuration: No GRUB configuration detected'])
  })
})

describe('extractBackupPath', () => {
  it('finds the backup GuestKit created', () => {
    expect(extractBackupPath('Repair complete.\nBackup created: /var/lib/libvirt/images/x.backup_1.qcow2\nWarning: y')).toBe('/var/lib/libvirt/images/x.backup_1.qcow2')
  })
  it('returns null when there is none', () => {
    expect(extractBackupPath('Repair complete.')).toBeNull()
  })
})

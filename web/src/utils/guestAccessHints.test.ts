// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { aggregateAccessNoteLabels, consoleAccessHints } from './guestAccessHints'

describe('consoleAccessHints', () => {
  it('warns serial is ssh-key only', () => {
    const hints = consoleAccessHints(
      { auth_mode: 'ssh_key', serial_password_login: false, guest_ip_private: true },
      'serial',
      { guestIp: '192.168.122.199', sshUser: 'ubuntu' },
    )
    expect(hints.some((h) => h.includes('SSH-key only'))).toBe(true)
  })

  it('suggests port-forward ssh command when nat is exposed', () => {
    const hints = consoleAccessHints(
      { auth_mode: 'ssh_key', serial_password_login: false, guest_ip_private: true, ssh_nat_host_port: 2222 },
      'shell',
      { guestIp: '192.168.122.199', sshUser: 'ubuntu', hypervisorHost: 'lab.test' },
    )
    expect(hints.join(' ')).toContain('ssh -p 2222 ubuntu@lab.test')
  })

  it('aggregates access note labels for Cinema pill', () => {
    const labels = aggregateAccessNoteLabels(
      { auth_mode: 'ssh_key', serial_password_login: false, guest_ip_private: true },
      { guestIp: '192.168.122.199' },
    )
    expect(labels).toContain('SSH key-only')
    expect(labels).toContain('NAT guest IP')
    expect(labels).toContain('SSH not exposed')
  })

  describe('windows guests', () => {
    const winHints = {
      auth_mode: 'unknown',
      serial_password_login: true,
      guest_ip_private: true,
      ssh_nat_host_port: null,
    }

    it('offers RDP rather than an ssh command', () => {
      // The regression: a Windows guest was told to `ssh ubuntu@…`, which it
      // would refuse — it needs an RDP address for a native client instead.
      const msgs = consoleAccessHints(winHints, 'shell', {
        guestIp: '192.168.122.84',
        sshUser: 'ubuntu',
        hypervisorHost: 'lab.test',
        osFamily: 'windows',
      })
      const joined = msgs.join(' ')
      expect(joined).not.toContain('ssh -p')
      expect(joined).not.toContain('ubuntu@')
      expect(joined).toContain('Remote Desktop')
    })

    it('gives the dial address once RDP is exposed', () => {
      const msgs = consoleAccessHints(winHints, 'shell', {
        guestIp: '192.168.122.84',
        hypervisorHost: 'lab.test',
        osFamily: 'windows',
        rdpNatHostPort: 33890,
      })
      expect(msgs.join(' ')).toContain('lab.test:33890')
    })

    it('labels the pill for RDP, not SSH', () => {
      const labels = aggregateAccessNoteLabels(winHints, {
        guestIp: '192.168.122.84',
        osFamily: 'windows',
      })
      expect(labels).toContain('Windows guest')
      expect(labels).toContain('RDP not exposed')
      expect(labels).not.toContain('SSH not exposed')
    })

    it('puts the dial-able address in the collapsed pill once exposed', () => {
      const labels = aggregateAccessNoteLabels(winHints, {
        guestIp: '192.168.122.84',
        hypervisorHost: '212.8.248.187',
        osFamily: 'windows',
        rdpNatHostPort: 13389,
      })
      expect(labels).toContain('RDP 212.8.248.187:13389')
      expect(labels).not.toContain('RDP not exposed')
    })

    it('leaves linux guests on the ssh path', () => {
      const labels = aggregateAccessNoteLabels(
        { auth_mode: 'ssh_key', serial_password_login: false, guest_ip_private: true },
        { guestIp: '192.168.122.10', osFamily: 'linux' },
      )
      expect(labels).toContain('SSH not exposed')
      expect(labels).not.toContain('Windows guest')
    })
  })
})

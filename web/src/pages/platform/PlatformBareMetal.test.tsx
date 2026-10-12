// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import PlatformBareMetal from './PlatformBareMetal'
import type {
  BaremetalBmcInfo,
  BaremetalCatalogImage,
  BaremetalImage,
  BaremetalProfile,
  BaremetalServer,
} from '../../api/platform'

const toast = { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() }

vi.mock('../../api/platform', () => ({
  listBaremetalServers: vi.fn(),
  registerBaremetalServer: vi.fn(),
  baremetalServerPower: vi.fn(),
  commissionBaremetalServer: vi.fn(),
  releaseBaremetalServer: vi.fn(),
  setBaremetalLifecycle: vi.fn(),
  listBaremetalImages: vi.fn(),
  importBaremetalImage: vi.fn(),
  deleteBaremetalImage: vi.fn(),
  listBaremetalProfiles: vi.fn(),
  provisionBaremetal: vi.fn(),
  refreshBaremetalBmcInfo: vi.fn(),
}))
vi.mock('../../contexts/ToastContext', () => ({ useToastContext: () => toast }))
import * as api from '../../api/platform'

const server = (o: Partial<BaremetalServer> = {}): BaremetalServer => ({
  id: 's1',
  hostname: 'rack2-u14',
  bmc_address: '10.20.0.14',
  bmc_type: 'redfish',
  state: 'powered_off',
  cpu_cores: 32,
  memory_mib: 262144,
  firewall_profile: 'BareMetalBmc',
  firewall_enabled: true,
  bmc_vlan: '',
  pxe_vlan: '',
  created_at: '2026-10-12T00:00:00Z',
  bmc_credentials_configured: true,
  last_error: '',
  lifecycle: 'new',
  zone: 'rack2',
  mac_address: 'aa:bb:cc:dd:ee:ff',
  ...o,
})

const image = (o: Partial<BaremetalImage> = {}): BaremetalImage => ({
  id: 'i1',
  name: 'ubuntu-26.04',
  os: 'ubuntu',
  arch: 'amd64',
  format: 'qcow2',
  size_bytes: 865115136,
  checksum: 'abc',
  checksum_algo: 'sha256',
  source_url: 'https://example/u.img',
  status: 'ready',
  error: '',
  created_at: '2026-10-12T00:00:00Z',
  ...o,
})

const catalog: BaremetalCatalogImage[] = [
  { name: 'ubuntu-26.04', os: 'ubuntu', arch: 'amd64', format: 'qcow2', url: 'u', checksum_algo: 'sha256' },
  { name: 'debian-13', os: 'debian', arch: 'amd64', format: 'qcow2', url: 'd', checksum_algo: 'sha512' },
]

const profile = (o: Partial<BaremetalProfile> = {}): BaremetalProfile => ({
  name: 'web',
  image: 'ubuntu-26.04',
  ssh_keys: '[]',
  timezone: 'UTC',
  zone: 'rack2',
  min_cpu_cores: 16,
  min_memory_mib: 0,
  auto_deploy: false,
  desired_count: 0,
  ...o,
})

function stub(opts: {
  servers?: BaremetalServer[]
  images?: BaremetalImage[]
  profiles?: BaremetalProfile[]
} = {}) {
  vi.mocked(api.listBaremetalServers).mockResolvedValue(opts.servers ?? [server()])
  vi.mocked(api.listBaremetalImages).mockResolvedValue({ images: opts.images ?? [], catalog })
  vi.mocked(api.listBaremetalProfiles).mockResolvedValue(opts.profiles ?? [])
}

const view = () => render(<MemoryRouter><PlatformBareMetal /></MemoryRouter>)

beforeEach(() => stub())
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('PlatformBareMetal', () => {
  it('shows lifecycle and power next to each server, with the last BMC error', async () => {
    stub({ servers: [server({ lifecycle: 'failed', last_error: 'boot override: HTTP 400' })] })
    view()
    await waitFor(() => expect(screen.getByText('rack2-u14')).toBeTruthy())
    expect(screen.getByText('failed')).toBeTruthy()
    expect(screen.getByText('powered_off')).toBeTruthy()
    expect(screen.getByText(/last error: boot override: HTTP 400/)).toBeTruthy()
    expect(screen.getByText(/zone rack2/)).toBeTruthy()
  })

  it('offers Commission for a new machine and calls the API', async () => {
    vi.mocked(api.commissionBaremetalServer).mockResolvedValue({
      id: 's1', lifecycle: 'commissioning', token: 't', report_path: '/r',
    })
    view()
    fireEvent.click(await screen.findByRole('button', { name: 'Commission' }))
    await waitFor(() => expect(api.commissionBaremetalServer).toHaveBeenCalledWith('s1'))
    expect(toast.success).toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: 'Release' })).toBeNull()
  })

  it('offers Release (not Commission) for an allocated machine', async () => {
    stub({ servers: [server({ lifecycle: 'allocated' })] })
    vi.mocked(api.releaseBaremetalServer).mockResolvedValue({ id: 's1', lifecycle: 'ready' })
    view()
    fireEvent.click(await screen.findByRole('button', { name: 'Release' }))
    await waitFor(() => expect(api.releaseBaremetalServer).toHaveBeenCalledWith('s1'))
    expect(screen.queryByRole('button', { name: 'Commission' })).toBeNull()
  })

  it('imports a catalog image the controller does not have yet', async () => {
    stub({ images: [image()] })
    vi.mocked(api.importBaremetalImage).mockResolvedValue(image({ name: 'debian-13', status: 'downloading' }))
    view()
    const select = (await screen.findByLabelText('Catalog image')) as HTMLSelectElement
    // ubuntu-26.04 is already imported and ready, so only debian-13 is offered.
    const offered = Array.from(select.options).map((o) => o.value).filter(Boolean)
    expect(offered).toEqual(['debian-13'])
    fireEvent.change(select, { target: { value: 'debian-13' } })
    fireEvent.click(screen.getByRole('button', { name: 'Import' }))
    await waitFor(() => expect(api.importBaremetalImage).toHaveBeenCalledWith({ name: 'debian-13' }))
  })

  it('shows a failed image with its reason, and offers it for import again', async () => {
    stub({ images: [image({ status: 'failed', error: 'sha256 mismatch', checksum: '' })] })
    view()
    await waitFor(() => expect(screen.getByText(/sha256 mismatch/)).toBeTruthy())
    const select = screen.getByLabelText('Catalog image') as HTMLSelectElement
    expect(Array.from(select.options).map((o) => o.value)).toContain('ubuntu-26.04')
  })

  it('provisions N machines from a profile and reports a shortage', async () => {
    stub({ profiles: [profile()] })
    vi.mocked(api.provisionBaremetal).mockResolvedValue({
      requested: 3,
      started: [{ server_id: 'a', hostname: 'm1', status: 'deploying' }],
      stopped_because: 'no ready server matches the request',
    })
    view()
    const count = (await screen.findByLabelText('Machines to provision for web')) as HTMLInputElement
    fireEvent.change(count, { target: { value: '3' } })
    fireEvent.click(screen.getByRole('button', { name: 'Provision' }))
    await waitFor(() => expect(api.provisionBaremetal).toHaveBeenCalledWith('web', 3))
    await waitFor(() => expect(toast.warning).toHaveBeenCalled())
    expect(String(toast.warning.mock.calls[0][0])).toContain('1 of 3')
    expect(String(toast.warning.mock.calls[0][0])).toContain('no ready server')
  })

  it('hides the profiles panel when there are none', async () => {
    view()
    await screen.findByText('rack2-u14')
    expect(screen.queryByText('Deploy profiles')).toBeNull()
  })

  it('opens BMC details for a Redfish server and lists what the BMC did not report', async () => {
    const info: BaremetalBmcInfo = {
      identity: {
        manufacturer: 'Dell Inc.', model: 'PowerEdge R760', serial_number: 'ABC123', sku: '',
        bios_version: '2.1.0', cpu_count: 2, cpu_model: 'Xeon Gold', memory_gib: 512,
        power_state: 'On', health: 'Warning',
      },
      temperatures: [{ name: 'Inlet', celsius: 24, health: 'OK' }],
      power_consumed_watts: 312,
      firmware: [{ name: 'BIOS', version: '2.1.0' }],
      log: [{ created: '2026-10-12T00:00:00Z', severity: 'Warning', message: 'temp high' }],
      unavailable: ['power'],
    }
    vi.mocked(api.refreshBaremetalBmcInfo).mockResolvedValue(info)
    view()
    fireEvent.click(await screen.findByRole('button', { name: 'BMC details' }))
    await waitFor(() => expect(api.refreshBaremetalBmcInfo).toHaveBeenCalledWith('s1'))
    const panel = (await screen.findByText(/BMC details: rack2-u14/)).closest('div') as HTMLElement
    expect(panel).toBeTruthy()
    expect(screen.getByText(/Dell Inc\. PowerEdge R760/)).toBeTruthy()
    expect(screen.getByText(/health Warning/)).toBeTruthy()
    expect(screen.getByText(/Inlet 24°C/)).toBeTruthy()
    expect(screen.getByText(/Not reported by this BMC: power/)).toBeTruthy()
  })

  it('does not offer BMC details without stored credentials or for IPMI', async () => {
    stub({
      servers: [
        server({ id: 'a', hostname: 'no-creds', bmc_credentials_configured: false }),
        server({ id: 'b', hostname: 'ipmi-box', bmc_type: 'ipmi' }),
      ],
    })
    view()
    await screen.findByText('no-creds')
    expect(screen.queryByRole('button', { name: 'BMC details' })).toBeNull()
  })

  it('registers a server with credentials, MAC and zone', async () => {
    vi.mocked(api.registerBaremetalServer).mockResolvedValue(server())
    view()
    fireEvent.click(await screen.findByRole('button', { name: /Register Server/ }))
    fireEvent.change(screen.getByLabelText('Hostname'), { target: { value: 'metal-01' } })
    fireEvent.change(screen.getByLabelText('BMC Address'), { target: { value: '10.0.0.9' } })
    fireEvent.change(screen.getByLabelText('BMC Username'), { target: { value: 'admin' } })
    fireEvent.change(screen.getByLabelText('BMC Password'), { target: { value: 'pw' } })
    fireEvent.change(screen.getByLabelText('PXE MAC address'), { target: { value: 'aa:bb:cc:00:00:01' } })
    fireEvent.change(screen.getByLabelText('Zone'), { target: { value: 'rack3' } })
    const form = screen.getByLabelText('Hostname').closest('div')!.parentElement!.parentElement as HTMLElement
    fireEvent.click(within(form.parentElement as HTMLElement).getByRole('button', { name: 'Register' }))
    await waitFor(() =>
      expect(api.registerBaremetalServer).toHaveBeenCalledWith({
        hostname: 'metal-01',
        bmc_address: '10.0.0.9',
        bmc_type: 'redfish',
        bmc_username: 'admin',
        bmc_password: 'pw',
        mac_address: 'aa:bb:cc:00:00:01',
        zone: 'rack3',
      }),
    )
  })

  it('does not offer the removed iLO REST BMC type (the controller refuses it)', async () => {
    view()
    fireEvent.click(await screen.findByRole('button', { name: /Register Server/ }))
    const select = screen.getByLabelText('BMC Type') as HTMLSelectElement
    expect(Array.from(select.options).map((o) => o.value)).toEqual(['redfish', 'ipmi', 'fake'])
  })
})

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'

vi.mock('../../../api/platform', () => ({
  createEnrollmentToken: vi.fn(),
  getJoinProgress: vi.fn(),
  listPlatformHosts: vi.fn(),
  getPlatformHostDetail: vi.fn(),
  enqueueValidateHost: vi.fn(),
  listEnrollmentTokens: vi.fn(),
  revokeEnrollmentToken: vi.fn(),
}))
vi.mock('../../../contexts/ToastContext', () => ({ useToastContext: () => ({ success: vi.fn(), error: vi.fn(), warning: vi.fn() }) }))
import {
  createEnrollmentToken, enqueueValidateHost, getJoinProgress, getPlatformHostDetail, listEnrollmentTokens, listPlatformHosts, revokeEnrollmentToken,
} from '../../../api/platform'
import AddMachinePanel from './AddMachinePanel'
import EnrollTokensTable from './EnrollTokensTable'
import HostEnrollWizard from '../HostEnrollWizard'
import PlatformEnroll from '../../../pages/platform/PlatformEnroll'

const JOIN = 'curl -fsSk https://ctl:5094/install.sh -o /tmp/i.sh && sudo bash /tmp/i.sh --controller https://ctl:5094 --ca-sha256 AB --token join-abc'
const token = (over = {}) => ({ token: 'join-abc', expires_at: '2099-01-01 00:00:00', install_command: 'curl http://127.0.0.1:5093/install.sh | sudo bash', join_command: JOIN, ...over })
const ev = (id: string, step: string, level: string, message: string, t = '01.000') => ({ id, step, level, message, created_at: `2026-10-07 10:00:${t}` })
const host = { id: 'h1', hostname: 'node-9', address: '10.0.0.9', state: 'online', validation_status: 'passed', vm_count: 0, cpu_percent: 0 }

beforeEach(() => {
  vi.mocked(listPlatformHosts).mockResolvedValue([] as never)
  vi.mocked(listEnrollmentTokens).mockResolvedValue([])
  vi.mocked(createEnrollmentToken).mockResolvedValue(token())
  vi.mocked(getJoinProgress).mockResolvedValue({ token_status: 'waiting', events: [ev('1', 'token', 'info', 'enrollment token issued')] })
})
afterEach(() => { cleanup(); vi.clearAllMocks() })

const panel = () => render(<MemoryRouter><AddMachinePanel /></MemoryRouter>)

describe('AddMachinePanel', () => {
  it('creates a token on open and shows the join command with a countdown (waiting state)', async () => {
    panel()
    await waitFor(() => expect(screen.getByTestId('join-command').textContent).toBe(JOIN))
    expect(createEnrollmentToken).toHaveBeenCalledTimes(1)
    expect(screen.getByTestId('token-countdown')).toBeTruthy()
    expect(screen.queryByTestId('listener-off-banner')).toBeNull()
    await waitFor(() => expect(screen.getByTestId('join-live-panel')).toBeTruthy())
    expect(screen.queryByTestId('join-success')).toBeNull()
    expect(screen.queryByTestId('join-failed')).toBeNull()
  })

  it('shows a banner naming the setting when the HTTPS join listener is off', async () => {
    vi.mocked(createEnrollmentToken).mockResolvedValue(token({ join_command: undefined }))
    panel()
    const b = await screen.findByTestId('listener-off-banner')
    expect(b.textContent).toContain('MACHINA_CONTROLLER_TLS_ADDR')
    expect(screen.getByTestId('join-command').textContent).toContain('127.0.0.1:5093')
    expect(screen.getByText('Local-only command')).toBeTruthy()
  })

  it('shows success with Open host and Add another once the host is validated and online', async () => {
    vi.mocked(getJoinProgress).mockResolvedValue({
      token_status: 'joined', host,
      events: [ev('1', 'token', 'info', 'issued'), ev('2', 'contact', 'info', 'node-9 contacted'), ev('3', 'register', 'ok', 'host record created'), ev('4', 'validate', 'ok', 'validation passed', '08.000')],
    })
    panel()
    const ok = await screen.findByTestId('join-success')
    expect(ok.textContent).toContain('node-9 joined')
    expect(screen.getByText('Open host').getAttribute('href')).toBe('/platform/hosts/h1')
    fireEvent.click(screen.getByText('Add another'))
    await waitFor(() => expect(createEnrollmentToken).toHaveBeenCalledTimes(2))
  })

  it('shows the failing check with its fix and re-checks the host', async () => {
    vi.mocked(getJoinProgress).mockResolvedValue({
      token_status: 'joined', host: { ...host, state: 'provisioning', validation_status: 'failed' },
      events: [ev('1', 'token', 'info', 'issued'), ev('9', 'contact', 'info', 'node-9 contacted'), ev('2', 'register', 'ok', 'created'), ev('3', 'check', 'error', 'agent_reachable: no route'), ev('4', 'validate', 'error', 'validation failed')],
    })
    vi.mocked(getPlatformHostDetail).mockResolvedValue({ ...host, validation_report: [{ name: 'agent_reachable', passed: false, message: 'Cannot connect to agent', remediation: 'Open port 50051 to the controller' }] } as never)
    vi.mocked(enqueueValidateHost).mockResolvedValue({ task_id: 't' })
    panel()
    const f = await screen.findByTestId('join-failed')
    await waitFor(() => expect(f.textContent).toContain('Open port 50051 to the controller'))
    fireEvent.click(screen.getByText('Re-check'))
    await waitFor(() => expect(enqueueValidateHost).toHaveBeenCalledWith('h1'))
  })

  it('offers Ansible, cloud-init and Terraform snippets built from the same command', async () => {
    panel()
    await screen.findByTestId('join-command')
    fireEvent.click(screen.getByRole('tab', { name: 'Automation' }))
    const tab = screen.getByTestId('automation-tab')
    expect(tab.textContent).toContain('deploy/ansible/site.yml')
    expect(tab.textContent).toContain('--token join-abc')
    expect(tab.textContent).toContain('deploy/terraform/join-nodes')
  })

  it('switches the operating-system note', async () => {
    panel()
    await screen.findByTestId('join-command')
    expect(screen.getByText(/apt-get/)).toBeTruthy()
    fireEvent.click(screen.getByRole('tab', { name: 'RHEL family' }))
    expect(screen.getByText(/dnf/)).toBeTruthy()
  })
})

describe('EnrollTokensTable', () => {
  const now = Date.parse('2026-10-07T12:00:00Z')
  it('shows open, used and expired tokens; only open ones can be revoked', () => {
    const onRevoke = vi.fn()
    render(<EnrollTokensTable now={now} onRevoke={onRevoke} rows={[
      { token: 'join-open-0000000', created_at: '2026-10-07 11:00:00', expires_at: '2026-10-08 11:00:00', used_at: null },
      { token: 'join-used-0000000', created_at: '2026-10-07 09:00:00', expires_at: '2026-10-08 09:00:00', used_at: '2026-10-07 09:05:00' },
      { token: 'join-old-00000000', created_at: '2026-10-06 09:00:00', expires_at: '2026-10-06 10:00:00', used_at: null },
    ]} />)
    const states = [...document.querySelectorAll('[data-token-state]')].map((r) => r.getAttribute('data-token-state'))
    expect(states).toEqual(['open', 'used', 'expired'])
    expect(screen.getAllByText('Revoke')).toHaveLength(1)
    fireEvent.click(screen.getByText('Revoke'))
    expect(onRevoke).toHaveBeenCalledWith('join-open-0000000')
  })
  it('explains single use when there are no tokens', () => {
    render(<MemoryRouter><EnrollTokensTable rows={[]} onRevoke={() => {}} /></MemoryRouter>)
    expect(screen.getByText(/one machine, once/)).toBeTruthy()
  })
})

describe('shells', () => {
  it('the wizard renders the same panel', async () => {
    render(<MemoryRouter><HostEnrollWizard open onClose={() => {}} /></MemoryRouter>)
    await waitFor(() => expect(screen.getByTestId('add-machine-panel')).toBeTruthy())
    await waitFor(() => expect(screen.getByTestId('join-command').textContent).toBe(JOIN))
  })
  it('the Enroll page renders the panel and the tokens table, and revokes through the API', async () => {
    vi.mocked(listEnrollmentTokens).mockResolvedValue([{ token: 'join-open-0000000', created_at: '2026-10-07 11:00:00', expires_at: '2099-01-01 00:00:00', used_at: null }])
    vi.mocked(revokeEnrollmentToken).mockResolvedValue({ revoked: true })
    render(<MemoryRouter><PlatformEnroll /></MemoryRouter>)
    await waitFor(() => expect(screen.getByTestId('add-machine-panel')).toBeTruthy())
    await waitFor(() => expect(screen.getByTestId('enroll-tokens-table')).toBeTruthy())
    fireEvent.click(screen.getAllByText('Revoke')[0])
    await waitFor(() => expect(revokeEnrollmentToken).toHaveBeenCalledWith('join-open-0000000'))
  })
})

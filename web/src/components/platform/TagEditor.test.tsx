// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import TagEditor from './TagEditor'

const api = vi.hoisted(() => ({ getTags: vi.fn(), putTags: vi.fn(), deleteTags: vi.fn() }))
vi.mock('../../api/tags', async () => {
  const actual = await vi.importActual<typeof import('../../api/tags')>('../../api/tags')
  return { ...actual, ...api }
})

afterEach(cleanup)
beforeEach(() => {
  vi.resetAllMocks()
  api.getTags.mockResolvedValue({ resource_type: 'vm', id: 'u', ec2_id: 'i-0123456789abcdef0', tags: { env: 'prod', team: 'payments' } })
})

it('lists tags with the EC2-style id and removes one', async () => {
  api.deleteTags.mockResolvedValue({ tags: { team: 'payments' } })
  render(<TagEditor resourceType="vm" resourceId="u" />)
  expect(await screen.findByText('i-0123456789abcdef0')).toBeTruthy()
  expect(screen.getByText('env')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Remove tag env' }))
  await waitFor(() => expect(api.deleteTags).toHaveBeenCalledWith('vm', 'u', ['env']))
  await waitFor(() => expect(screen.queryByText('env')).toBeNull())
})

it('adds a tag and clears the form', async () => {
  api.putTags.mockResolvedValue({ tags: { env: 'prod', team: 'payments', owner: 'sam' } })
  render(<TagEditor resourceType="vm" resourceId="u" />)
  await screen.findByText('env')
  fireEvent.change(screen.getByLabelText('Tag key'), { target: { value: 'owner' } })
  fireEvent.change(screen.getByLabelText('Tag value'), { target: { value: 'sam' } })
  fireEvent.click(screen.getByRole('button', { name: 'Add tag' }))
  await waitFor(() => expect(api.putTags).toHaveBeenCalledWith('vm', 'u', { owner: 'sam' }))
  expect(await screen.findByText('owner')).toBeTruthy()
  await waitFor(() => expect((screen.getByLabelText('Tag key') as HTMLInputElement).value).toBe(''))
})

it('refuses a reserved or malformed key before calling the server', async () => {
  render(<TagEditor resourceType="vm" resourceId="u" />)
  await screen.findByText('env')
  fireEvent.change(screen.getByLabelText('Tag key'), { target: { value: 'aws:cost' } })
  expect(screen.getByRole('alert').textContent).toContain('reserved')
  expect((screen.getByRole('button', { name: 'Add tag' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.change(screen.getByLabelText('Tag key'), { target: { value: 'bad;key' } })
  expect(screen.getByRole('alert').textContent).toContain('letters')
  expect(api.putTags).not.toHaveBeenCalled()
})

it('stops at 50 tags but still lets an existing key be overwritten', async () => {
  const fifty = Object.fromEntries(Array.from({ length: 50 }, (_, i) => [`k${i}`, 'v']))
  api.getTags.mockResolvedValue({ resource_type: 'vm', id: 'u', ec2_id: 'i-0123456789abcdef0', tags: fifty })
  render(<TagEditor resourceType="vm" resourceId="u" />)
  await screen.findByText('k0')
  fireEvent.change(screen.getByLabelText('Tag key'), { target: { value: 'extra' } })
  expect(screen.getByText(/at most 50 tags/)).toBeTruthy()
  expect((screen.getByRole('button', { name: 'Add tag' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.change(screen.getByLabelText('Tag key'), { target: { value: 'k0' } })
  expect((screen.getByRole('button', { name: 'Add tag' }) as HTMLButtonElement).disabled).toBe(false)
})

it('renders nothing on a controller without the tags API', async () => {
  api.getTags.mockRejectedValue(new Error('HTTP 404: not found'))
  const { container } = render(<TagEditor resourceType="vm" resourceId="u" />)
  await waitFor(() => expect(api.getTags).toHaveBeenCalled())
  await waitFor(() => expect(container.querySelector('[data-testid="tag-editor"]')).toBeNull())
})

it('read-only mode shows tags without controls', async () => {
  render(<TagEditor resourceType="vm" resourceId="u" readOnly />)
  await screen.findByText('env')
  expect(screen.queryByLabelText('Tag key')).toBeNull()
  expect(screen.queryByRole('button', { name: /Remove tag/ })).toBeNull()
})

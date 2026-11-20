// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import ImageSharing from './ImageSharing'

const api = vi.hoisted(() => ({
  listImageShares: vi.fn(), setImageVisibility: vi.fn(), shareImage: vi.fn(), unshareImage: vi.fn(),
}))
vi.mock('../../api/nativeTemplates', () => api)

const image = (over = {}) => ({ name: 'ubuntu', version: '24.04', visibility: 'private' as const, project: 'core', ...over })

afterEach(cleanup)
beforeEach(() => { vi.resetAllMocks(); api.listImageShares.mockResolvedValue(['lab']) })

it('lists the projects a private image is shared with and removes one', async () => {
  api.unshareImage.mockResolvedValue(undefined)
  render(<ImageSharing image={image()} />)
  expect(await screen.findByText('lab')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Remove' }))
  await waitFor(() => expect(api.unshareImage).toHaveBeenCalledWith('ubuntu', '24.04', 'lab'))
})

it('shares with a typed project and hides the share list for public images', async () => {
  api.shareImage.mockResolvedValue(undefined)
  const { unmount } = render(<ImageSharing image={image()} />)
  fireEvent.change(await screen.findByLabelText('Project to share with'), { target: { value: ' ops ' } })
  fireEvent.click(screen.getByRole('button', { name: 'Share' }))
  await waitFor(() => expect(api.shareImage).toHaveBeenCalledWith('ubuntu', '24.04', 'ops'))
  unmount()
  render(<ImageSharing image={image({ visibility: 'public' })} />)
  expect(screen.queryByLabelText('Project to share with')).toBeNull()
})

it('switches visibility through the API', async () => {
  api.setImageVisibility.mockResolvedValue({})
  render(<ImageSharing image={image({ visibility: 'public' })} />)
  fireEvent.change(screen.getByLabelText('Image visibility'), { target: { value: 'private' } })
  await waitFor(() => expect(api.setImageVisibility).toHaveBeenCalledWith('ubuntu', '24.04', 'private'))
  expect(await screen.findByLabelText('Project to share with')).toBeTruthy()
})

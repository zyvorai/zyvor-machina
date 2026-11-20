// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import DatabaseStatus from './DatabaseStatus'

const api = vi.hoisted(() => ({ apiGet: vi.fn() }))
vi.mock('../../api/client', () => api)

afterEach(cleanup)
beforeEach(() => vi.resetAllMocks())

it('names the SQLite backend and says it is answering', async () => {
  api.apiGet.mockResolvedValue({ database: 'ok', database_backend: 'sqlite', database_pool: { size: 2, idle: 2 } })
  render(<DatabaseStatus />)
  expect(await screen.findByText('SQLite (embedded)')).toBeTruthy()
  expect(screen.getByText('answering')).toBeTruthy()
  expect(screen.getByText(/connections 0 in use of 2/)).toBeTruthy()
})

it('shows PostgreSQL and a database that is not answering', async () => {
  api.apiGet.mockResolvedValue({ database: 'unavailable', database_backend: 'postgres' })
  render(<DatabaseStatus />)
  expect(await screen.findByText('PostgreSQL')).toBeTruthy()
  expect(screen.getByText('not answering')).toBeTruthy()
})

it('treats a controller that does not report the backend as the embedded default', async () => {
  api.apiGet.mockResolvedValue({ database: 'ok' })
  render(<DatabaseStatus />)
  expect(await screen.findByText('SQLite (embedded)')).toBeTruthy()
})

it('says so when the controller cannot be read', async () => {
  api.apiGet.mockRejectedValue(new Error('boom'))
  render(<DatabaseStatus />)
  expect(await screen.findByText(/Could not read the controller's database status/)).toBeTruthy()
})

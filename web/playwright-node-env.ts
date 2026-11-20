// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Applied before Playwright loads test workers (Node 26+ DEP0205 until @playwright/test ≥1.61).

const DEP0205 = '--disable-warning=DEP0205'

if (!process.env.NODE_OPTIONS?.includes(DEP0205)) {
  process.env.NODE_OPTIONS = [process.env.NODE_OPTIONS, DEP0205].filter(Boolean).join(' ')
}

// Avoid NO_COLOR vs FORCE_COLOR noise in Playwright workers.
if (process.env.FORCE_COLOR) {
  delete process.env.NO_COLOR
}

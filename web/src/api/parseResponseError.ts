// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { formatHttpErrorBody } from '../utils/apiError'

/** Build a thrown Error from a non-OK fetch Response (JSON, HTML, or plain text). */
export async function parseResponseError(res: Response): Promise<Error> {
  const text = await res.text().catch(() => '')
  return new Error(formatHttpErrorBody(res.status, res.statusText, text))
}

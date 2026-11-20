// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { deleteVM, type VmDeleteUndefineOpts } from '../api/vm'
import { formatUserError } from '../utils/apiError'

/**
 * Deletes a VM via the API. If libvirt rejects undefine without NVRAM (same as
 * `virsh undefine --nvram`), retries once with `undefine_nvram: true`.
 * Other undefine flags are preserved on retry.
 *
 * @param onBeforeNvramRetry — Optional hook (e.g. UI state + toast) before the retry request.
 */
export async function deleteVmWithNvramRetry(
  name: string,
  opts?: VmDeleteUndefineOpts,
  onBeforeNvramRetry?: (merged: VmDeleteUndefineOpts) => void,
  connection?: string | null,
): Promise<void> {
  try {
    await deleteVM(name, opts, connection)
  } catch (e: unknown) {
    const msg = formatUserError(e)
    const m = msg.toLowerCase()
    // Libvirt: "cannot undefine domain with nvram" — require both tokens to avoid unrelated "nvram" text.
    const looksLikeNvramUndefineConflict =
      m.includes('nvram') && (m.includes('undefine') || m.includes('cannot remove domain'))
    if (looksLikeNvramUndefineConflict && !opts?.undefine_nvram) {
      const merged: VmDeleteUndefineOpts = { ...opts, undefine_nvram: true }
      onBeforeNvramRetry?.(merged)
      await deleteVM(name, merged, connection)
      return
    }
    throw e
  }
}

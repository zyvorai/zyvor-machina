// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'

/** Shared footer for Fleet Cloud pages: disk migration links. */
export default function FleetCloudFooter() {
  return (
    <footer className="mt-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-fill-tertiary)] px-4 py-3 text-xs text-[var(--text-muted)] space-y-2">
      <p>
        Push qcow2 from{' '}
        <Link to="/disk-images" className="text-[var(--link)] hover:underline">Disk images</Link>
        . Import exported disks via{' '}
        <Link to="/import" className="text-[var(--link)] hover:underline">Import VM</Link>.
      </p>
    </footer>
  )
}

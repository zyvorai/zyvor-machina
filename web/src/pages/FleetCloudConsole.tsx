// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Navigate, useParams } from 'react-router'

// Native "instance" console — reuses Machina's existing VNC/SPICE/serial console
// system directly (same viewer every other VM in the app already uses) instead of
// the daemon's old external-cloud-client remote-console URL flow. No new backend needed:
// a Fleet Cloud instance IS a Machina VM, so its console works exactly the same way.
export default function FleetCloudConsolePage() {
  const { id } = useParams<{ id: string }>()
  if (!id) {
    return (
      <div className="p-8 text-center text-[var(--text-muted)]">
        Missing instance id.
      </div>
    )
  }
  return <Navigate to={`/platform/vms/${encodeURIComponent(id)}/console`} replace />
}

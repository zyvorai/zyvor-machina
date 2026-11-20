// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Actionable hints for libvirt / VM API errors shown in ErrorBanner. */
export function libvirtErrorHints(error: string | undefined): string[] {
  const e = (error || '').toLowerCase()
  const hints: string[] = []
  if (e.includes('libvirt_connection') || e.includes('connect to libvirt') || e.includes('connection refused')) {
    hints.push('Ensure libvirtd is running: systemctl status libvirtd')
    hints.push('Check URI in Settings — qemu:///system vs qemu:///session')
  }
  if (e.includes('not_found') || e.includes('not found')) {
    hints.push('The VM or resource may have been deleted outside Machina — refresh the page.')
  }
  if (e.includes('forbidden') || e.includes('permission')) {
    hints.push('Your role may not allow this action — admin is required for destructive operations.')
  }
  if (e.includes('network error') || e.includes('cannot reach the machina api')) {
    hints.push('Open the UI from the same host/port as machina-daemon and verify the service is running.')
  }
  if (hints.length === 0) {
    hints.push('Check machina-daemon logs: journalctl -u machina-daemon -n 50')
    hints.push('Verify the VM state with virsh list --all on the host.')
  }
  return hints
}

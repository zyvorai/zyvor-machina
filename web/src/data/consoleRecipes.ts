// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export type ConsoleRecipe = {
  id: string
  title: string
  summary: string
  steps: string[]
  lens?: string
}

export const CONSOLE_RECIPES: ConsoleRecipe[] = [
  {
    id: 'no-ip',
    title: 'VM has no IP',
    summary: 'Guest network not ready or misconfigured.',
    steps: [
      'Check Kubernetes / platform events for the VM',
      'Open Serial and verify interface inside guest',
      'Review cloud-init network config',
      'Check CNI route on the node',
      'Check eBPF flows and process events for the VM (Security tab)',
    ],
    lens: 'network',
  },
  {
    id: 'boot-stuck',
    title: 'Boot stuck',
    summary: 'Guest may be waiting at bootloader or kernel panic.',
    steps: [
      'Open Serial console',
      'Check cloud-init / journal logs',
      'Inspect disk offline with GuestKit if stopped',
      'Verify root disk and virtio drivers',
      'Rollback to last snapshot if needed',
    ],
    lens: 'serial',
  },
  {
    id: 'black-screen',
    title: 'Black screen',
    summary: 'Display not initialized or VNC connected too early.',
    steps: [
      'Wait 30s and reconnect Display',
      'Try Serial for boot output',
      'Verify VM is running (not paused)',
      'Check firmware mode (BIOS vs UEFI)',
      'Ask Zyra to explain the screen',
    ],
    lens: 'display',
  },
  {
    id: 'guest-agent-missing',
    title: 'Guest agent missing',
    summary: 'QGA / guestkit-agent not reporting.',
    steps: [
      'Confirm virtio channel is attached',
      'Install guestkit-agent inside guest',
      'Use GuestKit offline doctor if VM is stopped',
      'Check firewall inside guest',
    ],
    lens: 'ai',
  },
  {
    id: 'windows-install',
    title: 'Windows install',
    summary: 'VirtIO drivers may be required during setup.',
    steps: [
      'Use Display lens at native resolution',
      'Mount VirtIO ISO if drivers missing',
      'Send Ctrl+Alt+Del when prompted',
      'Switch to RDP after setup completes',
    ],
    lens: 'display',
  },
  {
    id: 'ssh-unreachable',
    title: 'SSH unreachable',
    summary: 'Port 22 not responding from host.',
    steps: [
      'Open Display or Serial',
      'Verify guest IP and sshd running',
      'Check Zeus firewall rules',
      'Trace path with Network lens',
    ],
    lens: 'network',
  },
]

export function recipeForError(error: string | null): ConsoleRecipe | null {
  if (!error) return null
  const e = error.toLowerCase()
  if (e.includes('no ip') || e.includes('network')) return CONSOLE_RECIPES.find((r) => r.id === 'no-ip') ?? null
  if (e.includes('black') || e.includes('disconnected')) return CONSOLE_RECIPES.find((r) => r.id === 'black-screen') ?? null
  if (e.includes('boot') || e.includes('kernel')) return CONSOLE_RECIPES.find((r) => r.id === 'boot-stuck') ?? null
  if (e.includes('ssh')) return CONSOLE_RECIPES.find((r) => r.id === 'ssh-unreachable') ?? null
  if (e.includes('grpc') || e.includes('500') || e.includes('internal')) return null
  return null
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Derive UX timeline phases from streamed host logs (workflow step bars).
 */

export const VM_CREATE_TIMELINE_LABELS = [
  'Prepare disk image',
  'Define VM on host',
  'virt-install & console',
  'Guest created',
] as const

export const GOLDEN_FORGE_TIMELINE_LABELS = [
  'Job accepted',
  'Build (Packer / QEMU)',
  'Write golden image',
  'Ready',
] as const

export const VIRT_IMAGE_TIMELINE_LABELS = ['Queued', 'Build disk image', 'Finished'] as const

interface TimelineState {
  /** Step index that is in progress (0..labels.length-1), or last index when failed on last step */
  activeIndex: number
  allComplete: boolean
  failed: boolean
}

function vmCreateFailedPhase(text: string): number {
  const iDef = text.indexOf('Defining VM with')
  const afterDef = iDef >= 0 ? text.slice(iDef) : ''
  const hasHostCmd = /\[(virt-install|qemu-img)] \$/.test(afterDef)
  if (hasHostCmd) return 2
  if (iDef >= 0) return 1
  return 0
}

/** VM create stream: machina milestones + virt-install / qemu-img command line. */
export function computeVmCreateTimeline(
  logs: string[],
  submitting: boolean,
  doneOk: boolean,
  failed: boolean,
): TimelineState {
  const text = logs.join('\n')
  if (failed) {
    return { activeIndex: vmCreateFailedPhase(text), allComplete: false, failed: true }
  }
  if (doneOk) {
    return { activeIndex: VM_CREATE_TIMELINE_LABELS.length - 1, allComplete: true, failed: false }
  }
  const iDef = text.indexOf('Defining VM with')
  const afterDef = iDef >= 0 ? text.slice(iDef) : ''
  const hasHostCmd = /\[(virt-install|qemu-img)] \$/.test(afterDef)
  if (iDef < 0) {
    return { activeIndex: 0, allComplete: false, failed: false }
  }
  if (!hasHostCmd && submitting) {
    return { activeIndex: 1, allComplete: false, failed: false }
  }
  if (hasHostCmd && submitting) {
    return { activeIndex: 2, allComplete: false, failed: false }
  }
  if (submitting) {
    return { activeIndex: 1, allComplete: false, failed: false }
  }
  return { activeIndex: 0, allComplete: false, failed: false }
}

function goldenFailedPhase(text: string): number {
  if (text.includes('[machina] Artifact:') || /\.qcow2\b/i.test(text)) return 2
  if (
    /==>|qemu-system|packer build|Provisioning with|\bPacker\b|Downloading|Building '|\[dockur\]|podman|dockurr\/windows/i.test(
      text,
    )
  ) {
    return 1
  }
  if (text.includes('[machina]')) return 0
  return 0
}

/** Golden Forge / POST /jobs/packer-golden-build log lines. */
export function computeGoldenForgeTimeline(
  logs: string[],
  running: boolean,
  doneOk: boolean,
  failed: boolean,
): TimelineState {
  const text = logs.join('\n')
  if (failed) {
    return { activeIndex: goldenFailedPhase(text), allComplete: false, failed: true }
  }
  if (doneOk) {
    return { activeIndex: GOLDEN_FORGE_TIMELINE_LABELS.length - 1, allComplete: true, failed: false }
  }
  if (text.includes('[machina] Artifact:') || /\.qcow2\b/i.test(text)) {
    return { activeIndex: 2, allComplete: false, failed: false }
  }
  if (
    /==>|qemu-system|packer build|Provisioning with|\bPacker\b|Downloading|Building '|Starting build|autoinstall|\[dockur\]|podman|dockurr\/windows/i.test(
      text,
    )
  ) {
    return { activeIndex: 1, allComplete: false, failed: false }
  }
  if (running || text.includes('[machina]')) {
    return { activeIndex: 0, allComplete: false, failed: false }
  }
  return { activeIndex: 0, allComplete: false, failed: false }
}

/** virt-image-build job logs. */
export function computeVirtImageBuildTimeline(
  logs: string[],
  status: 'running' | 'completed' | 'failed',
): TimelineState {
  const text = logs.join('\n')
  if (status === 'failed') {
    const idx = /virt-image-build|virt-builder|ERROR|error:/i.test(text) ? 1 : 0
    return { activeIndex: idx, allComplete: false, failed: true }
  }
  if (status === 'completed') {
    return { activeIndex: VIRT_IMAGE_TIMELINE_LABELS.length - 1, allComplete: true, failed: false }
  }
  if (/virt-builder|virt-image-build|Building|qemu-img|\[virt-builder]/i.test(text)) {
    return { activeIndex: 1, allComplete: false, failed: false }
  }
  return { activeIndex: 0, allComplete: false, failed: false }
}

export function computeVmCreateJobTimeline(
  logs: string[],
  status: 'running' | 'completed' | 'failed',
): TimelineState {
  const text = logs.join('\n')
  if (status === 'failed') {
    return { activeIndex: vmCreateFailedPhase(text), allComplete: false, failed: true }
  }
  if (status === 'completed') {
    return { activeIndex: VM_CREATE_TIMELINE_LABELS.length - 1, allComplete: true, failed: false }
  }
  return computeVmCreateTimeline(logs, true, false, false)
}

export function computePackerJobTimeline(
  logs: string[],
  status: 'running' | 'completed' | 'failed',
): TimelineState {
  if (status === 'failed') {
    return computeGoldenForgeTimeline(logs, false, false, true)
  }
  if (status === 'completed') {
    return computeGoldenForgeTimeline(logs, false, true, false)
  }
  return computeGoldenForgeTimeline(logs, true, false, false)
}

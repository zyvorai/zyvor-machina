// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

const KEY_PREFIX = 'machina-vm-poster:'

export function saveVmPosterScreenshot(vmId: string, dataUrl: string): void {
  try {
    localStorage.setItem(`${KEY_PREFIX}${vmId}`, dataUrl)
  } catch {
    /* quota */
  }
}

export function loadVmPosterScreenshot(vmId: string): string | null {
  try {
    return localStorage.getItem(`${KEY_PREFIX}${vmId}`)
  } catch {
    return null
  }
}

export function downloadCanvasScreenshot(canvas: HTMLCanvasElement, filename: string): void {
  const link = document.createElement('a')
  link.download = filename
  link.href = canvas.toDataURL('image/png')
  link.click()
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Read an OpenSSH `.pub` file into the callback (wizard / template deploy). */
export function readSshPubkeyFile(file: File | undefined, onKey: (key: string) => void): void {
  if (!file) return
  if (!file.name.endsWith('.pub')) return
  const reader = new FileReader()
  reader.onload = () => {
    const text = String(reader.result ?? '').trim()
    if (text) onKey(text)
  }
  reader.readAsText(file)
}

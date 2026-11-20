// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useLocation } from 'react-router'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { useAi } from '../../contexts/AiContext'

/** Floating Zyra pill — superseded by Navbar (classic) and Dynamic Island (platform). */
export default function ZyraAmbientBar() {
  const location = useLocation()
  const { info } = usePlatformInfo()
  const { mode } = useAi()
  const platform = Boolean(info?.control_plane?.proxy_url)

  if (!platform || mode === 'off') return null
  if (location.pathname.startsWith('/platform')) return null
  // Classic shell: Navbar exposes Zyra when AI is on.
  return null
}

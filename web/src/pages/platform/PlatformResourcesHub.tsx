// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Navigate } from 'react-router'

/** @deprecated Kept for bookmarks — redirects to storage. */
export default function PlatformResourcesHub() {
  return <Navigate to="/platform/storage" replace />
}

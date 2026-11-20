// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Re-exports from platformNavRegistry for backward compatibility.

export type {
  ContextNavItem,
  PlatformContextNav,
} from './platformNavRegistry'

export {
  SETTINGS_WORKSPACE_PATHS,
  contextNavForPath,
  isContextNavActive,
  isSettingsContextPath,
  isSettingsWorkspacePath,
  MAX_CONTEXT_PILLS,
  operationsNavItemsForTier,
  settingsItemsForTier,
  shouldShowContextBar,
  splitContextNavItems,
  suppressContextBar,
} from './platformNavRegistry'

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { activityHubHref, hubHrefForTier, operationsHubHref, tasksHubHref } from './platformHubLinks'

describe('platformHubLinks', () => {
  it('routes Normal tier to notification center', () => {
    expect(operationsHubHref('normal')).toBe('/platform/notifications')
    expect(tasksHubHref('normal')).toBe('/platform/notifications')
    expect(activityHubHref('normal')).toBe('/platform/notifications')
  })

  it('routes Power and Advanced tiers to hub pages', () => {
    expect(operationsHubHref('power')).toBe('/platform/operations')
    expect(tasksHubHref('advanced')).toBe('/platform/tasks')
    expect(activityHubHref('power')).toBe('/platform/activity')
  })

  it('resolves desktop hub hrefs with tier-aware operations fallback', () => {
    expect(hubHrefForTier('operations', 'normal')).toBe('/platform/notifications')
    expect(hubHrefForTier('operations', 'power')).toBe('/platform/operations')
    expect(hubHrefForTier('infrastructure', 'power')).toBe('/platform/infrastructure')
    expect(hubHrefForTier('resources', 'power')).toBe('/platform/infrastructure')
    expect(hubHrefForTier('integrations', 'normal')).toBe('/platform/settings?section=integrations')
    expect(hubHrefForTier('integrations', 'power')).toBe('/platform/settings?section=integrations')
  })
})

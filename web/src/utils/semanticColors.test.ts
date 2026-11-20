// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { checkStatusTone, connectionStatusTone, hostStateTone, httpStatusTone, hubLinkClasses, jobStatusTone, journalPriorityTone, k8sPhaseTone, migrationReadinessTone, navActiveChipClasses, notificationChannelTone, instanceStatusTone, poolStateBadgeClasses, prereqTone, riskTone, sessionBadgeClasses, serviceStateTone, statusBorderClass, statusChipClasses, statusSurfaceClasses, tabActiveClasses, taskStatusTone, toastSemanticTone, utilizationBarClass, utilizationTone, userRoleTone, vmStateTone } from './semanticColors'

describe('semanticColors', () => {
  it('maps task statuses', () => {
    expect(taskStatusTone('completed')).toBe('ok')
    expect(taskStatusTone('failed')).toBe('error')
    expect(taskStatusTone('running')).toBe('info')
  })

  it('maps host states', () => {
    expect(hostStateTone('online')).toBe('ok')
    expect(hostStateTone('offline')).toBe('error')
    expect(hostStateTone('online', false, true)).toBe('warn')
  })

  it('maps vm and instance statuses', () => {
    expect(vmStateTone('running')).toBe('ok')
    expect(instanceStatusTone('ACTIVE')).toBe('ok')
    expect(k8sPhaseTone('Failed')).toBe('error')
  })

  it('builds session badge classes', () => {
    expect(sessionBadgeClasses()).toContain('--machina-status-warn')
  })

  it('maps utilization percent to tone', () => {
    expect(utilizationTone(50)).toBe('ok')
    expect(utilizationTone(70)).toBe('ok')
    expect(utilizationTone(71)).toBe('warn')
    expect(utilizationTone(95)).toBe('error')
    expect(utilizationBarClass(95)).toContain('--machina-status-error')
    expect(utilizationBarClass(15, { warn: 20, error: 50 })).toContain('--machina-status-ok')
  })

  it('builds pool state badge classes', () => {
    expect(poolStateBadgeClasses('running')).toContain('--machina-status-ok')
    expect(poolStateBadgeClasses('inactive')).toContain('--machina-status-neutral')
  })

  it('maps user roles and status surfaces', () => {
    expect(userRoleTone('admin')).toBe('error')
    expect(userRoleTone('operator')).toBe('info')
    expect(statusChipClasses('warn')).toContain('--machina-status-warn')
    expect(statusSurfaceClasses('ok')).toContain('--machina-status-ok')
  })

  it('maps check and service states', () => {
    expect(checkStatusTone('pass')).toBe('ok')
    expect(checkStatusTone('fail')).toBe('error')
    expect(serviceStateTone('active')).toBe('ok')
    expect(serviceStateTone('failed')).toBe('error')
    expect(prereqTone(true)).toBe('ok')
    expect(prereqTone(false)).toBe('warn')
    expect(prereqTone(false, 'error')).toBe('error')
  })

  it('maps job, journal, connection, and notification tones', () => {
    expect(jobStatusTone('running')).toBe('warn')
    expect(jobStatusTone('completed')).toBe('ok')
    expect(journalPriorityTone('err')).toBe('error')
    expect(journalPriorityTone('warning')).toBe('warn')
    expect(connectionStatusTone('connected')).toBe('ok')
    expect(notificationChannelTone('email')).toBe('info')
    expect(toastSemanticTone('success')).toBe('ok')
    expect(statusBorderClass('warn')).toContain('--machina-status-warn')
  })

  it('builds nav and tab active classes', () => {
    expect(navActiveChipClasses()).toContain('--machina-status-info')
    expect(tabActiveClasses()).toContain('--machina-status-info')
    expect(hubLinkClasses()).toContain('--machina-status-info')
  })

  it('maps firewall risk levels', () => {
    expect(riskTone('critical')).toBe('error')
    expect(riskTone('high')).toBe('error')
    expect(riskTone('warning')).toBe('warn')
    expect(riskTone('medium')).toBe('warn')
    expect(riskTone('low')).toBe('ok')
    expect(riskTone('info')).toBe('ok')
    expect(riskTone('unknown')).toBe('neutral')
  })
})

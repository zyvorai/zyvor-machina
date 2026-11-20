// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, apiPost, apiDelete } from './client'

const API = '/api/v1'

// RBAC
export interface UserRole { username: string; role: string }
export const listRoles = () => readJsonArray<UserRole>(`${API}/roles`)
export const setRole = (username: string, role: string) => apiPost<unknown>(`${API}/roles`, { username, role })

// API Tokens
export interface ApiToken { name: string; token: string; username: string; role: string; created: string }
export const listTokens = () => readJsonArray<ApiToken>(`${API}/tokens`)
export const createToken = (name: string, username: string, role: string) => apiPost<ApiToken>(`${API}/tokens`, { name, username, role })
export const deleteToken = (token: string) => apiDelete(`${API}/tokens/${encodeURIComponent(token)}`)

// Alerts
export interface AlertRule { id: string; name: string; condition: string; threshold: number; enabled: boolean }
export interface Alert { id: string; rule_name: string; message: string; severity: string; timestamp: string; acknowledged: boolean }
export const listAlertRules = () => readJsonArray<AlertRule>(`${API}/alert-rules`)
export const saveAlertRules = (rules: AlertRule[]) => apiPost<unknown>(`${API}/alert-rules`, rules)
export const listAlerts = () => readJsonArray<Alert>(`${API}/alerts`)
export const acknowledgeAlert = (id: string) => apiPost<unknown>(`${API}/alerts/${encodeURIComponent(id)}/ack`, {})

// Webhooks
export interface WebhookConfig { id: string; url: string; events: string[]; enabled: boolean }
export const listWebhooks = () => readJsonArray<WebhookConfig>(`${API}/webhooks`)
export const saveWebhooks = (hooks: WebhookConfig[]) => apiPost<unknown>(`${API}/webhooks`, hooks)

// Schedules
export interface ScheduledAction { id: string; vm_name: string; action: string; schedule: string; enabled: boolean; last_run: string }
export const listSchedules = () => readJsonArray<ScheduledAction>(`${API}/schedules`)
export const saveSchedules = (schedules: ScheduledAction[]) => apiPost<unknown>(`${API}/schedules`, schedules)

// Notification Channels
export interface NotificationChannel { id: string; channel_type: string; config: string; enabled: boolean }
export const listNotificationChannels = () => readJsonArray<NotificationChannel>(`${API}/notifications`)
export const saveNotificationChannels = (channels: NotificationChannel[]) => apiPost<unknown>(`${API}/notifications`, channels)
export const testNotification = (channel: NotificationChannel) => apiPost<unknown>(`${API}/notifications/test`, { channel })

// Snapshot Schedules
export interface SnapshotSchedule { id: string; vm_name: string; interval_hours: number; retain_count: number; enabled: boolean; last_run: string }
export const listSnapshotSchedules = () => readJsonArray<SnapshotSchedule>(`${API}/snapshot-schedules`)
export const saveSnapshotSchedules = (schedules: SnapshotSchedule[]) => apiPost<unknown>(`${API}/snapshot-schedules`, schedules)

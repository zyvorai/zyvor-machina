// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

package machina

import "context"

// Host is a hypervisor host.
type Host struct {
	ID              string  `json:"id"`
	Hostname        string  `json:"hostname"`
	Address         string  `json:"address"`
	State           string  `json:"state"`
	MaintenanceMode bool    `json:"maintenance_mode"`
	Schedulable     bool    `json:"schedulable"`
	VMCount         int     `json:"vm_count"`
	CPUPercent      float32 `json:"cpu_percent"`
	MemoryUsedMiB   int64   `json:"memory_used_mib"`
	MemoryTotalMiB  int64   `json:"memory_total_mib"`
	Site            string  `json:"site"`
}

// ListHosts returns every enrolled host.
func (c *Client) ListHosts(ctx context.Context) ([]Host, error) {
	var out []Host
	return out, c.Do(ctx, "GET", "/api/v1/hosts", nil, nil, &out)
}

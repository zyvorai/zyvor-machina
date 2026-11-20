// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

package machina

import (
	"context"
	"fmt"
	"net/url"
	"time"
)

// VM is a machine as the controller reports it.
type VM struct {
	ID               string   `json:"id"`
	Name             string   `json:"name"`
	HostID           *string  `json:"host_id"`
	DesiredState     string   `json:"desired_state"`
	ObservedState    string   `json:"observed_state"`
	LifecyclePhase   string   `json:"lifecycle_phase"`
	LastError        string   `json:"last_error"`
	VCPUs            int      `json:"vcpus"`
	MemoryMiB        int64    `json:"memory_mib"`
	HAEnabled        bool     `json:"ha_enabled"`
	Project          *string  `json:"project"`
	Tags             []string `json:"tags"`
	GuestIP          *string  `json:"guest_ip"`
	GuestToolsStatus *string  `json:"guest_tools_status"`
}

// Task is a long-running operation the controller started for you.
type Task struct {
	TaskID    string `json:"task_id"`
	Status    string `json:"status"`
	Operation string `json:"operation"`
}

// TaskStatus is the polled state of a task.
type TaskStatus struct {
	ID        string  `json:"id"`
	Operation string  `json:"operation"`
	Status    string  `json:"status"`
	Progress  int     `json:"progress"`
	Message   *string `json:"message"`
}

// CreateVMRequest describes a new machine. Sizes use Kubernetes-style quantities ("4Gi", "80Gi").
type CreateVMRequest struct {
	Name         string
	Project      string
	VCPUs        int
	Memory       string
	DiskSize     string
	StorageClass string
	Network      string
	HostID       string
	Tags         []string
	// DesiredState is "running" (default) or "stopped".
	DesiredState string
}

func (r CreateVMRequest) body() map[string]any {
	mem, disk, class, network, state := r.Memory, r.DiskSize, r.StorageClass, r.Network, r.DesiredState
	if mem == "" {
		mem = "2Gi"
	}
	if disk == "" {
		disk = "20Gi"
	}
	if class == "" {
		class = "silver"
	}
	if network == "" {
		network = "default"
	}
	if state == "" {
		state = "running"
	}
	cores := r.VCPUs
	if cores < 1 {
		cores = 1
	}
	meta := map[string]any{"name": r.Name}
	if r.Project != "" {
		meta["project"] = r.Project
	}
	b := map[string]any{
		"api_version": "virt.zyvor.dev/v1",
		"kind":        "VirtualMachine",
		"metadata":    meta,
		"spec": map[string]any{
			"cpu":      map[string]any{"sockets": 1, "cores": cores},
			"memory":   mem,
			"storage":  []map[string]any{{"name": "root", "size": disk, "class": class}},
			"network":  []map[string]any{{"network": network, "ip_mode": "dhcp"}},
			"firmware": "bios",
			"graphics": map[string]any{"type": "vnc", "listen": "127.0.0.1"},
		},
		"tags":          append([]string{}, r.Tags...),
		"desired_state": state,
	}
	if r.HostID != "" {
		b["host_id"] = r.HostID
	}
	return b
}

// ListVMs returns every machine, optionally filtered by project.
func (c *Client) ListVMs(ctx context.Context, project string) ([]VM, error) {
	q := url.Values{}
	if project != "" {
		q.Set("project", project)
	}
	var out []VM
	return out, c.Do(ctx, "GET", "/api/v1/vms", q, nil, &out)
}

// GetVM returns one machine by id.
func (c *Client) GetVM(ctx context.Context, id string) (*VM, error) {
	var out VM
	if err := c.Do(ctx, "GET", "/api/v1/vms/"+url.PathEscape(id), nil, nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// CreateVM asks the controller to build a machine. It returns the task; the machine's id is found by name once
// the task has been accepted (see FindVMByName) because creation is asynchronous.
func (c *Client) CreateVM(ctx context.Context, req CreateVMRequest) (*Task, error) {
	if req.Name == "" {
		return nil, fmt.Errorf("machina: a machine needs a name")
	}
	var t Task
	if err := c.Do(ctx, "POST", "/api/v1/vms", nil, req.body(), &t); err != nil {
		return nil, err
	}
	return &t, nil
}

// FindVMByName returns the machine with this exact name, or nil.
func (c *Client) FindVMByName(ctx context.Context, name string) (*VM, error) {
	vms, err := c.ListVMs(ctx, "")
	if err != nil {
		return nil, err
	}
	for i := range vms {
		if vms[i].Name == name {
			return &vms[i], nil
		}
	}
	return nil, nil
}

// DeleteVM removes a machine (its disks too). It returns the task.
func (c *Client) DeleteVM(ctx context.Context, id string) (*Task, error) {
	var t Task
	if err := c.Do(ctx, "POST", "/api/v1/vms/"+url.PathEscape(id)+"/delete", nil, map[string]any{}, &t); err != nil {
		return nil, err
	}
	return &t, nil
}

// Power runs one of "start", "shutdown" (clean), "stop" (power off) or "reboot".
func (c *Client) Power(ctx context.Context, id, action string) (*Task, error) {
	switch action {
	case "start", "shutdown", "stop", "reboot":
	default:
		return nil, fmt.Errorf("machina: unknown power action %q", action)
	}
	var t Task
	if err := c.Do(ctx, "POST", "/api/v1/vms/"+url.PathEscape(id)+"/"+action, nil, map[string]any{}, &t); err != nil {
		return nil, err
	}
	return &t, nil
}

// GetTask reads a task's state.
func (c *Client) GetTask(ctx context.Context, id string) (*TaskStatus, error) {
	var out TaskStatus
	if err := c.Do(ctx, "GET", "/api/v1/tasks/"+url.PathEscape(id), nil, nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// WaitTask polls until the task succeeds, fails or ctx ends.
func (c *Client) WaitTask(ctx context.Context, id string, every time.Duration) (*TaskStatus, error) {
	if every <= 0 {
		every = 2 * time.Second
	}
	for {
		t, err := c.GetTask(ctx, id)
		if err != nil {
			return nil, err
		}
		switch t.Status {
		case "succeeded", "completed", "success":
			return t, nil
		case "failed", "error", "cancelled":
			msg := ""
			if t.Message != nil {
				msg = *t.Message
			}
			return t, fmt.Errorf("machina: task %s (%s) %s: %s", t.ID, t.Operation, t.Status, msg)
		}
		select {
		case <-ctx.Done():
			return t, ctx.Err()
		case <-time.After(every):
		}
	}
}

// WaitVMState polls until the machine's observed state equals want (for example "running").
func (c *Client) WaitVMState(ctx context.Context, id, want string, every time.Duration) (*VM, error) {
	if every <= 0 {
		every = 3 * time.Second
	}
	for {
		vm, err := c.GetVM(ctx, id)
		if err != nil {
			return nil, err
		}
		if vm.ObservedState == want {
			return vm, nil
		}
		select {
		case <-ctx.Done():
			return vm, ctx.Err()
		case <-time.After(every):
		}
	}
}

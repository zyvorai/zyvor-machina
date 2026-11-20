// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//go:build integration

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

package machina

import (
	"context"
	"os"
	"testing"
	"time"
)

// Run against a lab controller:
//
//	MACHINA_URL=https://host:5092/api/v1/platform/controller MACHINA_TOKEN=<api key> \
//	  go test -tags integration ./machina -run Live -v
//
// Set MACHINA_TEST_CREATE=1 to also create and delete a throwaway machine (never touches existing ones).
func liveClient(t *testing.T) *Client {
	url, tok := os.Getenv("MACHINA_URL"), os.Getenv("MACHINA_TOKEN")
	if url == "" || tok == "" {
		t.Skip("MACHINA_URL / MACHINA_TOKEN not set")
	}
	return New(url, tok, WithInsecureTLS())
}

func TestLiveReadOnly(t *testing.T) {
	c := liveClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	hosts, err := c.ListHosts(ctx)
	if err != nil {
		t.Fatal(err)
	}
	vms, err := c.ListVMs(ctx, "")
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("%d host(s), %d machine(s)", len(hosts), len(vms))
	if len(vms) > 0 {
		got, err := c.GetVM(ctx, vms[0].ID)
		if err != nil || got.Name != vms[0].Name {
			t.Fatalf("get: %v %+v", err, got)
		}
	}
}

func TestLiveThrowawayLifecycle(t *testing.T) {
	if os.Getenv("MACHINA_TEST_CREATE") != "1" {
		t.Skip("set MACHINA_TEST_CREATE=1 to create a throwaway machine")
	}
	c := liveClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()
	name := "sdk-throwaway-" + time.Now().Format("150405")
	task, err := c.CreateVM(ctx, CreateVMRequest{Name: name, VCPUs: 1, Memory: "512Mi", DiskSize: "1Gi", DesiredState: "stopped", Tags: []string{"sdk-test"}})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := c.WaitTask(ctx, task.TaskID, 2*time.Second); err != nil {
		t.Fatal(err)
	}
	vm, err := c.FindVMByName(ctx, name)
	if err != nil || vm == nil {
		t.Fatalf("created machine not found: %v", err)
	}
	t.Logf("created %s (%s) state=%s", vm.Name, vm.ID, vm.ObservedState)
	del, err := c.DeleteVM(ctx, vm.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := c.WaitTask(ctx, del.TaskID, 2*time.Second); err != nil {
		t.Fatal(err)
	}
	if again, _ := c.FindVMByName(ctx, name); again != nil {
		t.Fatalf("machine %s still exists after delete", name)
	}
}

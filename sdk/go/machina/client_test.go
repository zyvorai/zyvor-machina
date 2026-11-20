// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

package machina

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func newTestServer(t *testing.T, h http.HandlerFunc) (*Client, func()) {
	t.Helper()
	s := httptest.NewServer(h)
	return New(s.URL, "tok"), s.Close
}

func TestBearerTokenAndErrorDecoding(t *testing.T) {
	c, done := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer tok" {
			t.Errorf("missing bearer token: %q", r.Header.Get("Authorization"))
		}
		w.WriteHeader(404)
		_, _ = w.Write([]byte(`{"error":"vm not found","error_code":"not_found","remediation":"check the id"}`))
	})
	defer done()
	_, err := c.GetVM(context.Background(), "nope")
	if !IsNotFound(err) {
		t.Fatalf("want not-found, got %v", err)
	}
	ae := err.(*APIError)
	if ae.Code != "not_found" || ae.Remediation != "check the id" {
		t.Fatalf("error fields not decoded: %+v", ae)
	}
}

func TestCreateVMSendsTheExpectedSpec(t *testing.T) {
	var got map[string]any
	c, done := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" || r.URL.Path != "/api/v1/vms" {
			t.Errorf("unexpected %s %s", r.Method, r.URL.Path)
		}
		_ = json.NewDecoder(r.Body).Decode(&got)
		_, _ = w.Write([]byte(`{"task_id":"t1","status":"pending","operation":"vm.apply"}`))
	})
	defer done()
	task, err := c.CreateVM(context.Background(), CreateVMRequest{Name: "web-1", VCPUs: 4, Memory: "8Gi", Tags: []string{"prod"}})
	if err != nil || task.TaskID != "t1" {
		t.Fatalf("create: %v %+v", err, task)
	}
	spec := got["spec"].(map[string]any)
	if got["metadata"].(map[string]any)["name"] != "web-1" || spec["memory"] != "8Gi" {
		t.Fatalf("bad body: %v", got)
	}
	if spec["cpu"].(map[string]any)["cores"] != float64(4) {
		t.Fatalf("cores not set: %v", spec["cpu"])
	}
	if got["desired_state"] != "running" {
		t.Fatalf("default desired state: %v", got["desired_state"])
	}
}

func TestCreateVMNeedsAName(t *testing.T) {
	if _, err := New("http://x", "").CreateVM(context.Background(), CreateVMRequest{}); err == nil {
		t.Fatal("expected an error for a nameless machine")
	}
}

func TestPowerRejectsUnknownActions(t *testing.T) {
	if _, err := New("http://x", "").Power(context.Background(), "id", "explode"); err == nil {
		t.Fatal("expected an error")
	}
}

func TestWaitTaskReturnsOnSuccessAndErrorsOnFailure(t *testing.T) {
	calls := 0
	c, done := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		calls++
		status := "running"
		if calls >= 2 {
			status = "succeeded"
		}
		_, _ = w.Write([]byte(`{"id":"t1","operation":"vm.apply","status":"` + status + `","progress":50}`))
	})
	defer done()
	ts, err := c.WaitTask(context.Background(), "t1", time.Millisecond)
	if err != nil || ts.Status != "succeeded" || calls != 2 {
		t.Fatalf("wait: %v %+v calls=%d", err, ts, calls)
	}

	c2, done2 := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"id":"t2","operation":"vm.apply","status":"failed","progress":0,"message":"no capacity"}`))
	})
	defer done2()
	if _, err := c2.WaitTask(context.Background(), "t2", time.Millisecond); err == nil {
		t.Fatal("a failed task must be an error")
	}
}

func TestWaitVMStateHonoursContext(t *testing.T) {
	c, done := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"id":"v","name":"x","observed_state":"stopped"}`))
	})
	defer done()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	if _, err := c.WaitVMState(ctx, "v", "running", 5*time.Millisecond); err == nil {
		t.Fatal("expected the context deadline to end the wait")
	}
}

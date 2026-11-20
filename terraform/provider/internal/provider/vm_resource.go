// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

package provider

import (
	"context"
	"fmt"
	"time"

	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"
	"github.com/zyvorai/zyvor-machina/sdk/go/machina"
)

var (
	_ resource.Resource                = &vmResource{}
	_ resource.ResourceWithConfigure   = &vmResource{}
	_ resource.ResourceWithImportState = &vmResource{}
)

// NewVMResource is the machina_vm resource factory.
func NewVMResource() resource.Resource { return &vmResource{} }

type vmResource struct{ client *machina.Client }

type vmModel struct {
	ID            types.String `tfsdk:"id"`
	Name          types.String `tfsdk:"name"`
	Project       types.String `tfsdk:"project"`
	VCPUs         types.Int64  `tfsdk:"vcpus"`
	Memory        types.String `tfsdk:"memory"`
	DiskSize      types.String `tfsdk:"disk_size"`
	StorageClass  types.String `tfsdk:"storage_class"`
	Network       types.String `tfsdk:"network"`
	HostID        types.String `tfsdk:"host_id"`
	Tags          types.List   `tfsdk:"tags"`
	DesiredState  types.String `tfsdk:"desired_state"`
	ObservedState types.String `tfsdk:"observed_state"`
	GuestIP       types.String `tfsdk:"guest_ip"`
}

func (r *vmResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_vm"
}

func (r *vmResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	replace := []planmodifier.String{stringplanmodifier.RequiresReplace()}
	resp.Schema = schema.Schema{
		Description: "A virtual machine. Changing its size, disk, network, host or name replaces it; `desired_state` is applied in place.",
		Attributes: map[string]schema.Attribute{
			"id":             schema.StringAttribute{Computed: true, PlanModifiers: []planmodifier.String{stringplanmodifier.UseStateForUnknown()}},
			"name":           schema.StringAttribute{Required: true, PlanModifiers: replace, Description: "Machine name (unique)."},
			"project":        schema.StringAttribute{Optional: true, PlanModifiers: replace, Description: "Project to create it in."},
			"vcpus":          schema.Int64Attribute{Optional: true, Computed: true, Description: "vCPU cores (default 1). Changing it replaces the machine."},
			"memory":         schema.StringAttribute{Optional: true, Computed: true, PlanModifiers: replace, Description: "Memory, e.g. \"4Gi\" (default 2Gi)."},
			"disk_size":      schema.StringAttribute{Optional: true, Computed: true, PlanModifiers: replace, Description: "Root disk size, e.g. \"40Gi\" (default 20Gi)."},
			"storage_class":  schema.StringAttribute{Optional: true, Computed: true, PlanModifiers: replace, Description: "Storage class (default silver)."},
			"network":        schema.StringAttribute{Optional: true, Computed: true, PlanModifiers: replace, Description: "Network name (default \"default\")."},
			"host_id":        schema.StringAttribute{Optional: true, Computed: true, PlanModifiers: replace, Description: "Pin to a host; omit to let the scheduler place it."},
			"tags":           schema.ListAttribute{Optional: true, ElementType: types.StringType, Description: "Free-form tags."},
			"desired_state":  schema.StringAttribute{Optional: true, Computed: true, Description: "\"running\" (default) or \"stopped\"."},
			"observed_state": schema.StringAttribute{Computed: true, Description: "What the hypervisor reports."},
			"guest_ip":       schema.StringAttribute{Computed: true, Description: "Guest IP once the agent reports it."},
		},
	}
}

func (r *vmResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	if req.ProviderData == nil {
		return
	}
	c, ok := req.ProviderData.(*machina.Client)
	if !ok {
		resp.Diagnostics.AddError("Unexpected provider data", fmt.Sprintf("got %T", req.ProviderData))
		return
	}
	r.client = c
}

func (r *vmResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan vmModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	var tags []string
	if !plan.Tags.IsNull() {
		resp.Diagnostics.Append(plan.Tags.ElementsAs(ctx, &tags, false)...)
	}
	task, err := r.client.CreateVM(ctx, machina.CreateVMRequest{
		Name: plan.Name.ValueString(), Project: plan.Project.ValueString(), VCPUs: int(plan.VCPUs.ValueInt64()),
		Memory: plan.Memory.ValueString(), DiskSize: plan.DiskSize.ValueString(), StorageClass: plan.StorageClass.ValueString(),
		Network: plan.Network.ValueString(), HostID: plan.HostID.ValueString(), Tags: tags, DesiredState: plan.DesiredState.ValueString(),
	})
	if err != nil {
		resp.Diagnostics.AddError("Creating the machine failed", err.Error())
		return
	}
	if _, err := r.client.WaitTask(ctx, task.TaskID, 2*time.Second); err != nil {
		resp.Diagnostics.AddError("The machine did not finish building", err.Error())
		return
	}
	vm, err := r.client.FindVMByName(ctx, plan.Name.ValueString())
	if err != nil || vm == nil {
		resp.Diagnostics.AddError("The machine was created but could not be found", fmt.Sprint(err))
		return
	}
	if plan.DesiredState.ValueString() != "stopped" && vm.ObservedState != "running" {
		if v, err := r.client.WaitVMState(ctx, vm.ID, "running", 3*time.Second); err == nil {
			vm = v
		} else {
			resp.Diagnostics.AddWarning("Machine is not running yet", err.Error())
		}
	}
	fill(&plan, vm)
	resp.Diagnostics.Append(resp.State.Set(ctx, plan)...)
}

func (r *vmResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state vmModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	vm, err := r.client.GetVM(ctx, state.ID.ValueString())
	if machina.IsNotFound(err) {
		resp.State.RemoveResource(ctx) // deleted outside Terraform
		return
	}
	if err != nil {
		resp.Diagnostics.AddError("Reading the machine failed", err.Error())
		return
	}
	fill(&state, vm)
	resp.Diagnostics.Append(resp.State.Set(ctx, state)...)
}

// Update only handles power state; everything else that changes replaces the machine.
func (r *vmResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan, state vmModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	id := state.ID.ValueString()
	want := plan.DesiredState.ValueString()
	if want != "" && want != state.DesiredState.ValueString() {
		action, target := "start", "running"
		if want == "stopped" {
			action, target = "shutdown", "shutoff"
		}
		task, err := r.client.Power(ctx, id, action)
		if err != nil {
			resp.Diagnostics.AddError("Changing the power state failed", err.Error())
			return
		}
		if _, err := r.client.WaitTask(ctx, task.TaskID, 2*time.Second); err != nil {
			resp.Diagnostics.AddError("The power change did not finish", err.Error())
			return
		}
		_, _ = r.client.WaitVMState(ctx, id, target, 3*time.Second)
	}
	vm, err := r.client.GetVM(ctx, id)
	if err != nil {
		resp.Diagnostics.AddError("Reading the machine failed", err.Error())
		return
	}
	plan.ID = state.ID
	fill(&plan, vm)
	resp.Diagnostics.Append(resp.State.Set(ctx, plan)...)
}

func (r *vmResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state vmModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	task, err := r.client.DeleteVM(ctx, state.ID.ValueString())
	if machina.IsNotFound(err) {
		return
	}
	if err != nil {
		resp.Diagnostics.AddError("Deleting the machine failed", err.Error())
		return
	}
	if _, err := r.client.WaitTask(ctx, task.TaskID, 2*time.Second); err != nil {
		resp.Diagnostics.AddError("The delete did not finish", err.Error())
	}
}

func (r *vmResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("id"), req, resp)
}

// fill copies what the controller reports into the model, keeping the user's own spelling for sizes.
func fill(m *vmModel, vm *machina.VM) {
	m.ID = types.StringValue(vm.ID)
	m.Name = types.StringValue(vm.Name)
	m.VCPUs = types.Int64Value(int64(vm.VCPUs))
	m.DesiredState = types.StringValue(vm.DesiredState)
	m.ObservedState = types.StringValue(vm.ObservedState)
	if vm.HostID != nil {
		m.HostID = types.StringValue(*vm.HostID)
	}
	if vm.GuestIP != nil {
		m.GuestIP = types.StringValue(*vm.GuestIP)
	} else {
		m.GuestIP = types.StringValue("")
	}
	if m.Memory.IsNull() || m.Memory.IsUnknown() {
		m.Memory = types.StringValue(fmt.Sprintf("%dMi", vm.MemoryMiB))
	}
	if m.DiskSize.IsUnknown() {
		m.DiskSize = types.StringValue("")
	}
	if m.StorageClass.IsUnknown() {
		m.StorageClass = types.StringValue("")
	}
	if m.Network.IsUnknown() {
		m.Network = types.StringValue("")
	}
}

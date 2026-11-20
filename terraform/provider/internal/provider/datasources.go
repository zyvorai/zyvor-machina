// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

package provider

import (
	"context"
	"fmt"

	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/datasource/schema"
	"github.com/hashicorp/terraform-plugin-framework/types"
	"github.com/zyvorai/zyvor-machina/sdk/go/machina"
)

// ── machina_hosts ───────────────────────────────────────────────────────────

func NewHostsDataSource() datasource.DataSource { return &hostsDataSource{} }

type hostsDataSource struct{ client *machina.Client }

type hostItem struct {
	ID       types.String `tfsdk:"id"`
	Hostname types.String `tfsdk:"hostname"`
	Address  types.String `tfsdk:"address"`
	State    types.String `tfsdk:"state"`
	VMCount  types.Int64  `tfsdk:"vm_count"`
}

type hostsModel struct {
	Hosts []hostItem `tfsdk:"hosts"`
}

func (d *hostsDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_hosts"
}

func (d *hostsDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		Description: "Enrolled hypervisor hosts.",
		Attributes: map[string]schema.Attribute{
			"hosts": schema.ListNestedAttribute{Computed: true, NestedObject: schema.NestedAttributeObject{Attributes: map[string]schema.Attribute{
				"id": schema.StringAttribute{Computed: true}, "hostname": schema.StringAttribute{Computed: true},
				"address": schema.StringAttribute{Computed: true}, "state": schema.StringAttribute{Computed: true},
				"vm_count": schema.Int64Attribute{Computed: true},
			}}},
		},
	}
}

func (d *hostsDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	if c, ok := req.ProviderData.(*machina.Client); ok {
		d.client = c
	}
}

func (d *hostsDataSource) Read(ctx context.Context, _ datasource.ReadRequest, resp *datasource.ReadResponse) {
	hosts, err := d.client.ListHosts(ctx)
	if err != nil {
		resp.Diagnostics.AddError("Listing hosts failed", err.Error())
		return
	}
	var m hostsModel
	for _, h := range hosts {
		m.Hosts = append(m.Hosts, hostItem{
			ID: types.StringValue(h.ID), Hostname: types.StringValue(h.Hostname), Address: types.StringValue(h.Address),
			State: types.StringValue(h.State), VMCount: types.Int64Value(int64(h.VMCount)),
		})
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, &m)...)
}

// ── machina_vms ─────────────────────────────────────────────────────────────

func NewVMsDataSource() datasource.DataSource { return &vmsDataSource{} }

type vmsDataSource struct{ client *machina.Client }

type vmItem struct {
	ID            types.String `tfsdk:"id"`
	Name          types.String `tfsdk:"name"`
	ObservedState types.String `tfsdk:"observed_state"`
	VCPUs         types.Int64  `tfsdk:"vcpus"`
	MemoryMiB     types.Int64  `tfsdk:"memory_mib"`
	GuestIP       types.String `tfsdk:"guest_ip"`
}

type vmsModel struct {
	Project types.String `tfsdk:"project"`
	VMs     []vmItem     `tfsdk:"vms"`
}

func (d *vmsDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_vms"
}

func (d *vmsDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		Description: "Machines, optionally for one project.",
		Attributes: map[string]schema.Attribute{
			"project": schema.StringAttribute{Optional: true},
			"vms": schema.ListNestedAttribute{Computed: true, NestedObject: schema.NestedAttributeObject{Attributes: map[string]schema.Attribute{
				"id": schema.StringAttribute{Computed: true}, "name": schema.StringAttribute{Computed: true},
				"observed_state": schema.StringAttribute{Computed: true}, "vcpus": schema.Int64Attribute{Computed: true},
				"memory_mib": schema.Int64Attribute{Computed: true}, "guest_ip": schema.StringAttribute{Computed: true},
			}}},
		},
	}
}

func (d *vmsDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	if c, ok := req.ProviderData.(*machina.Client); ok {
		d.client = c
	}
}

func (d *vmsDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	var m vmsModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &m)...)
	vms, err := d.client.ListVMs(ctx, m.Project.ValueString())
	if err != nil {
		resp.Diagnostics.AddError("Listing machines failed", err.Error())
		return
	}
	m.VMs = nil
	for _, v := range vms {
		ip := ""
		if v.GuestIP != nil {
			ip = *v.GuestIP
		}
		m.VMs = append(m.VMs, vmItem{
			ID: types.StringValue(v.ID), Name: types.StringValue(v.Name), ObservedState: types.StringValue(v.ObservedState),
			VCPUs: types.Int64Value(int64(v.VCPUs)), MemoryMiB: types.Int64Value(v.MemoryMiB), GuestIP: types.StringValue(ip),
		})
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, &m)...)
}

var _ = fmt.Sprintf

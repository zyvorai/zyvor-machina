// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Package provider is the Terraform provider for Machina.
package provider

import (
	"context"
	"os"

	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/provider"
	"github.com/hashicorp/terraform-plugin-framework/provider/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/types"
	"github.com/zyvorai/zyvor-machina/sdk/go/machina"
)

type machinaProvider struct{ version string }

// New returns the provider factory.
func New(version string) func() provider.Provider {
	return func() provider.Provider { return &machinaProvider{version: version} }
}

type providerModel struct {
	URL      types.String `tfsdk:"url"`
	Token    types.String `tfsdk:"token"`
	Insecure types.Bool   `tfsdk:"insecure"`
}

func (p *machinaProvider) Metadata(_ context.Context, _ provider.MetadataRequest, resp *provider.MetadataResponse) {
	resp.TypeName = "machina"
	resp.Version = p.version
}

func (p *machinaProvider) Schema(_ context.Context, _ provider.SchemaRequest, resp *provider.SchemaResponse) {
	resp.Schema = schema.Schema{
		Description: "Manage a Machina private cloud.",
		Attributes: map[string]schema.Attribute{
			"url": schema.StringAttribute{
				Optional:    true,
				Description: "Controller URL, e.g. https://10.0.0.5:5093. Defaults to $MACHINA_URL.",
			},
			"token": schema.StringAttribute{
				Optional:    true,
				Sensitive:   true,
				Description: "API key (Settings -> API keys). Defaults to $MACHINA_TOKEN.",
			},
			"insecure": schema.BoolAttribute{
				Optional:    true,
				Description: "Accept a self-signed controller certificate (lab only). Defaults to $MACHINA_INSECURE=1.",
			},
		},
	}
}

func (p *machinaProvider) Configure(_ context.Context, req provider.ConfigureRequest, resp *provider.ConfigureResponse) {
	var cfg providerModel
	resp.Diagnostics.Append(req.Config.Get(context.Background(), &cfg)...)
	if resp.Diagnostics.HasError() {
		return
	}
	url := firstNonEmpty(cfg.URL.ValueString(), os.Getenv("MACHINA_URL"))
	token := firstNonEmpty(cfg.Token.ValueString(), os.Getenv("MACHINA_TOKEN"))
	insecure := cfg.Insecure.ValueBool() || os.Getenv("MACHINA_INSECURE") == "1"
	if url == "" {
		resp.Diagnostics.AddError("Missing controller URL", "Set provider \"machina\" { url = ... } or the MACHINA_URL environment variable.")
	}
	if token == "" {
		resp.Diagnostics.AddError("Missing API key", "Set provider \"machina\" { token = ... } or the MACHINA_TOKEN environment variable.")
	}
	if resp.Diagnostics.HasError() {
		return
	}
	var opts []machina.Option
	if insecure {
		opts = append(opts, machina.WithInsecureTLS())
	}
	c := machina.New(url, token, opts...)
	resp.DataSourceData = c
	resp.ResourceData = c
}

func (p *machinaProvider) Resources(_ context.Context) []func() resource.Resource {
	return []func() resource.Resource{NewVMResource}
}

func (p *machinaProvider) DataSources(_ context.Context) []func() datasource.DataSource {
	return []func() datasource.DataSource{NewHostsDataSource, NewVMsDataSource}
}

func firstNonEmpty(v ...string) string {
	for _, s := range v {
		if s != "" {
			return s
		}
	}
	return ""
}

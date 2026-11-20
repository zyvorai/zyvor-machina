// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `rvb` — minimal **Rust control plane** over the **mkosi** CLI (image engine).
//! See workspace `rvb/templates/catalog.yaml` for the builtin template catalog.

mod artifact;
mod catalog;
mod mkosi_workspace;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use crate::catalog::Template;
use crate::mkosi_workspace::OutputShape;

#[derive(Parser)]
#[command(
    name = "rvb",
    version,
    about = "Virt-builder-style UX over mkosi (prototype CLI)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List builtin template ids
    List,
    /// Show one template (YAML)
    Show {
        /// Template id (e.g. fedora-42)
        id: String,
    },
    /// Generate mkosi.conf, run `mkosi build`, optional qcow2 convert, emit checksum + buildinfo
    Build {
        /// Template id from catalog
        template: String,
        /// Output image path (.qcow2 or .raw)
        #[arg(short, long)]
        output: PathBuf,
        /// `qcow2` (default) or `raw`
        #[arg(long, default_value = "qcow2")]
        format: String,
        /// Skip .sha256 and .buildinfo.json next to the image
        #[arg(long, default_value_t = false)]
        no_metadata: bool,
    },
}

fn print_catalog_table(templates: &HashMap<String, Template>) {
    let mut ids: Vec<_> = templates.keys().cloned().collect();
    ids.sort();
    println!("{:20} {:12} {:12} FORMAT", "ID", "DISTRO", "RELEASE");
    for id in ids {
        let t = &templates[&id];
        println!(
            "{:20} {:12} {:12} {}",
            t.id, t.distribution, t.release, t.format
        );
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let catalog = catalog::load_builtin_catalog().context("load catalog")?;

    match cli.command {
        Commands::List => {
            print_catalog_table(&catalog);
        }
        Commands::Show { id } => {
            let t = catalog::get(&catalog, &id)?;
            println!(
                "{}",
                serde_yaml::to_string(&t).context("serialize template")?
            );
        }
        Commands::Build {
            template,
            output,
            format,
            no_metadata,
        } => {
            let t = catalog::get(&catalog, &template)?;
            let shape = match format.to_lowercase().as_str() {
                "qcow2" => OutputShape::Qcow2,
                "raw" => OutputShape::Raw,
                other => anyhow::bail!("unknown --format {other} (use qcow2 or raw)"),
            };
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("mkdir {}", parent.display()))?;
            }
            info!(template = %t.id, dest = %output.display(), "mkosi build");
            mkosi_workspace::build_catalog_template(&t, &output, shape, &t.id)
                .context("mkosi build pipeline")?;
            if !no_metadata {
                artifact::write_sidecars(&output, &t.id).context("write checksum / buildinfo")?;
            }
            println!("Wrote {}", output.display());
        }
    }

    Ok(())
}

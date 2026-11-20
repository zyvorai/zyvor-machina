// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! CLI for [`virt_image_build`] — see crate `lib.rs` for programmatic use.

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use virt_image_build::{
    build_disk_image, check_virt_builder, list_templates_text, BuildDiskRequest,
};

#[derive(Parser, Debug)]
#[command(name = "virt-image-build")]
#[command(
    about = "Build VM disk images with virt-builder (ISO-less images for virt-install --import)."
)]
struct Cli {
    #[arg(short, long, default_value = "fedora-39")]
    os: String,

    #[arg(short, long, default_value = "vm-image.qcow2")]
    output: String,

    #[arg(short, long, default_value = "20G")]
    size: String,

    #[arg(long, default_value = "qcow2")]
    format: String,

    #[arg(long, value_name = "PATH")]
    root_password_file: Option<PathBuf>,

    #[arg(long, default_value = "virtbuilder-guest.local")]
    hostname: String,

    #[arg(long, default_value = "")]
    install: String,

    #[arg(long = "run-command", action = clap::ArgAction::Append)]
    run_command: Vec<String>,

    #[arg(long, value_name = "PATH")]
    firstboot_script: Option<PathBuf>,

    #[arg(long = "copy-in", action = clap::ArgAction::Append)]
    copy_in: Vec<String>,

    #[arg(long, value_name = "PATH")]
    ssh_pubkey_file: Option<PathBuf>,

    #[arg(long)]
    list_templates: bool,

    #[arg(long)]
    check: bool,

    #[arg(long)]
    update: bool,

    #[arg(long)]
    selinux_relabel: bool,

    #[arg(last = true)]
    extra: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    check_virt_builder()?;

    if cli.check {
        println!("virt-builder is available.");
        return Ok(());
    }

    if cli.list_templates {
        print!("{}", list_templates_text()?);
        return Ok(());
    }

    let req = BuildDiskRequest {
        os: cli.os,
        output: cli.output.clone(),
        size: cli.size,
        format: cli.format,
        hostname: cli.hostname,
        install: cli.install,
        run_command: cli.run_command,
        copy_in: cli.copy_in,
        firstboot_script: cli
            .firstboot_script
            .map(|p| p.to_string_lossy().into_owned()),
        root_password_file: cli
            .root_password_file
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        root_password_inline: None,
        ssh_pubkey_file: cli
            .ssh_pubkey_file
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        ssh_pubkey_inline: None,
        update: cli.update,
        selinux_relabel: cli.selinux_relabel,
        extra_virt_builder_args: cli.extra,
        timeout_secs: 0,
    };

    eprintln!(
        "virt-builder {} → {} (size {}, format {})",
        req.os, req.output, req.size, req.format
    );

    build_disk_image(&req).context("build_disk_image")?;

    println!("OK: {}", cli.output);
    Ok(())
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Checksums + minimal build metadata JSON next to the image.

use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
pub struct BuildInfo<'a> {
    pub template: &'a str,
    pub output: String,
    pub sha256: String,
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>())
}

pub fn write_sidecars(output: &Path, template_id: &str) -> Result<()> {
    let sum = sha256_file(output)?;
    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let parent = output.parent();
    let sha_path = parent
        .map(|p| p.join(format!("{stem}.sha256")))
        .unwrap_or_else(|| output.with_extension("sha256"));
    let meta_path = parent
        .map(|p| p.join(format!("{stem}.buildinfo.json")))
        .unwrap_or_else(|| output.with_extension("buildinfo.json"));

    let fname = output
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    fs::write(&sha_path, format!("{}  {fname}\n", sum))
        .with_context(|| format!("write {}", sha_path.display()))?;

    let info = BuildInfo {
        template: template_id,
        output: output.display().to_string(),
        sha256: sum,
    };
    fs::write(
        &meta_path,
        serde_json::to_string_pretty(&info).context("serialize buildinfo")?,
    )
    .with_context(|| format!("write {}", meta_path.display()))?;
    Ok(())
}

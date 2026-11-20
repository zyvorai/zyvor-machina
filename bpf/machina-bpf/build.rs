// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Builds `machina-bpf-ebpf` for bpfel-unknown-none and stages the object at
// `$OUT_DIR/machina-bpf.o` for `include_bytes_aligned!`.
//
// The kernel crate needs nightly + `bpf-linker`. When they are missing (or on
// non-Linux hosts, or with MACHINA_BPF_SKIP=1) an empty object is staged and
// the datapath reports `programs_compiled: false` at runtime instead of
// failing the whole workspace build. MACHINA_BPF_OBJ=/path/to/obj stages a
// prebuilt object.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn find_tool(name: &str) -> Option<PathBuf> {
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let home = env::var_os("HOME")?;
    let p = Path::new(&home).join(".cargo/bin").join(name);
    p.is_file().then_some(p)
}

fn stage_empty(dst: &Path, why: &str) {
    println!(
        "cargo:warning=machina-bpf: eBPF programs not built ({why}); datapath will be unavailable"
    );
    fs::write(dst, b"").expect("write empty bpf object");
}

/// Cargo treats a missing `rerun-if-changed` path as always changed, so watching
/// where the tool would be installed keeps re-running this script until it
/// appears instead of caching the empty object forever.
fn watch_install_paths(name: &str) {
    let mut dirs: Vec<PathBuf> = env::var_os("PATH")
        .map(|p| env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = env::var_os("HOME") {
        dirs.push(Path::new(&home).join(".cargo/bin"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    for d in dirs {
        println!("cargo:rerun-if-changed={}", d.join(name).display());
    }
}

fn toolchain_dir(toolchain: &str, arch: &str) -> Option<PathBuf> {
    let home = env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| Path::new(&h).join(".rustup")))?;
    Some(
        home.join("toolchains")
            .join(format!("{toolchain}-{arch}-unknown-linux-gnu")),
    )
}

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let dst = out.join("machina-bpf.o");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let ebpf_dir = manifest.join("../machina-bpf-ebpf");
    let common_dir = manifest.join("../machina-bpf-common");

    for v in [
        "MACHINA_BPF_OBJ",
        "MACHINA_BPF_SKIP",
        "MACHINA_BPF_TOOLCHAIN",
    ] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    for d in [
        ebpf_dir.join("src"),
        ebpf_dir.join("Cargo.toml"),
        common_dir.join("src"),
    ] {
        println!("cargo:rerun-if-changed={}", d.display());
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        fs::write(&dst, b"").expect("write empty bpf object");
        return;
    }
    if let Some(obj) = env::var_os("MACHINA_BPF_OBJ") {
        fs::copy(&obj, &dst).expect("copy MACHINA_BPF_OBJ");
        return;
    }
    if matches!(
        env::var("MACHINA_BPF_SKIP").as_deref(),
        Ok("1") | Ok("true")
    ) {
        stage_empty(&dst, "MACHINA_BPF_SKIP set");
        return;
    }
    let toolchain = env::var("MACHINA_BPF_TOOLCHAIN").unwrap_or_else(|_| "nightly".into());
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".into());
    let Some(_linker) = find_tool("bpf-linker") else {
        watch_install_paths("bpf-linker");
        stage_empty(&dst, "bpf-linker not found; run `make bpf-deps`");
        return;
    };
    let Some(rustup) = find_tool("rustup") else {
        watch_install_paths("rustup");
        stage_empty(&dst, "rustup not found (nightly toolchain required)");
        return;
    };
    let target_dir = out.join("ebpf-target");

    let mut rustflags = String::new();
    for s in [
        &format!("--cfg=bpf_target_arch=\"{arch}\""),
        "-Cdebuginfo=2",
        "-Clink-arg=--btf",
    ] {
        if !rustflags.is_empty() {
            rustflags.push('\x1f');
        }
        rustflags.push_str(s);
    }

    let mut cmd = Command::new(&rustup);
    cmd.current_dir(&ebpf_dir)
        .args(["run", &toolchain, "cargo", "build", "--release"])
        .args(["--target", "bpfel-unknown-none", "-Z", "build-std=core"])
        .arg("--target-dir")
        .arg(&target_dir)
        .env("CARGO_ENCODED_RUSTFLAGS", rustflags);
    for k in [
        "RUSTC",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTC_WRAPPER",
        "RUSTFLAGS",
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET",
        "CARGO_MANIFEST_DIR",
        "CARGO_PKG_NAME",
    ] {
        cmd.env_remove(k);
    }
    let output = cmd
        .output()
        .expect("spawn rustup cargo build for machina-bpf-ebpf");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("toolchain") && stderr.contains("not installed") {
            if let Some(dir) = toolchain_dir(&toolchain, &arch) {
                println!("cargo:rerun-if-changed={}", dir.display());
            }
            stage_empty(
                &dst,
                &format!("rustup toolchain `{toolchain}` not installed; run `make bpf-deps`"),
            );
            return;
        }
        panic!("machina-bpf-ebpf build failed:\n{stderr}");
    }
    let built = target_dir.join("bpfel-unknown-none/release/machina-bpf");
    fs::copy(&built, &dst).unwrap_or_else(|e| panic!("copy {}: {e}", built.display()));
}

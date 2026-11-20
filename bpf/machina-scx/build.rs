// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

fn main() {
    #[cfg(feature = "scx")]
    scx::build();
}

#[cfg(feature = "scx")]
mod scx {
    use std::path::PathBuf;
    use std::process::Command;

    const SRC: &str = "src/bpf/scx_machina.bpf.c";

    pub fn build() {
        println!("cargo:rerun-if-changed={SRC}");
        println!("cargo:rerun-if-changed=include");
        println!("cargo:rerun-if-env-changed=MACHINA_SCX_BTF");
        let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
        // sched_ext has no stable ABI: compile against the build host's kernel BTF
        // (or MACHINA_SCX_BTF for the target kernel's).
        let btf =
            std::env::var("MACHINA_SCX_BTF").unwrap_or_else(|_| "/sys/kernel/btf/vmlinux".into());
        let dump = Command::new("bpftool")
            .args(["btf", "dump", "file", &btf, "format", "c"])
            .output()
            .expect("bpftool is required to build machina-scx (feature scx)");
        assert!(
            dump.status.success(),
            "bpftool btf dump {btf}: {}",
            String::from_utf8_lossy(&dump.stderr)
        );
        std::fs::write(out.join("vmlinux.h"), dump.stdout).expect("write vmlinux.h");
        let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("x86_64") => "x86",
            Ok("aarch64") => "arm64",
            Ok("riscv64") => "riscv",
            Ok("s390x") => "s390",
            Ok("powerpc64") => "powerpc",
            other => panic!("unsupported sched_ext target architecture {other:?}"),
        };
        let include = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
            .join("include");
        libbpf_cargo::SkeletonBuilder::new()
            .source(SRC)
            .clang_args([
                format!("-I{}", out.display()),
                format!("-I{}", include.display()),
                format!("-D__TARGET_ARCH_{arch}"),
                "-Wall".into(),
                "-Wno-unused-parameter".into(),
            ])
            .build_and_generate(out.join("scx_machina.skel.rs"))
            .expect("build scx_machina.bpf.c");
    }
}

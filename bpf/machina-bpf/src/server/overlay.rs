// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The WireGuard overlay (`machina-wg`, see `netpol::overlay`). The private
//! key stays in the state directory (0600) and goes to the kernel over
//! generic netlink, never through `wg` (distro AppArmor profiles only let
//! `wg` read `/etc/wireguard`); the controller only ever sees the public key. Disabling removes the
//! interface, its routes, the forward rules and the nftables tables.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use base64::Engine as _;

use crate::netpol::overlay::{self as ov, IFACE};

use super::*;

#[derive(Default)]
pub(super) struct OverlayRuntime {
    pub config: VmOverlay,
    pub dir: Option<PathBuf>,
    public_key: String,
    error: Option<String>,
    up: bool,
    /// `unreachable` routes installed for the fleet and own prefixes.
    unreachable: Vec<String>,
}

fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("run {cmd}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "{cmd} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run_stdin(cmd: &str, args: &[&str], input: &str) -> Result<String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("run {cmd}"))?;
    child
        .stdin
        .take()
        .context("stdin")?
        .write_all(input.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(anyhow!(
            "{cmd}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `WG_CMD_SET_DEVICE` with `WGDEVICE_A_PRIVATE_KEY`.
#[cfg(target_os = "linux")]
fn set_private_key(ifname: &str, key: &[u8; 32]) -> Result<()> {
    const GENL_ID_CTRL: u16 = 0x10;
    const CTRL_CMD_GETFAMILY: u8 = 3;
    const CTRL_ATTR_FAMILY_ID: u16 = 1;
    const CTRL_ATTR_FAMILY_NAME: u16 = 2;
    const WG_CMD_SET_DEVICE: u8 = 1;
    const WGDEVICE_A_IFNAME: u16 = 2;
    const WGDEVICE_A_PRIVATE_KEY: u16 = 3;
    const NLM_F_REQUEST: u16 = 1;
    const NLM_F_ACK: u16 = 4;
    const NLMSG_ERROR: u16 = 2;

    fn attr(buf: &mut Vec<u8>, ty: u16, val: &[u8]) {
        buf.extend_from_slice(&((4 + val.len()) as u16).to_ne_bytes());
        buf.extend_from_slice(&ty.to_ne_bytes());
        buf.extend_from_slice(val);
        buf.resize(buf.len().next_multiple_of(4), 0);
    }
    fn msg(ty: u16, flags: u16, seq: u32, cmd: u8, attrs: &[u8]) -> Vec<u8> {
        let mut m = Vec::with_capacity(20 + attrs.len());
        m.extend_from_slice(&((20 + attrs.len()) as u32).to_ne_bytes());
        m.extend_from_slice(&ty.to_ne_bytes());
        m.extend_from_slice(&flags.to_ne_bytes());
        m.extend_from_slice(&seq.to_ne_bytes());
        m.extend_from_slice(&0u32.to_ne_bytes());
        m.extend_from_slice(&[cmd, 1, 0, 0]);
        m.extend_from_slice(attrs);
        m
    }
    struct Fd(i32);
    impl Drop for Fd {
        fn drop(&mut self) {
            unsafe { libc::close(self.0) };
        }
    }
    let fd = Fd(unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::NETLINK_GENERIC,
        )
    });
    if fd.0 < 0 {
        return Err(anyhow!(
            "netlink socket: {}",
            std::io::Error::last_os_error()
        ));
    }
    let roundtrip = |m: &[u8]| -> Result<Vec<u8>> {
        let mut sa: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        sa.nl_family = libc::AF_NETLINK as u16;
        let n = unsafe {
            libc::sendto(
                fd.0,
                m.as_ptr().cast(),
                m.len(),
                0,
                (&sa as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as u32,
            )
        };
        if n < 0 {
            return Err(anyhow!("netlink send: {}", std::io::Error::last_os_error()));
        }
        let mut buf = vec![0u8; 8192];
        let n = unsafe { libc::recv(fd.0, buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n < 20 {
            return Err(anyhow!("netlink recv: {}", std::io::Error::last_os_error()));
        }
        buf.truncate(n as usize);
        if u16::from_ne_bytes([buf[4], buf[5]]) == NLMSG_ERROR {
            let err = i32::from_ne_bytes([buf[16], buf[17], buf[18], buf[19]]);
            if err != 0 {
                return Err(anyhow!("{}", std::io::Error::from_raw_os_error(-err)));
            }
        }
        Ok(buf)
    };

    let mut a = Vec::new();
    attr(&mut a, CTRL_ATTR_FAMILY_NAME, b"wireguard\0");
    let resp = roundtrip(&msg(GENL_ID_CTRL, NLM_F_REQUEST, 1, CTRL_CMD_GETFAMILY, &a))
        .context("wireguard netlink family (is the wireguard module loaded?)")?;
    let mut off = 20;
    let mut family = None;
    while off + 4 <= resp.len() {
        let len = u16::from_ne_bytes([resp[off], resp[off + 1]]) as usize;
        let ty = u16::from_ne_bytes([resp[off + 2], resp[off + 3]]);
        if len < 4 || off + len > resp.len() {
            break;
        }
        if ty == CTRL_ATTR_FAMILY_ID && len >= 6 {
            family = Some(u16::from_ne_bytes([resp[off + 4], resp[off + 5]]));
        }
        off += len.next_multiple_of(4);
    }
    let family = family.context("wireguard netlink family id missing")?;

    let mut name = ifname.as_bytes().to_vec();
    name.push(0);
    let mut a = Vec::new();
    attr(&mut a, WGDEVICE_A_IFNAME, &name);
    attr(&mut a, WGDEVICE_A_PRIVATE_KEY, key);
    roundtrip(&msg(
        family,
        NLM_F_REQUEST | NLM_F_ACK,
        2,
        WG_CMD_SET_DEVICE,
        &a,
    ))
    .context("set the WireGuard private key")?;
    a.fill(0);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn set_private_key(_ifname: &str, _key: &[u8; 32]) -> Result<()> {
    Err(anyhow!("WireGuard needs Linux"))
}

fn iface_exists() -> bool {
    std::path::Path::new(&format!("/sys/class/net/{IFACE}")).exists()
}

/// `-I FORWARD -i machina-wg -m conntrack --ctstate DNAT -j ACCEPT`, in
/// iptables and ip6tables: libvirt rejects new connections into its NAT
/// networks, and only mapped (DNATed) overlay traffic may pass.
fn forward_rule(tool: &str, add: bool) {
    if Command::new(tool).arg("--version").output().is_err() {
        return;
    }
    let rule = [
        "FORWARD",
        "-i",
        IFACE,
        "-m",
        "conntrack",
        "--ctstate",
        "DNAT",
        "-m",
        "comment",
        "--comment",
        "machina-overlay",
        "-j",
        "ACCEPT",
    ];
    let present = Command::new(tool)
        .arg("-C")
        .args(rule)
        .output()
        .is_ok_and(|o| o.status.success());
    let r = match (add, present) {
        (true, false) => run(tool, &[&["-I"][..], &rule[..]].concat()),
        (false, true) => run(tool, &[&["-D"][..], &rule[..]].concat()),
        _ => Ok(String::new()),
    };
    if let Err(e) = r {
        tracing::warn!("overlay forward rule: {e:#}");
    }
}

impl OverlayRuntime {
    fn key_path(&self) -> Result<PathBuf> {
        let d = self.dir.clone().context("no state directory")?;
        Ok(d.join("wg.key"))
    }

    /// The private key file, created on first use.
    fn ensure_key(&mut self) -> Result<PathBuf> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let p = self.key_path()?;
        if !p.exists() {
            let dir = p.parent().context("key dir")?;
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            let mut k = [0u8; 32];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut k))
                .context("read /dev/urandom")?;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&p)?;
            f.write_all(ov::private_key(k).as_bytes())?;
        }
        if self.public_key.is_empty() {
            let private = std::fs::read_to_string(&p)?;
            self.public_key = run_stdin("wg", &["pubkey"], private.trim())?
                .trim()
                .to_string();
        }
        Ok(p)
    }

    fn apply(&mut self, cfg: &VmOverlay) -> Result<()> {
        let key = self.ensure_key()?;
        if !iface_exists() {
            run("ip", &["link", "add", IFACE, "type", "wireguard"])?;
        }
        let mut raw = [0u8; 32];
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(std::fs::read_to_string(&key)?.trim())
            .ok()
            .filter(|k| k.len() == 32)
            .context("overlay key file is not a WireGuard key")?;
        raw.copy_from_slice(&decoded);
        let set = set_private_key(IFACE, &raw);
        raw.fill(0);
        set?;
        run(
            "wg",
            &["set", IFACE, "listen-port", &cfg.listen_port.to_string()],
        )?;
        run("ip", &["link", "set", IFACE, "mtu", "1420", "up"])?;

        let mut own: BTreeSet<String> = BTreeSet::new();
        for p in &cfg.prefixes {
            if let Some(a) = ov::addr_in(p, 1) {
                own.insert(format!("{a}/{}", if a.is_ipv4() { 32 } else { 128 }));
            }
        }
        let have = run("ip", &["-o", "addr", "show", "dev", IFACE])?;
        for l in have.lines() {
            let mut it = l.split_whitespace();
            if it.any(|w| w == "inet" || w == "inet6") {
                if let Some(a) = it.next() {
                    if !own.contains(a) && !a.starts_with("fe80:") {
                        let _ = run("ip", &["addr", "del", a, "dev", IFACE]);
                    }
                }
            }
        }
        for a in &own {
            if !have.contains(&format!(" {a} ")) {
                let mut args = vec!["addr", "add", a.as_str(), "dev", IFACE];
                if a.contains(':') {
                    args.push("nodad");
                }
                run("ip", &args)?;
            }
        }

        let wanted: BTreeSet<&str> = cfg.peers.iter().map(|p| p.public_key.as_str()).collect();
        for p in ov::parse_dump(&run("wg", &["show", IFACE, "dump"])?) {
            if !wanted.contains(p.public_key.as_str()) {
                run("wg", &["set", IFACE, "peer", &p.public_key, "remove"])?;
            }
        }
        for p in &cfg.peers {
            run(
                "wg",
                &[
                    "set",
                    IFACE,
                    "peer",
                    &p.public_key,
                    "endpoint",
                    &p.endpoint,
                    "allowed-ips",
                    &p.prefixes.join(","),
                    "persistent-keepalive",
                    "25",
                ],
            )?;
        }

        let routes: BTreeSet<String> = cfg
            .peers
            .iter()
            .flat_map(|p| if p.routes.is_empty() { p.prefixes.clone() } else { p.routes.clone() })
            .collect();
        for fam in ["-4", "-6"] {
            for r in ov::parse_routes(&run("ip", &[fam, "route", "show", "dev", IFACE])?) {
                let keep = routes.contains(&r)
                    || own.iter().any(|o| o.split('/').next() == Some(r.as_str()));
                if !keep && !r.starts_with("fe80:") {
                    let _ = run("ip", &[fam, "route", "del", &r, "dev", IFACE]);
                }
            }
        }
        for r in &routes {
            let fam = if r.contains(':') { "-6" } else { "-4" };
            run("ip", &[fam, "route", "replace", r, "dev", IFACE])?;
        }
        let blackhole: Vec<String> = cfg
            .fleet_prefixes
            .iter()
            .chain(&cfg.prefixes)
            .cloned()
            .collect();
        for r in std::mem::take(&mut self.unreachable) {
            if !blackhole.contains(&r) {
                let fam = if r.contains(':') { "-6" } else { "-4" };
            let _ = run("ip", &[fam, "route", "del", "unreachable", &r]);
            }
        }
        for r in &blackhole {
            let fam = if r.contains(':') { "-6" } else { "-4" };
            run("ip", &[fam, "route", "replace", "unreachable", r])?;
        }
        self.unreachable = blackhole;

        forward_rule("iptables", true);
        forward_rule("ip6tables", true);
        let mut child = Command::new("nft")
            .args(["-f", "-"])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("run nft")?;
        child
            .stdin
            .take()
            .context("nft stdin")?
            .write_all(ov::transaction(ov::render(cfg).as_deref()).as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            return Err(anyhow!(
                "nft: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }

    fn teardown(&mut self) {
        for r in std::mem::take(&mut self.unreachable) {
            let fam = if r.contains(':') { "-6" } else { "-4" };
            let _ = run("ip", &[fam, "route", "del", "unreachable", &r]);
        }
        if iface_exists() {
            if let Err(e) = run("ip", &["link", "del", IFACE]) {
                tracing::warn!("overlay: {e:#}");
            }
        }
        forward_rule("iptables", false);
        forward_rule("ip6tables", false);
        let _ = run_stdin("nft", &["-f", "-"], &ov::transaction(None));
    }
}

impl Engine {
    pub(super) fn vm_overlay_set(&mut self, cfg: VmOverlay) -> Result<VmOverlayStatus> {
        ov::validate(&cfg).map_err(|e| anyhow!(e))?;
        let o = &mut self.overlay;
        let res = if cfg.enabled {
            o.apply(&cfg)
        } else {
            if o.up || iface_exists() {
                o.teardown();
            }
            Ok(())
        };
        if let Err(e) = &res {
            tracing::warn!("overlay: {e:#}");
        } else if cfg.enabled {
            tracing::info!(
                peers = cfg.peers.len(),
                mappings = cfg.mappings.len(),
                "overlay applied"
            );
        }
        o.up = cfg.enabled && res.is_ok();
        o.error = res.err().map(|e| format!("{e:#}"));
        o.config = cfg;
        Ok(self.vm_overlay_status())
    }

    pub(super) fn vm_overlay_status(&self) -> VmOverlayStatus {
        let o = &self.overlay;
        let mut peers = if o.config.enabled && iface_exists() {
            run("wg", &["show", IFACE, "dump"])
                .map(|d| ov::parse_dump(&d))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        for p in &mut peers {
            if let Some(c) = o.config.peers.iter().find(|c| c.public_key == p.public_key) {
                p.host = c.host.clone();
            }
        }
        VmOverlayStatus {
            enabled: o.config.enabled,
            interface: if o.up { IFACE.into() } else { String::new() },
            public_key: o.public_key.clone(),
            listen_port: o.config.listen_port,
            prefixes: o.config.prefixes.clone(),
            mappings: o.config.mappings.clone(),
            peers,
            error: o.error.clone(),
            hostname: None,
        }
    }
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `machina-agent join` receives the fleet's shared agent token from the controller (answering a
//! valid single-use enrollment token) and stores it, so adding a host is one command with no
//! secret copied by hand.

use std::path::Path;

/// Default file the packaged services read their environment from.
pub const DEFAULT_ENV_FILE: &str = "/etc/default/machina-platform";

/// True when the join URL protects the token on the wire: https, or a loopback host (an SSH
/// tunnel or the controller on the same machine).
pub fn channel_is_safe(controller: &str) -> bool {
    if controller.starts_with("https://") {
        return true;
    }
    let Some(rest) = controller.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', '?']).next().unwrap_or("");
    let host = if let Some(v6) = host.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// A token is stored only if it is a single printable word, so it can never inject another line
/// into the environment file.
pub fn token_is_valid(token: &str) -> bool {
    !token.is_empty() && token.len() <= 512 && token.bytes().all(|b| b.is_ascii_graphic())
}

/// Replaces (or appends) `MACHINA_AGENT_TOKEN=` in `path` and leaves the file mode 0600.
/// Returns true when the file changed. The old file is kept next to it as `<name>.bak-pre-join`.
pub fn store_agent_token(path: &Path, token: &str) -> std::io::Result<bool> {
    set_env_vars(path, &[("MACHINA_AGENT_TOKEN", token)])
}

/// Sets `KEY=value` lines in an environment file (replacing existing ones, appending new ones),
/// mode 0600, previous file kept as `<name>.bak-pre-join`. Values must be one printable word.
/// Returns true when the file changed.
pub fn set_env_vars(path: &Path, vars: &[(&str, &str)]) -> std::io::Result<bool> {
    for (k, v) in vars {
        let key_ok = !k.is_empty()
            && k.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        if !key_ok || !token_is_valid(v) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "environment entry is empty or has characters not allowed in an environment file",
            ));
        }
    }
    let old = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    if vars
        .iter()
        .all(|(k, v)| old.lines().any(|l| l == format!("{k}={v}")))
    {
        return Ok(false);
    }
    let mut out: String = old
        .lines()
        .filter(|l| !vars.iter().any(|(k, _)| l.starts_with(&format!("{k}="))))
        .map(|l| format!("{l}\n"))
        .collect();
    for (k, v) in vars {
        out.push_str(&format!("{k}={v}\n"));
    }
    if !old.is_empty() {
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak-pre-join");
        std::fs::write(&bak, &old)?;
        set_private(Path::new(&bak))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".join-tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, out)?;
    set_private(&tmp)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// The machine's own hostname: the explicit value if non-empty, else the kernel's, else the
/// environment, else "localhost". (`$HOSTNAME` is a shell variable that is usually not exported,
/// so reading only the environment registered every host as "localhost".)
pub fn local_hostname(explicit: Option<&str>) -> String {
    let clean = |s: &str| s.trim().to_string();
    if let Some(h) = explicit.map(clean).filter(|h| !h.is_empty()) {
        return h;
    }
    if let Ok(h) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let h = clean(&h);
        if !h.is_empty() {
            return h;
        }
    }
    for var in ["HOSTNAME", "HOST"] {
        if let Some(h) = std::env::var(var)
            .ok()
            .map(|s| clean(&s))
            .filter(|h| !h.is_empty())
        {
            return h;
        }
    }
    "localhost".into()
}

/// The address of this machine that routes to `controller` (what the controller can use to reach
/// it), found by asking the kernel which source address it would use. None for loopback targets
/// or when it cannot be determined.
pub fn address_towards(controller: &str) -> Option<String> {
    let rest = controller
        .strip_prefix("https://")
        .or_else(|| controller.strip_prefix("http://"))?;
    let hostport = rest.split(['/', '?']).next()?;
    let target = if hostport.contains(':') && !hostport.starts_with('[') {
        hostport.to_string()
    } else if hostport.starts_with('[') {
        hostport.to_string()
    } else {
        format!("{hostport}:443")
    };
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect(target).ok()?;
    let ip = sock.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

/// The `host:port` the controller should dial: a wildcard listen address is replaced with the
/// machine's address; a concrete one is kept.
pub fn advertised(listen: &str, address: &str) -> String {
    match listen.rsplit_once(':') {
        Some((h, port)) if h == "0.0.0.0" || h == "[::]" || h == "::" || h.is_empty() => {
            format!("{address}:{port}")
        }
        _ => listen.to_string(),
    }
}

/// Fetches the controller's CA over TLS that is NOT verified (the CA is what we do not have yet)
/// and accepts it only if its SHA-256 fingerprint is the one the operator pinned.
pub async fn fetch_pinned_ca(controller: &str, pinned: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let v: serde_json::Value = client
        .get(format!(
            "{}/api/v1/pki/ca",
            controller.trim_end_matches('/')
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let pem = v["ca_pem"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("the controller sent no CA certificate"))?
        .to_string();
    if !machina_bpf::authca::fingerprint_matches(&pem, pinned) {
        anyhow::bail!(
            "the CA offered by {controller} does not match the pinned fingerprint (--ca-sha256): \
             this is not the controller you meant, or the command is stale"
        );
    }
    Ok(pem)
}

/// Writes the node's key (0600), certificate and the fleet CA into `dir` (0700); returns their paths.
pub fn write_pki(
    dir: &Path,
    key_pem: &str,
    cert_pem: &str,
    ca_pem: &str,
) -> std::io::Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let (k, c, a) = (
        dir.join("agent.key"),
        dir.join("agent.pem"),
        dir.join("ca.pem"),
    );
    std::fs::write(&k, key_pem)?;
    set_private(&k)?;
    std::fs::write(&c, cert_pem)?;
    std::fs::write(&a, ca_pem)?;
    Ok((k, c, a))
}

#[cfg(unix)]
fn set_private(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
fn set_private(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_loopback_carry_the_token() {
        assert!(channel_is_safe("https://ctl.example:5093"));
        assert!(channel_is_safe("http://127.0.0.1:15093"));
        assert!(channel_is_safe("http://localhost:5093/x"));
        assert!(channel_is_safe("http://[::1]:5093"));
        assert!(!channel_is_safe("http://10.0.0.5:5093"));
        assert!(!channel_is_safe("http://127.0.0.1.evil.example"));
        assert!(!channel_is_safe("ctl:5093"));
    }

    #[test]
    fn sets_several_variables_at_once_and_is_idempotent() {
        let d = std::env::temp_dir().join(format!("machina-enrol-multi-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("platform");
        std::fs::write(&f, "A=1\nMACHINA_AGENT_LISTEN=127.0.0.1:50051\n").unwrap();
        let vars = [
            ("MACHINA_AGENT_LISTEN", "10.1.2.3:50051"),
            ("MACHINA_AGENT_CONSOLE_LISTEN", "10.1.2.3:50052"),
        ];
        assert!(set_env_vars(&f, &vars).unwrap());
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "A=1\nMACHINA_AGENT_LISTEN=10.1.2.3:50051\nMACHINA_AGENT_CONSOLE_LISTEN=10.1.2.3:50052\n"
        );
        assert!(!set_env_vars(&f, &vars).unwrap());
        assert!(set_env_vars(&f, &[("bad key", "x")]).is_err());
        assert!(set_env_vars(&f, &[("OK", "two words")]).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn advertises_the_machine_address_for_wildcard_listeners() {
        assert_eq!(advertised("0.0.0.0:50051", "10.1.2.3"), "10.1.2.3:50051");
        assert_eq!(advertised("[::]:50051", "10.1.2.3"), "10.1.2.3:50051");
        assert_eq!(advertised("10.9.9.9:50051", "10.1.2.3"), "10.9.9.9:50051");
        assert_eq!(advertised("127.0.0.1:50051", "10.1.2.3"), "127.0.0.1:50051");
    }

    #[test]
    fn hostname_prefers_a_non_empty_explicit_value_and_never_returns_empty() {
        assert_eq!(local_hostname(Some("  node-7 ")), "node-7");
        assert!(!local_hostname(Some("")).is_empty());
        assert!(!local_hostname(None).is_empty());
    }

    #[test]
    fn loopback_controllers_have_no_routable_address() {
        assert_eq!(address_towards("http://127.0.0.1:5093"), None);
        assert_eq!(address_towards("not a url"), None);
    }

    #[test]
    fn the_pki_files_are_written_with_a_private_key() {
        let d = std::env::temp_dir().join(format!("machina-pki-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (k, c, a) = write_pki(&d, "KEY", "CERT", "CA").unwrap();
        assert_eq!(std::fs::read_to_string(&k).unwrap(), "KEY");
        assert_eq!(std::fs::read_to_string(&c).unwrap(), "CERT");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "CA");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&k).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(&d).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_values_that_could_add_lines() {
        assert!(!token_is_valid(""));
        assert!(!token_is_valid("a\nB=1"));
        assert!(!token_is_valid("a b"));
        assert!(token_is_valid("0123abcDEF-_=="));
    }

    #[test]
    fn replaces_the_line_keeps_the_rest_and_the_mode() {
        let d = std::env::temp_dir().join(format!("machina-enrol-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("platform");
        std::fs::write(&f, "A=1\nMACHINA_AGENT_TOKEN=old\nB=2\n").unwrap();
        assert!(store_agent_token(&f, "new").unwrap());
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "A=1\nB=2\nMACHINA_AGENT_TOKEN=new\n"
        );
        assert!(!store_agent_token(&f, "new").unwrap());
        assert_eq!(
            std::fs::read_to_string(d.join("platform.bak-pre-join")).unwrap(),
            "A=1\nMACHINA_AGENT_TOKEN=old\nB=2\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(store_agent_token(&f, "bad\nX=1").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}

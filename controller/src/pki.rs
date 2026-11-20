// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The fleet's transport PKI. The controller keeps a CA (separate from the VM network policy CA),
//! signs one certificate per node when it joins (the node keeps its key and sends a CSR), and has
//! a client certificate of its own, so controller and agents authenticate each other with mutual
//! TLS. A node certificate names `agent.machina` (what the controller verifies) and the node's
//! own host name.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use machina_bpf::authca::{self, Ca};

/// The name every agent certificate carries and the controller checks.
pub const AGENT_DOMAIN: &str = "agent.machina";
const CONTROLLER_NAME: &str = "controller.machina";
/// Node and controller certificates live this long; they are reissued at startup past 60 days.
pub const NODE_CERT_SECS: i64 = 90 * 24 * 3600;
const REISSUE_AFTER: Duration = Duration::from_secs(60 * 24 * 3600);

#[derive(Clone)]
pub struct Identity {
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_pem: String,
}

pub fn dir() -> PathBuf {
    std::env::var_os("MACHINA_FLEET_CA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/machina/fleet-ca"))
}

pub fn ca_in(dir: &Path) -> Result<Ca> {
    Ca::load_or_create_named(dir, "machina fleet CA")
}

static CA: Mutex<Option<Arc<Ca>>> = Mutex::new(None);

pub fn ca() -> Result<Arc<Ca>> {
    let mut g = CA.lock().unwrap();
    if let Some(c) = g.as_ref() {
        return Ok(c.clone());
    }
    let c = Arc::new(ca_in(&dir()).context("fleet CA")?);
    *g = Some(c.clone());
    Ok(c)
}

/// The CA certificate and its SHA-256 fingerprint (what a joining node pins).
pub fn ca_info() -> Result<(String, String)> {
    let c = ca()?;
    Ok((c.cert_pem.clone(), authca::cert_sha256(&c.cert_pem)?))
}

/// An agent certificate for `csr_pem`: valid for `agent.machina` and the host's own name.
pub fn sign_agent(csr_pem: &str, host_id: &str) -> Result<(String, i64)> {
    ca()?.sign_csr(
        csr_pem,
        &format!("machina agent {host_id}"),
        &[AGENT_DOMAIN.to_string(), authca::host_dns(host_id)],
        NODE_CERT_SECS,
    )
}

/// `<base>.pem` / `<base>.key` in `dir`, issued by the fleet CA for `sans` and reused until the
/// names change or it is 60 days old (the key is generated here and never leaves the controller).
fn load_or_issue_in(
    dir: &Path,
    ca: &Ca,
    base: &str,
    cn: &str,
    sans: &[String],
) -> Result<Identity> {
    let (cp, kp, sp) = (
        dir.join(format!("{base}.pem")),
        dir.join(format!("{base}.key")),
        dir.join(format!("{base}.sans")),
    );
    let want = sans.join(",");
    let fresh = std::fs::metadata(&cp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < REISSUE_AFTER);
    if let (Ok(cert_pem), Ok(key_pem), Ok(have)) = (
        std::fs::read_to_string(&cp),
        std::fs::read_to_string(&kp),
        std::fs::read_to_string(&sp),
    ) {
        if fresh && have == want {
            return Ok(Identity {
                cert_pem,
                key_pem,
                ca_pem: ca.cert_pem.clone(),
            });
        }
    }
    let key_pem = authca::new_host_key()?;
    let (cert_pem, _) = ca.sign_csr(&authca::host_csr(&key_pem)?, cn, sans, NODE_CERT_SECS)?;
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    authca::write_private(&kp, &key_pem)?;
    std::fs::write(&cp, &cert_pem)?;
    std::fs::write(&sp, &want)?;
    Ok(Identity {
        cert_pem,
        key_pem,
        ca_pem: ca.cert_pem.clone(),
    })
}

static CONTROLLER_ID: Mutex<Option<Identity>> = Mutex::new(None);

/// The certificate the controller presents to agents.
pub fn controller_identity() -> Result<Identity> {
    let mut g = CONTROLLER_ID.lock().unwrap();
    if let Some(i) = g.as_ref() {
        return Ok(i.clone());
    }
    let i = load_or_issue_in(
        &dir(),
        &*ca()?,
        "controller",
        CONTROLLER_NAME,
        &[CONTROLLER_NAME.to_string()],
    )?;
    *g = Some(i.clone());
    Ok(i)
}

/// The server certificate for the controller's network (join) listener.
pub fn server_identity(sans: &[String]) -> Result<Identity> {
    load_or_issue_in(&dir(), &*ca()?, "server", "machina controller", sans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("machina-pki-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_ca_is_created_once_and_the_identity_is_reused_until_the_names_change() {
        let d = tmp("reuse");
        let ca = ca_in(&d).unwrap();
        assert_eq!(ca_in(&d).unwrap().cert_pem, ca.cert_pem, "CA persists");
        let a = load_or_issue_in(
            &d,
            &ca,
            "server",
            "s",
            &["10.0.0.1".into(), "ctl.example".into()],
        )
        .unwrap();
        let b = load_or_issue_in(
            &d,
            &ca,
            "server",
            "s",
            &["10.0.0.1".into(), "ctl.example".into()],
        )
        .unwrap();
        assert_eq!(a.cert_pem, b.cert_pem, "same names: same certificate");
        let c = load_or_issue_in(&d, &ca, "server", "s", &["10.0.0.2".into()]).unwrap();
        assert_ne!(a.cert_pem, c.cert_pem, "new names: reissued");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_agent_certificate_names_agent_machina_and_its_host() {
        let d = tmp("agent");
        let ca = ca_in(&d).unwrap();
        let key = authca::new_host_key().unwrap();
        let (pem, not_after) = ca
            .sign_csr(
                &authca::host_csr(&key).unwrap(),
                "n",
                &[AGENT_DOMAIN.to_string(), authca::host_dns("abc")],
                NODE_CERT_SECS,
            )
            .unwrap();
        assert!(not_after > authca::unix_now() + 89 * 86400);
        assert!(pem.contains("BEGIN CERTIFICATE"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

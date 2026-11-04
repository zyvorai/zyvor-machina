// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Mutual authentication PKI for VM network policy (`authentication.mode`).
//!
//! The controller keeps a CA and signs one certificate per hypervisor; the
//! host's machina-bpfd generates the key and sends only a CSR. A certificate
//! names its host as the DNS SAN `<host-id>.host.machina`, so a bpfd that
//! connects to the owner of a peer identity verifies both the chain and that
//! it reached that host; the server checks the client's name the same way.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, CertificateSigningRequestParams, DistinguishedName,
    DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};

/// bpfd-to-bpfd authentication port (Cilium uses the same one).
pub const AUTH_PORT: u16 = 4250;
/// Host certificate lifetime; the controller re-issues past half of it.
pub const HOST_CERT_SECS: i64 = 24 * 3600;

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// SAN naming a host (host ids are UUIDs or `local`).
pub fn host_dns(host_id: &str) -> String {
    let label: String = host_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("{}.host.machina", label.trim_matches('-'))
}

pub struct Ca {
    pub cert_pem: String,
    key_pem: String,
}

impl Ca {
    pub fn generate() -> Result<Self> {
        Self::generate_named("machina VM network policy CA")
    }

    /// A new CA with the given common name (the fleet transport CA is a separate trust domain).
    pub fn generate_named(common_name: &str) -> Result<Self> {
        let key = KeyPair::generate()?;
        let mut p = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, common_name);
        p.distinguished_name = dn;
        p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let now = time::OffsetDateTime::now_utc();
        p.not_before = now - time::Duration::hours(1);
        p.not_after = now + time::Duration::days(3650);
        let cert = p.self_signed(&key)?;
        Ok(Self {
            cert_pem: cert.pem(),
            key_pem: key.serialize_pem(),
        })
    }

    /// `ca.pem` / `ca.key` in `dir`, created (0600 key) on first use.
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        Self::load_or_create_named(dir, "machina VM network policy CA")
    }

    pub fn load_or_create_named(dir: &Path, common_name: &str) -> Result<Self> {
        let (cp, kp) = (dir.join("ca.pem"), dir.join("ca.key"));
        if let (Ok(cert_pem), Ok(key_pem)) =
            (std::fs::read_to_string(&cp), std::fs::read_to_string(&kp))
        {
            return Ok(Self { cert_pem, key_pem });
        }
        let ca = Self::generate_named(common_name)?;
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        write_private(&kp, &ca.key_pem)?;
        std::fs::write(&cp, &ca.cert_pem).with_context(|| format!("write {}", cp.display()))?;
        Ok(ca)
    }

    /// Sign a host CSR. Only the key comes from the CSR: the name, usages
    /// and lifetime are the controller's. Returns (cert PEM, not_after).
    pub fn sign_host(&self, csr_pem: &str, host_id: &str) -> Result<(String, i64)> {
        let name = host_dns(host_id);
        self.sign_csr(csr_pem, &name, std::slice::from_ref(&name), HOST_CERT_SECS)
    }

    /// Sign a CSR for a node or the controller: only the key comes from the CSR; `common_name`,
    /// the SANs (an IP address becomes an IP SAN, anything else a DNS name) and the lifetime
    /// `secs` are the issuer's. Both server and client authentication are allowed.
    pub fn sign_csr(
        &self,
        csr_pem: &str,
        common_name: &str,
        sans: &[String],
        secs: i64,
    ) -> Result<(String, i64)> {
        let mut csr = CertificateSigningRequestParams::from_pem(csr_pem)
            .map_err(|e| anyhow!("invalid CSR: {e}"))?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, common_name);
        csr.params.distinguished_name = dn;
        csr.params.subject_alt_names = sans
            .iter()
            .map(|n| match n.parse::<std::net::IpAddr>() {
                Ok(ip) => Ok(SanType::IpAddress(ip)),
                Err(_) => Ok(SanType::DnsName(n.clone().try_into()?)),
            })
            .collect::<Result<Vec<_>>>()?;
        csr.params.is_ca = IsCa::ExplicitNoCa;
        csr.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        csr.params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        csr.params.custom_extensions.clear();
        let now = time::OffsetDateTime::now_utc();
        let not_after = now + time::Duration::seconds(secs);
        csr.params.not_before = now - time::Duration::minutes(5);
        csr.params.not_after = not_after;
        let key = KeyPair::from_pem(&self.key_pem)?;
        let issuer = CertificateParams::from_ca_cert_pem(&self.cert_pem)?.self_signed(&key)?;
        let cert = csr.signed_by(&issuer, &key)?;
        Ok((cert.pem(), not_after.unix_timestamp()))
    }

    /// `evidence.pem` / `evidence.key` in `dir`: a document-signing
    /// certificate issued by this CA, created on first use.
    pub fn doc_signer(&self, dir: &Path) -> Result<DocSigner> {
        let (cp, kp) = (dir.join("evidence.pem"), dir.join("evidence.key"));
        if let (Ok(cert_pem), Ok(key_pem)) =
            (std::fs::read_to_string(&cp), std::fs::read_to_string(&kp))
        {
            return Ok(DocSigner { cert_pem, key_pem });
        }
        let key = KeyPair::generate()?;
        let mut p = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "machina segmentation evidence signer");
        p.distinguished_name = dn;
        p.is_ca = IsCa::ExplicitNoCa;
        p.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::ContentCommitment,
        ];
        let now = time::OffsetDateTime::now_utc();
        p.not_before = now - time::Duration::hours(1);
        p.not_after = now + time::Duration::days(1825);
        let ca_key = KeyPair::from_pem(&self.key_pem)?;
        let issuer = CertificateParams::from_ca_cert_pem(&self.cert_pem)?.self_signed(&ca_key)?;
        let cert = p.signed_by(&key, &issuer, &ca_key)?;
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        write_private(&kp, &key.serialize_pem())?;
        std::fs::write(&cp, cert.pem()).with_context(|| format!("write {}", cp.display()))?;
        Ok(DocSigner {
            cert_pem: cert.pem(),
            key_pem: key.serialize_pem(),
        })
    }
}

/// Signs documents (segmentation evidence) with a CA-issued certificate.
pub struct DocSigner {
    pub cert_pem: String,
    key_pem: String,
}

impl DocSigner {
    /// ECDSA P-256 / SHA-256, DER encoded (what `openssl dgst -sha256
    /// -verify` checks).
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        let der = PrivateKeyDer::from_pem_slice(self.key_pem.as_bytes())
            .map_err(|e| anyhow!("signing key: {e}"))?;
        let key = rustls::crypto::ring::sign::any_ecdsa_type(&der)
            .map_err(|e| anyhow!("signing key: {e}"))?;
        let signer = key
            .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
            .ok_or_else(|| anyhow!("signing key is not ECDSA P-256"))?;
        signer.sign(data).map_err(|e| anyhow!("sign: {e}"))
    }

    pub fn verify(&self, data: &[u8], sig: &[u8]) -> bool {
        let Ok(der) = CertificateDer::from_pem_slice(self.cert_pem.as_bytes()) else {
            return false;
        };
        let Ok(cert) = webpki::EndEntityCert::try_from(&der) else {
            return false;
        };
        cert.verify_signature(webpki::ring::ECDSA_P256_SHA256, data, sig)
            .is_ok()
    }
}

pub fn write_private(path: &Path, data: &str) -> Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    o.mode(0o600);
    let mut f = o
        .open(&tmp)
        .with_context(|| format!("write {}", tmp.display()))?;
    f.write_all(data.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// A new host key (PEM) and a CSR for it.
pub fn new_host_key() -> Result<String> {
    Ok(KeyPair::generate()?.serialize_pem())
}

pub fn host_csr(key_pem: &str) -> Result<String> {
    let key = KeyPair::from_pem(key_pem)?;
    Ok(CertificateParams::default()
        .serialize_request(&key)?
        .pem()?)
}

/// Key + certificate + CA of one host.
#[derive(Clone)]
pub struct HostIdentity {
    pub host_id: String,
    pub ca_pem: String,
    pub cert_pem: String,
    pub key_pem: String,
}

fn certs(pem: &str) -> Result<Vec<CertificateDer<'static>>> {
    let v: Vec<_> = CertificateDer::pem_slice_iter(pem.as_bytes()).collect::<Result<_, _>>()?;
    if v.is_empty() {
        bail!("no certificate in PEM");
    }
    Ok(v)
}

fn roots(ca_pem: &str) -> Result<rustls::RootCertStore> {
    let mut r = rustls::RootCertStore::empty();
    for c in certs(ca_pem)? {
        r.add(c)?;
    }
    Ok(r)
}

impl HostIdentity {
    fn key(&self) -> Result<PrivateKeyDer<'static>> {
        Ok(PrivateKeyDer::from_pem_slice(self.key_pem.as_bytes())?)
    }

    /// Server side: requires a client certificate from the same CA.
    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>> {
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots(&self.ca_pem)?),
            provider(),
        )
        .build()?;
        let c = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs(&self.cert_pem)?, self.key()?)?;
        Ok(Arc::new(c))
    }

    /// Client side: the server must present the certificate of `host_dns`.
    pub fn client_config(&self) -> Result<Arc<rustls::ClientConfig>> {
        let c = rustls::ClientConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(roots(&self.ca_pem)?)
            .with_client_auth_cert(certs(&self.cert_pem)?, self.key()?)?;
        Ok(Arc::new(c))
    }
}

/// Whether a (chain-verified) peer certificate names `host_id`.
pub fn cert_names_host(cert: &CertificateDer<'_>, host_id: &str) -> bool {
    let Ok(ee) = webpki::EndEntityCert::try_from(cert) else {
        return false;
    };
    let Ok(name) = ServerName::try_from(host_dns(host_id)) else {
        return false;
    };
    ee.verify_is_valid_for_subject_name(&name).is_ok()
}

pub fn server_name(host_id: &str) -> Result<ServerName<'static>> {
    ServerName::try_from(host_dns(host_id)).map_err(|e| anyhow!("host name: {e}"))
}

pub fn unix_now() -> i64 {
    UnixTime::now().as_secs() as i64
}

/// SHA-256 of the first certificate in `pem`, as lower-case hex (what an operator pins).
pub fn cert_sha256(pem: &str) -> Result<String> {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let der = certs(pem)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no certificate"))?;
    // Fingerprint only — never log or print the PEM/DER itself.
    let digest = Sha256::digest(der.as_ref());
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    Ok(out)
}

/// Whether `pinned` (hex, `:` separators and case ignored) is the fingerprint of `pem`.
pub fn fingerprint_matches(pem: &str, pinned: &str) -> bool {
    let want: String = pinned
        .chars()
        .filter(|c| *c != ':' && !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    cert_sha256(pem).map(|have| have == want).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_csr_sets_names_lifetime_and_pins_by_fingerprint() {
        let ca = Ca::generate_named("machina fleet CA").unwrap();
        let key = new_host_key().unwrap();
        let sans = vec!["agent.machina".to_string(), "10.1.2.3".to_string()];
        let (pem, not_after) = ca
            .sign_csr(&host_csr(&key).unwrap(), "node-1", &sans, 90 * 86400)
            .unwrap();
        assert!(not_after > unix_now() + 89 * 86400);
        let der = certs(&pem).unwrap().remove(0);
        let ee = webpki::EndEntityCert::try_from(&der).unwrap();
        assert!(ee
            .verify_is_valid_for_subject_name(&ServerName::try_from("agent.machina").unwrap())
            .is_ok());
        assert!(ee
            .verify_is_valid_for_subject_name(&ServerName::try_from("10.1.2.3").unwrap())
            .is_ok());
        assert!(ee
            .verify_is_valid_for_subject_name(&ServerName::try_from("other.machina").unwrap())
            .is_err());
        let fp = cert_sha256(&ca.cert_pem).unwrap();
        assert_eq!(fp.len(), 64);
        assert!(fingerprint_matches(&ca.cert_pem, &fp.to_uppercase()));
        let colon: String = fp
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(":");
        assert!(fingerprint_matches(&ca.cert_pem, &colon));
        assert!(!fingerprint_matches(&ca.cert_pem, "00"));
        assert!(!fingerprint_matches(&pem, &fp), "a leaf is not the CA");
    }

    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};

    fn host(ca: &Ca, id: &str) -> HostIdentity {
        let key_pem = new_host_key().unwrap();
        let (cert_pem, not_after) = ca.sign_host(&host_csr(&key_pem).unwrap(), id).unwrap();
        assert!(not_after > unix_now() + HOST_CERT_SECS - 60);
        HostIdentity {
            host_id: id.into(),
            ca_pem: ca.cert_pem.clone(),
            cert_pem,
            key_pem,
        }
    }

    /// One TLS exchange; returns the server's view of the client's host and
    /// the reply line, or an error from either side.
    fn exchange(
        server: &HostIdentity,
        client: &HostIdentity,
        dial: &str,
    ) -> Result<(bool, String)> {
        let l = TcpListener::bind("127.0.0.1:0")?;
        let addr = l.local_addr()?;
        let scfg = server.server_config()?;
        let cid = client.host_id.clone();
        let t = std::thread::spawn(move || -> Result<bool> {
            let (s, _) = l.accept()?;
            let conn = rustls::ServerConnection::new(scfg)?;
            let mut tls = rustls::StreamOwned::new(conn, s);
            let mut line = String::new();
            BufReader::new(&mut tls).read_line(&mut line)?;
            let named = tls
                .conn
                .peer_certificates()
                .and_then(|c| c.first())
                .is_some_and(|c| cert_names_host(c, &cid));
            tls.write_all(b"pong\n")?;
            tls.conn.send_close_notify();
            let _ = tls.flush();
            Ok(named)
        });
        let res = (|| -> Result<String> {
            let conn = rustls::ClientConnection::new(client.client_config()?, server_name(dial)?)?;
            let mut tls = rustls::StreamOwned::new(conn, TcpStream::connect(addr)?);
            tls.write_all(b"ping\n")?;
            let mut line = String::new();
            BufReader::new(&mut tls).read_line(&mut line)?;
            Ok(line)
        })();
        let named = t.join().unwrap();
        let line = res?;
        Ok((named?, line))
    }

    #[test]
    fn mutual_tls_names_hosts() {
        let ca = Ca::generate().unwrap();
        let a = host(&ca, "0b9c2a3e-0000-4000-8000-00000000000a");
        let b = host(&ca, "0b9c2a3e-0000-4000-8000-00000000000b");
        let (named, line) = exchange(&b, &a, &b.host_id).unwrap();
        assert!(named);
        assert_eq!(line, "pong\n");

        assert!(
            exchange(&b, &a, &a.host_id).is_err(),
            "server must be the dialed host"
        );

        let other = Ca::generate().unwrap();
        let rogue = host(&other, &a.host_id);
        assert!(
            exchange(&b, &rogue, &b.host_id).is_err(),
            "foreign CA client rejected"
        );
        assert!(
            exchange(&rogue, &a, &b.host_id).is_err(),
            "foreign CA server rejected"
        );
    }

    #[test]
    fn ca_persists_and_csr_name_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let ca = Ca::load_or_create(dir.path()).unwrap();
        let again = Ca::load_or_create(dir.path()).unwrap();
        assert_eq!(ca.cert_pem, again.cert_pem);
        let key = new_host_key().unwrap();
        let csr = CertificateParams::new(vec!["evil.host.machina".to_string()])
            .unwrap()
            .serialize_request(&KeyPair::from_pem(&key).unwrap())
            .unwrap()
            .pem()
            .unwrap();
        let (cert, _) = again.sign_host(&csr, "h1").unwrap();
        let der = certs(&cert).unwrap().remove(0);
        assert!(cert_names_host(&der, "h1"));
        assert!(!cert_names_host(&der, "evil"));
        assert_eq!(host_dns("Local_Host"), "local-host.host.machina");
    }
}

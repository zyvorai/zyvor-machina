// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! bpfd-to-bpfd mutual authentication for `authentication.mode: required`
//! towards VMs on other hosts.
//!
//! The subject's bpfd dials the bpfd of the host that owns the peer identity
//! on [`AUTH_PORT`] over TLS 1.3, both sides presenting controller-signed
//! host certificates. The server answers only when the client certificate
//! names the host the request claims, the peer identity is a VM on one of
//! its taps, and its synced state places the subject identity on the
//! client's host.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use rustls::pki_types::pem::PemObject;
use serde::{Deserialize, Serialize};

use crate::api::{VmAuthCertInfo, VmAuthIdentity};
use crate::authca::{
    self, cert_names_host, host_csr, new_host_key, server_name, HostIdentity, AUTH_PORT,
};

const IO_TIMEOUT: Duration = Duration::from_secs(4);
/// Concurrent handshakes out / connections in (the unit caps tasks at 64).
const MAX_OUT: usize = 8;
const MAX_IN: usize = 16;

/// What the auth server answers from; refreshed by the engine tick.
#[derive(Default, Clone, PartialEq)]
pub(super) struct View {
    pub host_id: String,
    /// Identity → VM name, for VMs on this host's taps.
    pub local: HashMap<u32, String>,
    /// Fleet VM identity → host id.
    pub vm_hosts: HashMap<u32, String>,
    pub host_addrs: BTreeMap<String, String>,
}

pub(super) struct Handshake {
    pub pair: (u32, u32),
    pub ok: bool,
    pub note: String,
}

#[derive(Serialize, Deserialize)]
struct Ask {
    from: String,
    #[serde(default)]
    subject: u32,
    #[serde(default)]
    peer: u32,
    #[serde(default)]
    probe: bool,
}

#[derive(Serialize, Deserialize)]
struct Answer {
    ok: bool,
    #[serde(default)]
    why: String,
}

#[derive(Serialize, Deserialize)]
struct Meta {
    host_id: String,
    not_after: i64,
}

#[derive(Default)]
pub(super) struct VmAuth {
    dir: Option<PathBuf>,
    id: Option<HostIdentity>,
    not_after: i64,
    server: Arc<RwLock<Option<Arc<rustls::ServerConfig>>>>,
    view: Arc<RwLock<View>>,
    done: Arc<Mutex<Vec<Handshake>>>,
    pending: Arc<Mutex<HashSet<(u32, u32)>>>,
    listening: Arc<AtomicBool>,
    started: bool,
}

fn read_line_capped(r: &mut impl Read) -> Result<String> {
    let mut line = String::new();
    BufReader::new(r.take(4096)).read_line(&mut line)?;
    if !line.ends_with('\n') {
        bail!("no reply");
    }
    Ok(line)
}

impl VmAuth {
    /// Load a previously installed certificate from `dir`.
    pub fn set_dir(&mut self, dir: PathBuf) {
        let load = || -> Option<(HostIdentity, i64)> {
            let meta: Meta =
                serde_json::from_slice(&std::fs::read(dir.join("cert.json")).ok()?).ok()?;
            let id = HostIdentity {
                host_id: meta.host_id,
                ca_pem: std::fs::read_to_string(dir.join("ca.pem")).ok()?,
                cert_pem: std::fs::read_to_string(dir.join("cert.pem")).ok()?,
                key_pem: std::fs::read_to_string(dir.join("key.pem")).ok()?,
            };
            Some((id, meta.not_after))
        };
        let loaded = load();
        self.dir = Some(dir);
        if let Some((id, not_after)) = loaded {
            if let Err(e) = self.activate(id, not_after) {
                tracing::warn!("vm auth: stored certificate unusable: {e:#}");
            }
        }
    }

    fn dir(&self) -> Result<&PathBuf> {
        self.dir
            .as_ref()
            .ok_or_else(|| anyhow!("no state directory"))
    }

    fn key_pem(&self) -> Result<String> {
        let dir = self.dir()?;
        let path = dir.join("key.pem");
        if let Ok(k) = std::fs::read_to_string(&path) {
            return Ok(k);
        }
        std::fs::create_dir_all(dir)?;
        let k = new_host_key()?;
        authca::write_private(&path, &k)?;
        Ok(k)
    }

    pub fn identity(&self) -> Result<VmAuthIdentity> {
        Ok(VmAuthIdentity {
            csr: host_csr(&self.key_pem()?)?,
            cert: self.cert_info(),
        })
    }

    pub fn cert_info(&self) -> Option<VmAuthCertInfo> {
        self.id.as_ref().map(|id| VmAuthCertInfo {
            host_id: id.host_id.clone(),
            not_after: self.not_after,
            listening: self.listening.load(Ordering::Relaxed),
        })
    }

    pub fn install(
        &mut self,
        host_id: String,
        ca_pem: String,
        cert_pem: String,
        not_after: i64,
    ) -> Result<VmAuthCertInfo> {
        let id = HostIdentity {
            host_id,
            ca_pem,
            cert_pem,
            key_pem: self.key_pem()?,
        };
        let leaf = rustls::pki_types::CertificateDer::from_pem_slice(id.cert_pem.as_bytes())
            .context("certificate PEM")?;
        if !cert_names_host(&leaf, &id.host_id) {
            bail!("certificate does not name host {}", id.host_id);
        }
        self.activate(id.clone(), not_after)?;
        let dir = self.dir()?.clone();
        std::fs::write(dir.join("ca.pem"), &id.ca_pem)?;
        std::fs::write(dir.join("cert.pem"), &id.cert_pem)?;
        std::fs::write(
            dir.join("cert.json"),
            serde_json::to_vec(&Meta {
                host_id: id.host_id.clone(),
                not_after,
            })?,
        )?;
        self.cert_info()
            .ok_or_else(|| anyhow!("certificate not active"))
    }

    fn activate(&mut self, id: HostIdentity, not_after: i64) -> Result<()> {
        let scfg = id.server_config()?;
        id.client_config()?;
        *self.server.write().unwrap_or_else(|e| e.into_inner()) = Some(scfg);
        self.id = Some(id);
        self.not_after = not_after;
        if !self.started {
            self.started = true;
            self.listen();
        }
        Ok(())
    }

    fn listen(&self) {
        let (server, view, listening) = (
            self.server.clone(),
            self.view.clone(),
            self.listening.clone(),
        );
        std::thread::Builder::new()
            .name("vm-auth".into())
            .spawn(move || {
                let l = match TcpListener::bind((IpAddr::from([0u8; 16]), AUTH_PORT))
                    .or_else(|_| TcpListener::bind(("0.0.0.0", AUTH_PORT)))
                {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::warn!("vm auth: cannot listen on {AUTH_PORT}: {e}");
                        return;
                    }
                };
                listening.store(true, Ordering::Relaxed);
                let active = Arc::new(AtomicUsize::new(0));
                for s in l.incoming().flatten() {
                    let Some(cfg) = server.read().unwrap_or_else(|e| e.into_inner()).clone() else {
                        continue;
                    };
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_IN {
                        active.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let (view, active) = (view.clone(), active.clone());
                    let _ = std::thread::Builder::new()
                        .name("vm-auth-conn".into())
                        .spawn(move || {
                            if let Err(e) = serve(s, cfg, &view) {
                                tracing::debug!("vm auth: {e:#}");
                            }
                            active.fetch_sub(1, Ordering::SeqCst);
                        });
                }
            })
            .ok();
    }

    pub fn set_view(&self, v: View) {
        let mut cur = self.view.write().unwrap_or_else(|e| e.into_inner());
        if *cur != v {
            *cur = v;
        }
    }

    pub fn has_cert(&self) -> bool {
        self.id.is_some()
    }

    pub fn take_done(&self) -> Vec<Handshake> {
        std::mem::take(&mut *self.done.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn is_pending(&self, pair: (u32, u32)) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&pair)
    }

    /// Start a handshake for (subject, peer) with `peer_host`; false when
    /// already running or at the concurrency cap (the datapath asks again).
    pub fn start(&self, pair: (u32, u32), peer_host: &str) -> bool {
        let Some(id) = self.id.clone() else {
            return false;
        };
        let addr = self
            .view
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .host_addrs
            .get(peer_host)
            .cloned();
        {
            let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if p.contains(&pair) || p.len() >= MAX_OUT {
                return false;
            }
            p.insert(pair);
        }
        let (done, pending, host) = (
            self.done.clone(),
            self.pending.clone(),
            peer_host.to_string(),
        );
        let spawned = std::thread::Builder::new()
            .name("vm-auth-dial".into())
            .spawn(move || {
                let res = match addr {
                    None => Err(anyhow!("no address for host {host}")),
                    Some(a) => dial(
                        &id,
                        &host,
                        &a,
                        Ask {
                            from: id.host_id.clone(),
                            subject: pair.0,
                            peer: pair.1,
                            probe: false,
                        },
                    ),
                };
                let (ok, note) = match res {
                    Ok(a) if a.ok => (
                        true,
                        format!("authenticated (mTLS with host {host}: {})", a.why),
                    ),
                    Ok(a) => (false, format!("host {host} refused: {}", a.why)),
                    Err(e) => (false, format!("mTLS with host {host} failed: {e:#}")),
                };
                done.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(Handshake { pair, ok, note });
                pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&pair);
            });
        if spawned.is_err() {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&pair);
        }
        spawned.is_ok()
    }

    pub fn probe_identity(&self) -> Option<HostIdentity> {
        self.id.clone()
    }
}

/// Handshake without identities: proves both certificates and the host name.
pub(super) fn probe(id: &HostIdentity, host_id: &str, address: &str) -> Result<String> {
    let a = dial(
        id,
        host_id,
        address,
        Ask {
            from: id.host_id.clone(),
            subject: 0,
            peer: 0,
            probe: true,
        },
    )?;
    if !a.ok {
        bail!("{}", a.why);
    }
    Ok(a.why)
}

fn dial(id: &HostIdentity, host_id: &str, address: &str, ask: Ask) -> Result<Answer> {
    let ip: IpAddr = address
        .trim_matches(|c| c == '[' || c == ']')
        .parse()
        .with_context(|| format!("host address {address}"))?;
    let s = TcpStream::connect_timeout(&SocketAddr::new(ip, AUTH_PORT), IO_TIMEOUT)?;
    s.set_read_timeout(Some(IO_TIMEOUT))?;
    s.set_write_timeout(Some(IO_TIMEOUT))?;
    let conn = rustls::ClientConnection::new(id.client_config()?, server_name(host_id)?)?;
    let mut tls = rustls::StreamOwned::new(conn, s);
    let mut line = serde_json::to_vec(&ask)?;
    line.push(b'\n');
    tls.write_all(&line)?;
    Ok(serde_json::from_str(&read_line_capped(&mut tls)?)?)
}

fn serve(s: TcpStream, cfg: Arc<rustls::ServerConfig>, view: &RwLock<View>) -> Result<()> {
    s.set_read_timeout(Some(IO_TIMEOUT))?;
    s.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut tls = rustls::StreamOwned::new(rustls::ServerConnection::new(cfg)?, s);
    let ask: Ask = serde_json::from_str(&read_line_capped(&mut tls)?)?;
    let named = tls
        .conn
        .peer_certificates()
        .and_then(|c| c.first())
        .is_some_and(|c| cert_names_host(c, &ask.from));
    let v = view.read().unwrap_or_else(|e| e.into_inner()).clone();
    let answer = answer(&v, &ask, named);
    let mut out = serde_json::to_vec(&answer)?;
    out.push(b'\n');
    tls.write_all(&out)?;
    tls.conn.send_close_notify();
    let _ = tls.flush();
    Ok(())
}

fn answer(v: &View, ask: &Ask, named: bool) -> Answer {
    let no = |why: String| Answer { ok: false, why };
    if !named {
        return no(format!(
            "client certificate does not name host {}",
            ask.from
        ));
    }
    if ask.probe {
        return Answer {
            ok: true,
            why: format!("host {}", v.host_id),
        };
    }
    let Some(vm) = v.local.get(&ask.peer) else {
        return no(format!("identity {} is not a VM on this host", ask.peer));
    };
    if v.vm_hosts.get(&ask.subject) != Some(&ask.from) {
        return no(format!(
            "identity {} is not a VM of host {}",
            ask.subject, ask.from
        ));
    }
    Answer {
        ok: true,
        why: format!("VM {vm}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authca::Ca;

    fn host(ca: &Ca, id: &str) -> HostIdentity {
        let key_pem = new_host_key().unwrap();
        let (cert_pem, _) = ca.sign_host(&host_csr(&key_pem).unwrap(), id).unwrap();
        HostIdentity {
            host_id: id.into(),
            ca_pem: ca.cert_pem.clone(),
            cert_pem,
            key_pem,
        }
    }

    #[test]
    fn answers() {
        let v = View {
            host_id: "b".into(),
            local: HashMap::from([(20, "db-1".to_string())]),
            vm_hosts: HashMap::from([(10, "a".to_string())]),
            host_addrs: BTreeMap::new(),
        };
        let ask = |from: &str, subject, peer| Ask {
            from: from.into(),
            subject,
            peer,
            probe: false,
        };
        assert!(answer(&v, &ask("a", 10, 20), true).ok);
        assert!(!answer(&v, &ask("a", 10, 20), false).ok, "unnamed client");
        assert!(!answer(&v, &ask("a", 10, 21), true).ok, "peer not local");
        assert!(
            !answer(&v, &ask("c", 10, 20), true).ok,
            "subject on another host"
        );
    }

    #[test]
    fn dial_and_serve() {
        let ca = Ca::generate().unwrap();
        let (a, b) = (host(&ca, "host-a"), host(&ca, "host-b"));
        let view = Arc::new(RwLock::new(View {
            host_id: "host-b".into(),
            local: HashMap::from([(20, "db-1".to_string())]),
            vm_hosts: HashMap::from([(10, "host-a".to_string())]),
            host_addrs: BTreeMap::new(),
        }));
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let cfg = b.server_config().unwrap();
        let v2 = view.clone();
        let t = std::thread::spawn(move || {
            for _ in 0..2 {
                let (s, _) = l.accept().unwrap();
                let _ = serve(s, cfg.clone(), &v2);
            }
        });
        let dial_port = |ask: Ask| -> Result<Answer> {
            let s = TcpStream::connect(("127.0.0.1", port))?;
            let conn = rustls::ClientConnection::new(a.client_config()?, server_name("host-b")?)?;
            let mut tls = rustls::StreamOwned::new(conn, s);
            let mut line = serde_json::to_vec(&ask)?;
            line.push(b'\n');
            tls.write_all(&line)?;
            Ok(serde_json::from_str(&read_line_capped(&mut tls)?)?)
        };
        let ok = dial_port(Ask {
            from: "host-a".into(),
            subject: 10,
            peer: 20,
            probe: false,
        })
        .unwrap();
        assert!(ok.ok, "{}", ok.why);
        let lie = dial_port(Ask {
            from: "host-c".into(),
            subject: 10,
            peer: 20,
            probe: false,
        })
        .unwrap();
        assert!(!lie.ok && lie.why.contains("does not name"), "{}", lie.why);
        t.join().unwrap();
    }
}

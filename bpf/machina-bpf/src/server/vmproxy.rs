// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Transparent L7 proxy for VM flows on proxy entries: `terminatingTLS`,
//! `originatingTLS`, and `headerMatches` with ADD / DELETE / REPLACE or a
//! `secret`.
//!
//! The edge redirects a VM's TCP flow here (`mn_vm_l7_inject` assigns it to
//! the transparent listener), so the accepted socket's local address is the
//! original destination and VM_PROXY_FLOW holds the identities the edge
//! resolved. Each HTTP/1.x request is checked against the union of the
//! flow's L7 rules, rewritten and forwarded on a connection from this host
//! whose mark carries the client identity (VM_PROXY_SRC), so the server's
//! edge still sees the client. Denied requests get 403.
//!
//! Secrets are directories under `MACHINA_NETPOL_SECRETS_DIR` (default
//! `/etc/machina/netpol-secrets`): `<namespace>/<name>/<key>`, namespace
//! `default` when the reference has none.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use aya::maps::{HashMap as BpfHash, MapData};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::broadcast;
use tokio::time::timeout;

use machina_bpf_common::{FlowKey, VmProxyFlow, ADDR_LEN, VM_PROXY_UP_MAGIC};

use super::{lock, publish_vm_flow, SharedState};
use crate::api::{StreamEvent, VmEdgeL7Rule, VmFlowRecord};
use crate::authca::provider;
use crate::loader::{mono_to_rfc3339, monotonic_ns};
use crate::netpol::fqdn;
use crate::netpol::l7::{self, L7Rules, Matcher, SecretRef, TlsContext};

/// Listener port on 127.0.0.1 and ::1 (the edge looks it up by address).
pub(super) const PROXY_PORT: u16 = 4251;
const SECRETS_ENV: &str = "MACHINA_NETPOL_SECRETS_DIR";
const SECRETS_DIR: &str = "/etc/machina/netpol-secrets";
const SYSTEM_ROOTS: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
];
const HEAD_MAX: usize = 64 * 1024;
const LINE_MAX: usize = 8 * 1024;
const MAX_CONNS: usize = 2048;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const IPPROTO_TCP: u8 = 6;
const DENIED: &[u8] = b"HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\nContent-Length: 15\r\nConnection: close\r\n\r\nAccess denied\r\n";
const BAD_REQUEST: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const BAD_GATEWAY: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

type BoxIo = Box<dyn Io>;

struct Maps {
    flows: BpfHash<MapData, FlowKey, VmProxyFlow>,
    srcs: BpfHash<MapData, u32, u32>,
    /// client identity → VM_PROXY_SRC slot
    slots: HashMap<u32, u32>,
}

struct Inner {
    shared: SharedState,
    bus: broadcast::Sender<StreamEvent>,
    maps: Mutex<Maps>,
    secrets: PathBuf,
    conns: AtomicUsize,
}

#[derive(Default)]
pub(super) struct VmProxy {
    inner: Option<Arc<Inner>>,
    running: bool,
}

impl VmProxy {
    pub fn init(
        &mut self,
        shared: SharedState,
        bus: broadcast::Sender<StreamEvent>,
        flows: BpfHash<MapData, FlowKey, VmProxyFlow>,
        srcs: BpfHash<MapData, u32, u32>,
    ) {
        let secrets = std::env::var_os(SECRETS_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(SECRETS_DIR));
        self.inner = Some(Arc::new(Inner {
            shared,
            bus,
            maps: Mutex::new(Maps {
                flows,
                srcs,
                slots: HashMap::new(),
            }),
            secrets,
            conns: AtomicUsize::new(0),
        }));
    }

    /// Start the listeners (once); returns the port.
    pub fn ensure_running(&mut self) -> Result<u16> {
        let inner = self
            .inner
            .clone()
            .ok_or_else(|| anyhow!("proxy maps not loaded"))?;
        if self.running {
            return Ok(PROXY_PORT);
        }
        let l4 = transparent_listener(SocketAddr::from(([127, 0, 0, 1], PROXY_PORT)))
            .context("proxy listener on 127.0.0.1")?;
        let l6 = transparent_listener(SocketAddr::from((Ipv6Addr::LOCALHOST, PROXY_PORT)))
            .map_err(|e| tracing::info!("vm proxy: no IPv6 listener: {e:#}"))
            .ok();
        std::thread::Builder::new()
            .name("vm-proxy".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("vm-proxy-rt")
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::warn!("vm proxy: runtime: {e}");
                        return;
                    }
                };
                rt.block_on(async move {
                    let tasks: Vec<_> = [Some(l4), l6]
                        .into_iter()
                        .flatten()
                        .map(|l| tokio::spawn(accept_loop(inner.clone(), l)))
                        .collect();
                    for t in tasks {
                        let _ = t.await;
                    }
                });
            })?;
        self.running = true;
        Ok(PROXY_PORT)
    }
}

fn setsockopt_int(fd: RawFd, level: i32, opt: i32, v: i32) -> std::io::Result<()> {
    let r = unsafe {
        libc::setsockopt(
            fd,
            level,
            opt,
            (&v as *const i32).cast(),
            std::mem::size_of::<i32>() as libc::socklen_t,
        )
    };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Non-blocking IP_TRANSPARENT listener: the edge assigns it connections to
/// any destination.
fn transparent_listener(addr: SocketAddr) -> Result<std::net::TcpListener> {
    let fam = if addr.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    let fd = unsafe {
        libc::socket(
            fam,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let raw = fd.as_raw_fd();
    setsockopt_int(raw, libc::SOL_SOCKET, libc::SO_REUSEADDR, 1)?;
    let r = match addr {
        SocketAddr::V4(a) => {
            setsockopt_int(raw, libc::SOL_IP, libc::IP_TRANSPARENT, 1)?;
            let sa = libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: a.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(a.ip().octets()),
                },
                sin_zero: [0; 8],
            };
            unsafe {
                libc::bind(
                    raw,
                    (&sa as *const libc::sockaddr_in).cast(),
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            }
        }
        SocketAddr::V6(a) => {
            setsockopt_int(raw, libc::SOL_IPV6, libc::IPV6_TRANSPARENT, 1)?;
            setsockopt_int(raw, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, 1)?;
            let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            sa.sin6_port = a.port().to_be();
            sa.sin6_addr.s6_addr = a.ip().octets();
            unsafe {
                libc::bind(
                    raw,
                    (&sa as *const libc::sockaddr_in6).cast(),
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                )
            }
        }
    };
    if r != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if unsafe { libc::listen(raw, 1024) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(std::net::TcpListener::from(fd))
}

async fn accept_loop(inner: Arc<Inner>, l: std::net::TcpListener) {
    let l = match tokio::net::TcpListener::from_std(l) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("vm proxy: listener: {e}");
            return;
        }
    };
    loop {
        let (s, client) = match l.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::debug!("vm proxy: accept: {e}");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        if inner.conns.load(Ordering::Relaxed) >= MAX_CONNS {
            continue;
        }
        inner.conns.fetch_add(1, Ordering::Relaxed);
        let inner = inner.clone();
        tokio::spawn(async move {
            if let Err(e) = serve(&inner, s, client).await {
                tracing::debug!("vm proxy {client}: {e:#}");
            }
            inner.conns.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

fn addr16(ip: IpAddr) -> [u8; ADDR_LEN] {
    match ip {
        IpAddr::V4(a) => a.to_ipv6_mapped().octets(),
        IpAddr::V6(a) => a.octets(),
    }
}

/// VM_PROXY_FLOW key of a proxied connection (the edge's `ct_key`).
fn flow_key(client: SocketAddr, orig: SocketAddr) -> FlowKey {
    FlowKey {
        proto: IPPROTO_TCP,
        local_port: client.port(),
        remote_port: orig.port(),
        local: addr16(client.ip()),
        remote: addr16(orig.ip()),
        ..FlowKey::default()
    }
}

fn secret_path(dir: &Path, s: &SecretRef, key: &str) -> PathBuf {
    dir.join(s.namespace.as_deref().unwrap_or("default"))
        .join(&s.name)
        .join(key)
}

/// Header match values from their secrets (`value` key); unreadable ones
/// stay unresolved and never match.
fn resolve_secrets(dir: &Path, r: &L7Rules) -> L7Rules {
    let mut r = r.clone();
    for h in &mut r.http {
        for m in &mut h.header_matches {
            if let (Some(s), None) = (&m.secret, &m.value) {
                match std::fs::read_to_string(secret_path(dir, s, "value")) {
                    Ok(v) => m.value = Some(v.trim_end_matches(['\r', '\n']).to_string()),
                    Err(e) => tracing::warn!(
                        "vm proxy: header secret {}/{}: {e}",
                        s.namespace.as_deref().unwrap_or("default"),
                        s.name
                    ),
                }
            }
        }
    }
    r
}

fn server_config(dir: &Path, c: &TlsContext) -> Result<rustls::ServerConfig> {
    let cp = secret_path(
        dir,
        &c.secret,
        c.certificate.as_deref().unwrap_or("tls.crt"),
    );
    let kp = secret_path(
        dir,
        &c.secret,
        c.private_key.as_deref().unwrap_or("tls.key"),
    );
    let cert = std::fs::read(&cp).with_context(|| format!("read {}", cp.display()))?;
    let key = std::fs::read(&kp).with_context(|| format!("read {}", kp.display()))?;
    let chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(&cert).collect::<Result<_, _>>()?;
    if chain.is_empty() {
        bail!("{} holds no certificate", cp.display());
    }
    let key = PrivateKeyDer::from_pem_slice(&key)?;
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(chain, key)?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(cfg)
}

/// Trusts the secret's CA (or the system bundle when the policy names no
/// `trustedCA` and the secret has no `ca.crt`); presents the secret's
/// certificate when it has one.
fn client_config(dir: &Path, c: &TlsContext) -> Result<rustls::ClientConfig> {
    let ca = secret_path(dir, &c.secret, c.trusted_ca.as_deref().unwrap_or("ca.crt"));
    let pem = match std::fs::read(&ca) {
        Ok(p) => p,
        Err(_) if c.trusted_ca.is_none() => SYSTEM_ROOTS
            .iter()
            .find_map(|p| std::fs::read(p).ok())
            .ok_or_else(|| anyhow!("no {} and no system CA bundle", ca.display()))?,
        Err(e) => return Err(anyhow!("read {}: {e}", ca.display())),
    };
    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(CertificateDer::pem_slice_iter(&pem).filter_map(|c| c.ok()));
    if roots.is_empty() {
        bail!("no usable CA certificate in {}", ca.display());
    }
    let b = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots);
    let cp = secret_path(
        dir,
        &c.secret,
        c.certificate.as_deref().unwrap_or("tls.crt"),
    );
    let kp = secret_path(
        dir,
        &c.secret,
        c.private_key.as_deref().unwrap_or("tls.key"),
    );
    let mut cfg = match (std::fs::read(&cp), std::fs::read(&kp)) {
        (Ok(cert), Ok(key)) => b.with_client_auth_cert(
            CertificateDer::pem_slice_iter(&cert).collect::<Result<_, _>>()?,
            PrivateKeyDer::from_pem_slice(&key)?,
        )?,
        _ => b.with_no_client_auth(),
    };
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(cfg)
}

struct Policy {
    matcher: Matcher,
    server_names: Vec<String>,
    terminating: Option<TlsContext>,
    originating: Option<TlsContext>,
    source: Option<String>,
}

/// Union of the flow's egress L7 rules (same selection as the L7 reader).
fn flow_policy(inner: &Inner, f: &VmProxyFlow, port: u16) -> Option<Policy> {
    let rules = lock(&inner.shared).vm_l7_rules.clone();
    let hits: Vec<&VmEdgeL7Rule> = rules
        .iter()
        .filter(|r| {
            r.egress
                && r.subject_identity == f.subject
                && (r.peer_identity == 0 || r.peer_identity == f.peer)
                && (r.proto == 0 || r.proto == IPPROTO_TCP)
                && (r.port == 0 || (r.port..=r.port_end.max(r.port)).contains(&port))
        })
        .collect();
    if hits.is_empty() {
        return None;
    }
    let resolved: Vec<L7Rules> = hits
        .iter()
        .map(|r| resolve_secrets(&inner.secrets, &r.rules))
        .collect();
    Some(Policy {
        matcher: Matcher::new(resolved.iter()),
        server_names: hits
            .iter()
            .flat_map(|r| r.rules.server_names.iter().cloned())
            .collect(),
        terminating: hits.iter().find_map(|r| r.rules.terminating_tls.clone()),
        originating: hits.iter().find_map(|r| r.rules.originating_tls.clone()),
        source: hits.iter().find_map(|r| r.source.clone()),
    })
}

/// The n-th slot spread over the VM_PROXY_SLOT bits.
fn slot_bits(n: u32) -> u32 {
    ((n >> 8) & 0xff) << 16 | (n & 0xff)
}

impl Inner {
    fn slot(&self, identity: u32) -> Result<u32> {
        let mut m = lock(&self.maps);
        if let Some(s) = m.slots.get(&identity) {
            return Ok(*s);
        }
        let n = m.slots.len() as u32 + 1;
        if n >= 65_536 {
            bail!("proxy identity slots exhausted");
        }
        let s = slot_bits(n);
        m.srcs.insert(s, identity, 0)?;
        m.slots.insert(identity, s);
        Ok(s)
    }
}

struct Conn<'a> {
    inner: &'a Inner,
    flow: VmProxyFlow,
    client: SocketAddr,
    orig: SocketAddr,
    source: Option<String>,
}

impl Conn<'_> {
    fn record(&self, allowed: bool, kind: &str, summary: String) -> VmFlowRecord {
        let (rec, alerts) = {
            let mut s = lock(&self.inner.shared);
            let (iface, tap_vm) = s.iface(self.flow.ifindex);
            let idx = &s.vm_flow_index;
            let subject = idx.name(self.flow.subject).cloned();
            let peer = idx.name(self.flow.peer).cloned();
            let vm = tap_vm
                .or_else(|| subject.as_ref().map(|s| s.0.clone()))
                .unwrap_or_default();
            let rec = VmFlowRecord {
                ts: mono_to_rfc3339(monotonic_ns()),
                iface: iface.unwrap_or_default(),
                vm,
                direction: "egress".into(),
                src: self.client.ip().to_string(),
                src_port: self.client.port(),
                dst: self.orig.ip().to_string(),
                dst_port: self.orig.port(),
                src_vm: subject.as_ref().map(|s| s.0.clone()),
                dst_vm: peer.as_ref().map(|s| s.0.clone()),
                src_labels: subject.map(|s| s.1).unwrap_or_default(),
                dst_labels: peer.map(|s| s.1).unwrap_or_default(),
                src_identity: self.flow.subject,
                dst_identity: self.flow.peer,
                proto: "TCP".into(),
                verdict: if allowed { "FORWARDED" } else { "DROPPED" }.into(),
                drop_reason: (!allowed).then(|| "l7-deny".into()),
                policy: self.source.clone(),
                l7_type: Some(kind.into()),
                l7: Some(summary),
                ..Default::default()
            };
            let alerts = s.record_vm_flow(&rec);
            (rec, alerts)
        };
        publish_vm_flow(&self.inner.bus, &rec, alerts);
        rec
    }

    async fn connect(&self, pol: &Policy, name: Option<&str>) -> Result<BoxIo> {
        let sock = if self.orig.is_ipv4() {
            TcpSocket::new_v4()?
        } else {
            TcpSocket::new_v6()?
        };
        let mark = VM_PROXY_UP_MAGIC | self.inner.slot(self.flow.subject)?;
        setsockopt_int(
            sock.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_MARK,
            mark as i32,
        )?;
        let s = timeout(IO_TIMEOUT, sock.connect(self.orig))
            .await
            .map_err(|_| anyhow!("connect to {} timed out", self.orig))??;
        let _ = s.set_nodelay(true);
        let Some(o) = &pol.originating else {
            return Ok(Box::new(s));
        };
        let cfg = client_config(&self.inner.secrets, o).context("originatingTLS")?;
        let sn = match name.filter(|n| !n.is_empty()) {
            Some(n) => ServerName::try_from(n.to_string())?,
            None => ServerName::IpAddress(self.orig.ip().into()),
        };
        let tls = timeout(
            IO_TIMEOUT,
            tokio_rustls::TlsConnector::from(Arc::new(cfg)).connect(sn, s),
        )
        .await
        .map_err(|_| anyhow!("TLS handshake with {} timed out", self.orig))??;
        Ok(Box::new(tls))
    }
}

async fn serve(inner: &Arc<Inner>, s: TcpStream, client: SocketAddr) -> Result<()> {
    let orig = s.local_addr()?;
    let flow = lock(&inner.maps)
        .flows
        .get(&flow_key(client, orig), 0)
        .map_err(|_| anyhow!("no redirected flow to {orig}"))?;
    let pol = flow_policy(inner, &flow, orig.port())
        .ok_or_else(|| anyhow!("no proxy rule for {orig}"))?;
    let c = Conn {
        inner,
        flow,
        client,
        orig,
        source: pol.source.clone(),
    };
    let _ = s.set_nodelay(true);
    let (io, sni): (BoxIo, Option<String>) = match &pol.terminating {
        None => (Box::new(s), None),
        Some(t) => {
            let acc = tokio_rustls::LazyConfigAcceptor::new(rustls::server::Acceptor::default(), s);
            let start = timeout(IO_TIMEOUT, acc)
                .await
                .map_err(|_| anyhow!("ClientHello timed out"))??;
            let sni = start
                .client_hello()
                .server_name()
                .map(|n| n.to_ascii_lowercase());
            let name = sni.clone().unwrap_or_default();
            if !pol.server_names.is_empty()
                && !pol.server_names.iter().any(|p| fqdn::matches(p, &name))
            {
                c.record(false, "tls", format!("tls sni={name} [denied]"));
                return Ok(());
            }
            let cfg = match server_config(&inner.secrets, t) {
                Ok(cfg) => cfg,
                Err(e) => {
                    c.record(
                        false,
                        "tls",
                        format!("tls sni={name} (terminatingTLS: {e:#}) [denied]"),
                    );
                    return Ok(());
                }
            };
            let tls = timeout(IO_TIMEOUT, start.into_stream(Arc::new(cfg)))
                .await
                .map_err(|_| anyhow!("TLS handshake timed out"))??;
            (Box::new(tls), sni)
        }
    };
    http_loop(&c, &pol, io, sni).await
}

/// Index of `pat` in `buf` once read; `None` at EOF before any byte.
async fn read_until(
    r: &mut ReadHalf<BoxIo>,
    buf: &mut Vec<u8>,
    pat: &[u8],
    max: usize,
) -> Result<Option<usize>> {
    loop {
        if let Some(p) = buf.windows(pat.len()).position(|w| w == pat) {
            return Ok(Some(p));
        }
        if buf.len() >= max {
            bail!("line or head over {max} bytes");
        }
        let mut tmp = [0u8; 8192];
        let n = r.read(&mut tmp).await?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            bail!("connection closed mid-request");
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

async fn forward_exact(
    r: &mut ReadHalf<BoxIo>,
    w: &mut WriteHalf<BoxIo>,
    buf: &mut Vec<u8>,
    n: usize,
) -> Result<()> {
    let take = n.min(buf.len());
    w.write_all(&buf[..take]).await?;
    buf.drain(..take);
    let mut left = n - take;
    let mut tmp = vec![0u8; 16384];
    while left > 0 {
        let want = left.min(tmp.len());
        let k = r.read(&mut tmp[..want]).await?;
        if k == 0 {
            bail!("connection closed mid-body");
        }
        w.write_all(&tmp[..k]).await?;
        left -= k;
    }
    Ok(())
}

async fn forward_chunked(
    r: &mut ReadHalf<BoxIo>,
    w: &mut WriteHalf<BoxIo>,
    buf: &mut Vec<u8>,
) -> Result<()> {
    loop {
        let e = read_until(r, buf, b"\r\n", LINE_MAX)
            .await?
            .ok_or_else(|| anyhow!("connection closed mid-body"))?;
        let line = std::str::from_utf8(&buf[..e]).map_err(|_| anyhow!("bad chunk size"))?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| anyhow!("bad chunk size"))?;
        w.write_all(&buf[..e + 2]).await?;
        buf.drain(..e + 2);
        if size == 0 {
            loop {
                let e = read_until(r, buf, b"\r\n", LINE_MAX)
                    .await?
                    .ok_or_else(|| anyhow!("connection closed in trailers"))?;
                w.write_all(&buf[..e + 2]).await?;
                buf.drain(..e + 2);
                if e == 0 {
                    return Ok(());
                }
            }
        }
        forward_exact(r, w, buf, size + 2).await?;
    }
}

/// Requests forwarded upstream, oldest first: start, HEAD, flow record.
type Pending = Arc<Mutex<std::collections::VecDeque<(Instant, bool, VmFlowRecord)>>>;

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.split("\r\n").skip(1).find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn response_status(head: &str) -> Option<u16> {
    let line = head.split("\r\n").next()?;
    let mut it = line.split_whitespace();
    it.next().filter(|v| v.starts_with("HTTP/1."))?;
    it.next()?.parse().ok()
}

/// Relay responses, framing each one so its status and latency reach the
/// flow history.
async fn copy_responses(
    mut r: ReadHalf<BoxIo>,
    w: Arc<tokio::sync::Mutex<WriteHalf<BoxIo>>>,
    pending: Pending,
    shared: SharedState,
) {
    let mut buf = Vec::with_capacity(8192);
    let res: Result<()> = async {
        loop {
            let Some(end) = read_until(&mut r, &mut buf, b"\r\n\r\n", HEAD_MAX).await? else {
                return Ok(());
            };
            let head_len = end + 4;
            let head = String::from_utf8_lossy(&buf[..end]).into_owned();
            let mut out = w.lock().await;
            let Some(status) = response_status(&head) else {
                out.write_all(&buf).await?;
                buf.clear();
                tokio::io::copy(&mut r, &mut *out).await?;
                return Ok(());
            };
            out.write_all(&buf[..head_len]).await?;
            buf.drain(..head_len);
            if (100..200).contains(&status) && status != 101 {
                continue;
            }
            let req = lock(&pending).pop_front();
            let is_head = req.as_ref().is_some_and(|p| p.1);
            if let Some((start, _, rec)) = req {
                let ms = start.elapsed().as_millis() as u64;
                lock(&shared).flow_hist.observe_response(&rec, status, ms);
            }
            if status == 101 {
                out.write_all(&buf).await?;
                buf.clear();
                tokio::io::copy(&mut r, &mut *out).await?;
                return Ok(());
            }
            if is_head || status == 204 || status == 304 {
                continue;
            }
            let chunked = header_value(&head, "transfer-encoding")
                .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
            let len = header_value(&head, "content-length").and_then(|v| v.parse::<usize>().ok());
            if chunked {
                forward_chunked(&mut r, &mut out, &mut buf).await?;
            } else if let Some(n) = len {
                forward_exact(&mut r, &mut out, &mut buf, n).await?;
            } else {
                out.write_all(&buf).await?;
                buf.clear();
                tokio::io::copy(&mut r, &mut *out).await?;
                return Ok(());
            }
        }
    }
    .await;
    if let Err(e) = res {
        tracing::debug!("proxy response relay: {e:#}");
    }
    let _ = w.lock().await.shutdown().await;
}

fn host_only(h: &str) -> &str {
    if let Some(rest) = h.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("");
    }
    h.rsplit_once(':')
        .filter(|(_, p)| p.bytes().all(|b| b.is_ascii_digit()))
        .map_or(h, |(a, _)| a)
}

fn summary(req: &l7::HttpRequest) -> String {
    format!("{} {}{}", req.method, req.host, req.path)
}

fn is_upgrade(req: &l7::HttpRequest) -> bool {
    req.method == "CONNECT"
        || req.headers.iter().any(|(n, v)| {
            n == "upgrade" || (n == "connection" && v.to_ascii_lowercase().contains("upgrade"))
        })
}

async fn http_loop(c: &Conn<'_>, pol: &Policy, io: BoxIo, sni: Option<String>) -> Result<()> {
    let (mut cr, cw) = tokio::io::split(io);
    let cw = Arc::new(tokio::sync::Mutex::new(cw));
    let mut buf = Vec::with_capacity(8192);
    let mut up: Option<WriteHalf<BoxIo>> = None;
    let mut responses = None;
    let pending: Pending = Arc::default();
    let tag = if pol.terminating.is_some() {
        " (tls intercepted)"
    } else {
        ""
    };
    loop {
        let Some(end) = read_until(&mut cr, &mut buf, b"\r\n\r\n", HEAD_MAX).await? else {
            break;
        };
        let head_len = end + 4;
        let (req, total) = match l7::parse_http(&buf[..head_len]) {
            Ok(x) => x,
            Err(why) => {
                c.record(false, "http", format!("{why}{tag} [denied]"));
                let _ = cw.lock().await.write_all(BAD_REQUEST).await;
                break;
            }
        };
        let v = pol.matcher.check_http(&req);
        let mut line = format!("{}{tag}", summary(&req));
        if let Some(n) = &v.note {
            line.push_str(&format!(" ({n})"));
        }
        if !v.allowed {
            line.push_str(" [denied]");
        }
        let rec = c.record(v.allowed, "http", line);
        if !v.allowed {
            let mut w = cw.lock().await;
            let _ = w.write_all(DENIED).await;
            let _ = w.shutdown().await;
            break;
        }
        if up.is_none() {
            let name = sni.as_deref().or(Some(host_only(&req.host)));
            match c.connect(pol, name).await {
                Ok(u) => {
                    let (ur, uw) = tokio::io::split(u);
                    up = Some(uw);
                    responses = Some(tokio::spawn(copy_responses(
                        ur,
                        cw.clone(),
                        pending.clone(),
                        c.inner.shared.clone(),
                    )));
                }
                Err(e) => {
                    c.record(false, "http", format!("{} upstream: {e:#}", summary(&req)));
                    let _ = cw.lock().await.write_all(BAD_GATEWAY).await;
                    break;
                }
            }
        }
        let Some(uw) = up.as_mut() else { break };
        let head = std::str::from_utf8(&buf[..end]).unwrap_or_default();
        let head = l7::rewrite_head(head, &v.edits);
        lock(&pending).push_back((Instant::now(), req.method == "HEAD", rec));
        uw.write_all(head.as_bytes()).await?;
        uw.write_all(b"\r\n\r\n").await?;
        buf.drain(..head_len);
        if is_upgrade(&req) {
            uw.write_all(&buf).await?;
            buf.clear();
            tokio::io::copy(&mut cr, uw).await?;
            break;
        }
        match total {
            Some(t) => forward_exact(&mut cr, uw, &mut buf, t - head_len).await?,
            None => forward_chunked(&mut cr, uw, &mut buf).await?,
        }
    }
    if let Some(mut uw) = up {
        let _ = uw.shutdown().await;
    }
    if let Some(t) = responses {
        let _ = t.await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use machina_bpf_common::VM_PROXY_SLOT;

    #[test]
    fn response_heads() {
        let h = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 12\r\nX: y";
        assert_eq!(response_status(h), Some(503));
        assert_eq!(header_value(h, "content-length"), Some("12"));
        assert_eq!(response_status("SSH-2.0-x"), None);
    }

    #[test]
    fn keys_hosts_and_secrets() {
        let c: SocketAddr = "192.168.122.5:40000".parse().unwrap();
        let o: SocketAddr = "1.1.1.1:443".parse().unwrap();
        let k = flow_key(c, o);
        assert_eq!((k.local_port, k.remote_port, k.proto), (40000, 443, 6));
        assert_eq!(&k.local[10..], &[0xff, 0xff, 192, 168, 122, 5]);
        assert_eq!(host_only("api.example.com:8443"), "api.example.com");
        assert_eq!(host_only("[::1]:80"), "::1");
        assert_eq!(host_only("plain"), "plain");
        for n in [1, 0x200, 0x8000, 0xffff] {
            let s = slot_bits(n);
            assert_eq!(s & !VM_PROXY_SLOT, 0);
            assert_eq!((VM_PROXY_UP_MAGIC | s) & 0xff00, 0, "slot {n:#x}");
        }
        assert_ne!(slot_bits(0x100), slot_bits(0x1));
        let d = Path::new("/s");
        let s = SecretRef {
            namespace: None,
            name: "tok".into(),
        };
        assert_eq!(
            secret_path(d, &s, "value"),
            Path::new("/s/default/tok/value")
        );
    }

    #[test]
    fn secret_header_values_resolve() {
        let dir = std::env::temp_dir().join(format!("np-proxy-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("web/tok")).unwrap();
        std::fs::write(dir.join("web/tok/value"), "s3cret\n").unwrap();
        let r = l7::from_to_ports(&serde_json::json!({"rules": {"http": [{"headerMatches": [
            {"name": "X-Token", "secret": {"namespace": "web", "name": "tok"}, "mismatch": "REPLACE"},
            {"name": "X-Gone", "secret": {"name": "missing"}}
        ]}]}}))
        .unwrap();
        let got = resolve_secrets(&dir, &r);
        let hm = &got.http[0].header_matches;
        assert_eq!(hm[0].value.as_deref(), Some("s3cret"));
        assert_eq!(hm[1].value, None);
        let m = Matcher::new([&got]);
        let req = l7::parse_http(b"GET / HTTP/1.1\r\nX-Token: s3cret\r\n\r\n")
            .unwrap()
            .0;
        assert!(!m.check_http(&req).allowed, "X-Gone is unresolved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tls_configs_from_secret_files() {
        let dir = std::env::temp_dir().join(format!("np-proxy-tls-{}", std::process::id()));
        let sd = dir.join("default/intercept");
        std::fs::create_dir_all(&sd).unwrap();
        let ca = crate::authca::Ca::generate().unwrap();
        let key = crate::authca::new_host_key().unwrap();
        let csr = crate::authca::host_csr(&key).unwrap();
        let (cert, _) = ca.sign_host(&csr, "h1").unwrap();
        std::fs::write(sd.join("tls.crt"), &cert).unwrap();
        std::fs::write(sd.join("tls.key"), &key).unwrap();
        std::fs::write(sd.join("ca.crt"), &ca.cert_pem).unwrap();
        let t = TlsContext {
            secret: SecretRef {
                namespace: None,
                name: "intercept".into(),
            },
            ..Default::default()
        };
        assert!(server_config(&dir, &t).is_ok());
        assert!(client_config(&dir, &t).is_ok());
        let missing = TlsContext {
            trusted_ca: Some("nope.pem".into()),
            ..t.clone()
        };
        assert!(client_config(&dir, &missing).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

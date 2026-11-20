// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! First-payload L7 classification: TLS ClientHello (SNI, ALPN) and HTTP/1.x
//! request heads. Pure parsing of a possibly truncated buffer, never panics.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tls {
    pub sni: Option<String>,
    pub alpn: Vec<String>,
    /// Legacy record version from the ClientHello, e.g. "1.2".
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Http {
    pub method: String,
    pub path: String,
    pub version: String,
    pub host: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum L7 {
    Tls(Tls),
    Http(Http),
    Ssh { banner: String },
}

const HTTP_METHODS: &[&str] = &[
    "GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "CONNECT", "TRACE",
];

pub fn classify(b: &[u8]) -> Option<L7> {
    if let Some(t) = parse_client_hello(b) {
        return Some(L7::Tls(t));
    }
    if let Some(h) = parse_http(b) {
        return Some(L7::Http(h));
    }
    if b.starts_with(b"SSH-") {
        let end = b
            .iter()
            .position(|c| *c == b'\r' || *c == b'\n')
            .unwrap_or(b.len())
            .min(128);
        return Some(L7::Ssh {
            banner: String::from_utf8_lossy(&b[..end]).into_owned(),
        });
    }
    None
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(((self.u8()? as u16) << 8) | self.u8()? as u16)
    }
    fn u24(&mut self) -> Option<usize> {
        Some(((self.u8()? as usize) << 16) | ((self.u8()? as usize) << 8) | self.u8()? as usize)
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        (self.p + n <= self.b.len()).then(|| self.p += n)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p + n)?;
        self.p += n;
        Some(s)
    }
}

fn tls_version(major: u8, minor: u8) -> String {
    match (major, minor) {
        (3, 0) => "ssl3".into(),
        (3, n) => format!("1.{}", n - 1),
        _ => format!("{major}.{minor}"),
    }
}

/// Everything a ClientHello fingerprint needs (wire order, GREASE kept).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hello {
    pub legacy_version: u16,
    pub ciphers: Vec<u16>,
    pub extensions: Vec<u16>,
    pub groups: Vec<u16>,
    pub point_formats: Vec<u8>,
    pub sig_algs: Vec<u16>,
    pub supported_versions: Vec<u16>,
    pub sni: Option<String>,
    pub alpn: Vec<String>,
    /// The whole handshake message was readable.
    pub complete: bool,
}

/// RFC 8701 GREASE values (0x?a?a).
pub fn is_grease(v: u16) -> bool {
    v & 0x0f0f == 0x0a0a && (v >> 8) == (v & 0xff)
}

fn u16_list(e: &mut Rd<'_>, len: usize) -> Vec<u16> {
    let mut v = Vec::new();
    for _ in 0..len / 2 {
        match e.u16() {
            Some(x) => v.push(x),
            None => break,
        }
    }
    v
}

/// Parse a (possibly truncated) TLS record carrying a ClientHello.
pub fn parse_hello(b: &[u8]) -> Option<Hello> {
    let mut r = Rd { b, p: 0 };
    if r.u8()? != 0x16 || r.u8()? != 3 {
        return None;
    }
    r.skip(3)?; // record minor version + length
    if r.u8()? != 0x01 {
        return None;
    }
    let hs_len = r.u24()?;
    let hs_end = r.p + hs_len;
    let mut out = Hello {
        legacy_version: r.u16()?,
        complete: hs_end <= b.len(),
        ..Hello::default()
    };
    r.skip(32)?; // random
    let sid = r.u8()? as usize;
    r.skip(sid)?;
    let cs = r.u16()? as usize;
    let Some(cs_body) = r.take(cs) else {
        out.complete = false;
        return Some(out);
    };
    out.ciphers = u16_list(&mut Rd { b: cs_body, p: 0 }, cs);
    let Some(comp) = r.u8() else { return Some(out) };
    if r.skip(comp as usize).is_none() {
        return Some(out);
    }
    let Some(ext_len) = r.u16() else {
        return Some(out);
    };
    let end = (r.p + ext_len as usize).min(b.len());
    while r.p + 4 <= end {
        let ty = r.u16()?;
        let len = r.u16()? as usize;
        let Some(body) = r.take(len) else {
            out.complete = false;
            break;
        };
        out.extensions.push(ty);
        let mut e = Rd { b: body, p: 0 };
        match ty {
            0x0000 => {
                // server_name list → first host_name entry
                let _list = e.u16();
                while let (Some(kind), Some(n)) = (e.u8(), e.u16()) {
                    let Some(name) = e.take(n as usize) else {
                        break;
                    };
                    if kind == 0 {
                        if let Ok(s) = std::str::from_utf8(name) {
                            if !s.is_empty() && s.bytes().all(|c| c.is_ascii_graphic()) {
                                out.sni = Some(s.to_ascii_lowercase());
                            }
                        }
                        break;
                    }
                }
            }
            0x000a => {
                let n = e.u16().unwrap_or(0) as usize;
                out.groups = u16_list(&mut e, n);
            }
            0x000b => {
                let n = e.u8().unwrap_or(0) as usize;
                out.point_formats = e.take(n).map(<[u8]>::to_vec).unwrap_or_default();
            }
            0x000d => {
                let n = e.u16().unwrap_or(0) as usize;
                out.sig_algs = u16_list(&mut e, n);
            }
            0x0010 => {
                let _list = e.u16();
                while let Some(n) = e.u8() {
                    let Some(p) = e.take(n as usize) else { break };
                    out.alpn.push(String::from_utf8_lossy(p).into_owned());
                }
            }
            0x002b => {
                let n = e.u8().unwrap_or(0) as usize;
                out.supported_versions = u16_list(&mut e, n);
            }
            _ => {}
        }
    }
    Some(out)
}

/// Extensions may be cut off by the snap length: whatever was readable is kept.
pub fn parse_client_hello(b: &[u8]) -> Option<Tls> {
    let h = parse_hello(b)?;
    let best = h
        .supported_versions
        .iter()
        .copied()
        .filter(|v| !is_grease(*v))
        .max();
    let v = best.unwrap_or(h.legacy_version);
    Some(Tls {
        sni: h.sni,
        alpn: h.alpn,
        version: tls_version((v >> 8) as u8, v as u8),
    })
}

fn join<T: ToString>(v: impl Iterator<Item = T>, sep: &str) -> String {
    v.map(|x| x.to_string()).collect::<Vec<_>>().join(sep)
}

/// JA3 string and its MD5: `version,ciphers,extensions,groups,point_formats`
/// in decimal, GREASE removed.
pub fn ja3(h: &Hello) -> (String, String) {
    use md5::Digest;
    let ng = |v: &[u16]| join(v.iter().filter(|x| !is_grease(**x)), "-");
    let s = format!(
        "{},{},{},{},{}",
        h.legacy_version,
        ng(&h.ciphers),
        ng(&h.extensions),
        ng(&h.groups),
        join(h.point_formats.iter(), "-")
    );
    let hash = md5::Md5::digest(s.as_bytes());
    let hex = hash.iter().map(|b| format!("{b:02x}")).collect::<String>();
    (s, hex)
}

fn sha12(s: &str) -> String {
    use sha2::Digest;
    if s.is_empty() {
        return "000000000000".into();
    }
    let h = sha2::Sha256::digest(s.as_bytes());
    h.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// JA4 (TLS over TCP): `t{ver}{d|i}{ciphers:02}{exts:02}{alpn}_{sha(ciphers)}_{sha(exts_sigalgs)}`.
pub fn ja4(h: &Hello) -> String {
    let best = h
        .supported_versions
        .iter()
        .copied()
        .filter(|v| !is_grease(*v))
        .max();
    let ver = match best.unwrap_or(h.legacy_version) {
        0x0304 => "13",
        0x0303 => "12",
        0x0302 => "11",
        0x0301 => "10",
        0x0300 => "s3",
        _ => "00",
    };
    let ciphers: Vec<u16> = h
        .ciphers
        .iter()
        .copied()
        .filter(|v| !is_grease(*v))
        .collect();
    let exts: Vec<u16> = h
        .extensions
        .iter()
        .copied()
        .filter(|v| !is_grease(*v))
        .collect();
    let alpn = match h.alpn.first().map(|a| a.as_bytes()) {
        Some([]) | None => "00".to_string(),
        Some(a) => {
            let (f, l) = (a[0], a[a.len() - 1]);
            if f.is_ascii_alphanumeric() && l.is_ascii_alphanumeric() {
                format!("{}{}", f as char, l as char)
            } else {
                let hx = a.iter().map(|b| format!("{b:02x}")).collect::<String>();
                format!("{}{}", &hx[..1], &hx[hx.len() - 1..])
            }
        }
    };
    let a = format!(
        "t{ver}{}{:02}{:02}{alpn}",
        if h.sni.is_some() { 'd' } else { 'i' },
        ciphers.len().min(99),
        exts.len().min(99)
    );
    let mut cs: Vec<String> = ciphers.iter().map(|c| format!("{c:04x}")).collect();
    cs.sort();
    let mut es: Vec<String> = exts
        .iter()
        .filter(|e| **e != 0x0000 && **e != 0x0010)
        .map(|e| format!("{e:04x}"))
        .collect();
    es.sort();
    let mut c_in = es.join(",");
    let sigs: Vec<String> = h.sig_algs.iter().map(|s| format!("{s:04x}")).collect();
    if !sigs.is_empty() && !c_in.is_empty() {
        c_in = format!("{c_in}_{}", sigs.join(","));
    }
    format!("{a}_{}_{}", sha12(&cs.join(",")), sha12(&c_in))
}

pub fn parse_http(b: &[u8]) -> Option<Http> {
    let sp = b.iter().take(8).position(|c| *c == b' ')?;
    let method = std::str::from_utf8(&b[..sp]).ok()?;
    if !HTTP_METHODS.contains(&method) {
        return None;
    }
    let line_end = b.iter().position(|c| *c == b'\n').unwrap_or(b.len());
    let line = String::from_utf8_lossy(&b[..line_end]);
    let mut parts = line.trim_end_matches('\r').split(' ');
    parts.next();
    let path = parts.next()?.chars().take(512).collect::<String>();
    let version = parts.next().unwrap_or("").to_string();
    if !version.is_empty() && !version.starts_with("HTTP/") {
        return None;
    }
    let mut out = Http {
        method: method.to_string(),
        path,
        version,
        ..Http::default()
    };
    let rest = b.get(line_end + 1..).unwrap_or(&[]);
    for raw in rest.split(|c| *c == b'\n') {
        let l = String::from_utf8_lossy(raw);
        let l = l.trim_end_matches('\r');
        if l.is_empty() {
            break;
        }
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        let v = v.trim().chars().take(256).collect::<String>();
        match k.trim().to_ascii_lowercase().as_str() {
            "host" => out.host = Some(v),
            "user-agent" => out.user_agent = Some(v),
            _ => {}
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(ty: u16, body: &[u8]) -> Vec<u8> {
        let mut v = ty.to_be_bytes().to_vec();
        v.extend((body.len() as u16).to_be_bytes());
        v.extend(body);
        v
    }

    fn client_hello(sni: &str) -> Vec<u8> {
        let mut sn = Vec::new();
        sn.extend(((sni.len() + 3) as u16).to_be_bytes());
        sn.push(0);
        sn.extend((sni.len() as u16).to_be_bytes());
        sn.extend(sni.as_bytes());
        let mut alpn = Vec::new();
        alpn.extend(12u16.to_be_bytes());
        alpn.push(2);
        alpn.extend(b"h2");
        alpn.push(8);
        alpn.extend(b"http/1.1");
        let versions = [4u8, 0x0a, 0x0a, 3, 4];
        let mut exts = ext(0x0000, &sn);
        exts.extend(ext(0x0010, &alpn));
        exts.extend(ext(0x002b, &versions));

        let mut hs = vec![3, 3];
        hs.extend([0u8; 32]);
        hs.push(0); // session id
        hs.extend(4u16.to_be_bytes());
        hs.extend([0x13, 0x01, 0x13, 0x02]);
        hs.extend([1, 0]);
        hs.extend((exts.len() as u16).to_be_bytes());
        hs.extend(exts);

        let mut msg = vec![1];
        msg.extend(&(hs.len() as u32).to_be_bytes()[1..]);
        msg.extend(hs);
        let mut rec = vec![0x16, 3, 1];
        rec.extend((msg.len() as u16).to_be_bytes());
        rec.extend(msg);
        rec
    }

    #[test]
    fn tls_sni_alpn_version() {
        let t = parse_client_hello(&client_hello("API.Example.com")).unwrap();
        assert_eq!(t.sni.as_deref(), Some("api.example.com"));
        assert_eq!(t.alpn, vec!["h2", "http/1.1"]);
        assert_eq!(t.version, "1.3");
    }

    #[test]
    fn tls_truncated_never_panics() {
        let full = client_hello("example.org");
        for n in 0..full.len() {
            let _ = classify(&full[..n]);
        }
        assert!(parse_client_hello(&full[..60]).is_some_and(|t| t.sni.is_none()));
    }

    #[test]
    fn ja3_ja4_fingerprints() {
        let mut ch = client_hello("example.org");
        let h = parse_hello(&ch).unwrap();
        assert!(h.complete);
        assert_eq!(h.ciphers, vec![0x1301, 0x1302]);
        assert_eq!(h.extensions, vec![0x0000, 0x0010, 0x002b]);
        let (s, hash) = ja3(&h);
        assert_eq!(s, "771,4865-4866,0-16-43,,");
        assert_eq!(hash.len(), 32);
        let j = ja4(&h);
        assert!(j.starts_with("t13d0203h2_"), "{j}");
        assert_eq!(j.len(), "t13d0203h2_".len() + 12 + 1 + 12);
        // GREASE values are ignored.
        assert!(is_grease(0x1a1a) && is_grease(0xfafa) && !is_grease(0x1301));
        // Truncation is flagged, not fatal.
        ch.truncate(ch.len() - 3);
        assert!(!parse_hello(&ch).unwrap().complete);
    }

    #[test]
    fn http_request() {
        let h = parse_http(
            b"GET /index.html?q=1 HTTP/1.1\r\nHost: Example.com\r\nUser-Agent: curl/8\r\n\r\n",
        )
        .unwrap();
        assert_eq!(h.method, "GET");
        assert_eq!(h.path, "/index.html?q=1");
        assert_eq!(h.version, "HTTP/1.1");
        assert_eq!(h.host.as_deref(), Some("Example.com"));
        assert_eq!(h.user_agent.as_deref(), Some("curl/8"));
        assert!(parse_http(b"HELLO world").is_none());
        assert!(parse_http(b"GET / FOO/1").is_none());
    }

    #[test]
    fn ssh_banner() {
        assert_eq!(
            classify(b"SSH-2.0-OpenSSH_9.6\r\n"),
            Some(L7::Ssh {
                banner: "SSH-2.0-OpenSSH_9.6".into()
            })
        );
        assert_eq!(classify(b"\x00\x01binary"), None);
    }
}

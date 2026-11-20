// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Incremental L7 inspection of one TCP byte stream (client → server). The
//! VM edge hands bpfd every segment past the flow's allowed window; bpfd
//! feeds the new bytes in order and learns how far the stream is allowed:
//! request heads are checked against the rules, HTTP bodies
//! (Content-Length, chunked, HTTP/2 DATA) pass without a check. Kafka
//! requests are read whole, since topic names sit between record batches.
//! HTTP/2 (prior knowledge or after an `h2c` upgrade) is followed frame by
//! frame with an HPACK decoder; every header block is decoded to keep its
//! table in step, and each request (gRPC included) is checked.

use super::l7::{self, Request};

/// Longest request head (HTTP header block, TLS record, DNS message)
/// buffered before giving up.
pub const MAX_HEAD: usize = 64 * 1024;
/// Largest Kafka request inspected (the client default
/// `max.request.size` is 1 MiB).
pub const KAFKA_MAX: usize = 4 << 20;
const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
/// HPACK dynamic table limit; clients stay within the server's
/// SETTINGS_HEADER_TABLE_SIZE, commonly 4 KiB.
const HPACK_MAX_TABLE: usize = 64 * 1024;
const H2_DATA: u8 = 0x0;
const H2_HEADERS: u8 = 0x1;
const H2_PUSH_PROMISE: u8 = 0x5;
const H2_CONTINUATION: u8 = 0x9;
const H2_END_HEADERS: u8 = 0x4;
const H2_PADDED: u8 = 0x8;
const H2_PRIORITY: u8 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Http,
    Kafka,
    Tls,
    DnsTcp,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Http => "http",
            Kind::Kafka => "kafka",
            Kind::Tls => "tls",
            Kind::DnsTcp => "dns",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub req: Request,
    pub allowed: bool,
    pub note: Option<String>,
}

/// Result of feeding bytes.
#[derive(Debug, Default)]
pub struct Progress {
    /// Stream offset up to which every byte is allowed.
    pub allowed: u64,
    /// Bytes right after `allowed` that are allowed without inspection (a
    /// body). The caller may pass them in the kernel and then calls
    /// [`Stream::skip`].
    pub body: u64,
    /// The rest of the connection is allowed (TLS after the ClientHello,
    /// CONNECT, an upgraded connection).
    pub open: bool,
    /// A request was denied, or the stream could not be parsed (`error`).
    pub denied: bool,
    pub error: Option<String>,
    pub verdicts: Vec<Verdict>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chunk {
    Size,
    Data(u64),
    DataEnd,
    Trailer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum St {
    Head,
    Body(u64),
    Chunk(Chunk),
    Open,
    Denied,
}

pub type Check<'a> = dyn FnMut(&Request) -> (bool, Option<String>) + 'a;

struct H2 {
    dec: loona_hpack::Decoder<'static>,
    /// Header block being collected (HEADERS + CONTINUATION) and its stream.
    block: Vec<u8>,
    block_stream: u32,
}

pub struct Stream {
    kind: Kind,
    st: St,
    h2: Option<Box<H2>>,
    /// Unconsumed bytes, starting at stream offset `base`.
    buf: Vec<u8>,
    base: u64,
}

enum Step {
    /// Bytes consumed (allowed); keep going.
    Took(usize),
    NeedMore,
    Stop,
}

fn find(b: &[u8], pat: &[u8]) -> Option<usize> {
    b.windows(pat.len()).position(|w| w == pat)
}

impl Stream {
    pub fn new(kind: Kind) -> Self {
        Stream {
            kind,
            st: St::Head,
            h2: None,
            buf: Vec::new(),
            base: 0,
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Stream offset of the next byte to feed.
    pub fn pos(&self) -> u64 {
        self.base + self.buf.len() as u64
    }

    /// Body bytes at [`pos`](Self::pos) the caller may let pass unseen
    /// (then calls [`skip`](Self::skip)).
    pub fn body_left(&self) -> u64 {
        match self.st {
            St::Body(n) | St::Chunk(Chunk::Data(n)) if self.buf.is_empty() => n,
            _ => 0,
        }
    }

    /// Stream offset up to which every byte is allowed.
    pub fn allowed(&self) -> u64 {
        self.base
    }

    /// The connection speaks HTTP/2.
    pub fn is_h2(&self) -> bool {
        self.h2.is_some()
    }

    pub fn is_open(&self) -> bool {
        self.st == St::Open
    }

    pub fn is_denied(&self) -> bool {
        self.st == St::Denied
    }

    /// The caller let `n` body bytes (from [`Progress::body`]) pass unseen.
    pub fn skip(&mut self, n: u64) {
        debug_assert!(self.buf.is_empty());
        let (r, chunk) = match self.st {
            St::Body(r) => (r, false),
            St::Chunk(Chunk::Data(r)) => (r, true),
            _ => return,
        };
        let n = n.min(r);
        self.base += n;
        self.st = match (r - n, chunk) {
            (0, true) => St::Chunk(Chunk::DataEnd),
            (0, false) => St::Head,
            (l, true) => St::Chunk(Chunk::Data(l)),
            (l, false) => St::Body(l),
        };
    }

    fn head_limit(&self) -> usize {
        match self.kind {
            Kind::Kafka => KAFKA_MAX + 4,
            _ => MAX_HEAD,
        }
    }

    /// Feed the bytes at [`pos`](Self::pos) onwards.
    pub fn feed(&mut self, data: &[u8], check: &mut Check) -> Progress {
        let mut p = Progress::default();
        if matches!(self.st, St::Open | St::Denied) {
            p.open = self.st == St::Open;
            p.denied = self.st == St::Denied;
            p.allowed = self.pos();
            return p;
        }
        self.buf.extend_from_slice(data);
        let mut i = 0usize;
        while let Step::Took(n) = self.step(i, check, &mut p) {
            i += n;
        }
        self.buf.drain(..i);
        self.base += i as u64;
        if self.st == St::Head && self.buf.len() > self.head_limit() {
            let why = format!(
                "{} request over {} bytes",
                self.kind.name(),
                self.head_limit()
            );
            self.fail(&mut p, why);
        }
        if self.st == St::Open {
            p.open = true;
            self.base += self.buf.len() as u64;
            self.buf.clear();
        }
        p.denied = self.st == St::Denied;
        p.allowed = self.base;
        p.body = match self.st {
            St::Body(n) | St::Chunk(Chunk::Data(n)) if self.buf.is_empty() => n,
            _ => 0,
        };
        p
    }

    fn fail(&mut self, p: &mut Progress, why: String) {
        self.st = St::Denied;
        p.error = Some(why);
    }

    fn verdict(&mut self, p: &mut Progress, req: Request, check: &mut Check) -> bool {
        let (allowed, note) = check(&req);
        p.verdicts.push(Verdict { req, allowed, note });
        if !allowed {
            self.st = St::Denied;
        }
        allowed
    }

    fn step(&mut self, i: usize, check: &mut Check, p: &mut Progress) -> Step {
        let rest = &self.buf[i..];
        match self.st {
            St::Open | St::Denied => Step::Stop,
            St::Body(n) => {
                let take = (n as usize).min(rest.len());
                self.st = if take as u64 == n {
                    St::Head
                } else {
                    St::Body(n - take as u64)
                };
                if take == 0 {
                    Step::NeedMore
                } else {
                    Step::Took(take)
                }
            }
            St::Chunk(c) => self.chunk(c, i, p),
            St::Head if self.h2.is_some() => self.h2_frame(i, check, p),
            St::Head => match self.kind {
                Kind::Http => self.http_head(i, check, p),
                Kind::Kafka => self.kafka_head(i, check, p),
                Kind::Tls => self.tls_head(i, check, p),
                Kind::DnsTcp => self.dns_head(i, check, p),
            },
        }
    }

    fn http_head(&mut self, i: usize, check: &mut Check, p: &mut Progress) -> Step {
        let rest = &self.buf[i..];
        if rest.is_empty() {
            return Step::NeedMore;
        }
        if H2_PREFACE.starts_with(&rest[..rest.len().min(H2_PREFACE.len())]) {
            if rest.len() < H2_PREFACE.len() {
                return Step::NeedMore;
            }
            let mut dec = loona_hpack::Decoder::new();
            dec.set_max_allowed_table_size(HPACK_MAX_TABLE);
            self.h2 = Some(Box::new(H2 {
                dec,
                block: Vec::new(),
                block_stream: 0,
            }));
            return Step::Took(H2_PREFACE.len());
        }
        let Some(end) = find(rest, b"\r\n\r\n") else {
            if !l7::HTTP_METHODS.iter().any(|m| {
                let n = m.len().min(rest.len());
                m.as_bytes()[..n] == rest[..n]
            }) {
                self.fail(p, "not an HTTP/1.x request".into());
                return Step::Stop;
            }
            return Step::NeedMore;
        };
        let head_len = end + 4;
        let (req, total) = match l7::parse_http(&rest[..head_len]) {
            Ok(v) => v,
            Err(e) => {
                self.fail(p, format!("HTTP: {e}"));
                return Step::Stop;
            }
        };
        // An h2c upgrade is followed by the HTTP/2 preface (or by more
        // HTTP/1 if the server declines), so it is not a tunnel.
        let upgrade = req.headers.iter().find(|(n, _)| n == "upgrade");
        let tunnel = req.method == "CONNECT"
            || (upgrade.is_some_and(|(_, v)| !v.eq_ignore_ascii_case("h2c"))
                && req
                    .headers
                    .iter()
                    .any(|(n, v)| n == "connection" && v.to_ascii_lowercase().contains("upgrade")));
        let body = match total {
            Some(t) => t.saturating_sub(head_len) as u64,
            None => 0,
        };
        if !self.verdict(p, Request::Http(req), check) {
            return Step::Stop;
        }
        self.st = if tunnel {
            St::Open
        } else if total.is_none() {
            St::Chunk(Chunk::Size)
        } else if body > 0 {
            St::Body(body)
        } else {
            St::Head
        };
        Step::Took(head_len)
    }

    fn h2_frame(&mut self, i: usize, check: &mut Check, p: &mut Progress) -> Step {
        let rest = &self.buf[i..];
        if rest.len() < 9 {
            return Step::NeedMore;
        }
        let len = u32::from_be_bytes([0, rest[0], rest[1], rest[2]]) as usize;
        let (ty, flags) = (rest[3], rest[4]);
        let sid = u32::from_be_bytes([rest[5], rest[6], rest[7], rest[8]]) & 0x7fff_ffff;
        let in_block = self.h2.as_ref().is_some_and(|h| h.block_stream != 0);
        if in_block && ty != H2_CONTINUATION {
            self.fail(p, "HTTP/2 frame inside a header block".into());
            return Step::Stop;
        }
        if ty == H2_DATA {
            self.st = if len > 0 {
                St::Body(len as u64)
            } else {
                St::Head
            };
            return Step::Took(9);
        }
        if len > MAX_HEAD {
            self.fail(p, format!("HTTP/2 frame over {MAX_HEAD} bytes"));
            return Step::Stop;
        }
        if rest.len() < 9 + len {
            return Step::NeedMore;
        }
        let payload = rest[9..9 + len].to_vec();
        let done = match ty {
            H2_HEADERS => {
                let Some(frag) = h2_fragment(&payload, flags).filter(|_| sid != 0) else {
                    self.fail(p, "HTTP/2: malformed HEADERS".into());
                    return Step::Stop;
                };
                let h2 = self.h2.as_mut().expect("h2");
                h2.block = frag;
                h2.block_stream = sid;
                flags & H2_END_HEADERS != 0
            }
            H2_CONTINUATION => {
                let fits = self.h2.as_ref().is_some_and(|h| {
                    in_block && h.block_stream == sid && h.block.len() + payload.len() <= MAX_HEAD
                });
                if !fits {
                    self.fail(p, "HTTP/2: unexpected CONTINUATION".into());
                    return Step::Stop;
                }
                let h2 = self.h2.as_mut().expect("h2");
                h2.block.extend_from_slice(&payload);
                flags & H2_END_HEADERS != 0
            }
            H2_PUSH_PROMISE => {
                self.fail(p, "HTTP/2: PUSH_PROMISE from a client".into());
                return Step::Stop;
            }
            _ => false,
        };
        if done && !self.h2_request(check, p) {
            return Step::Stop;
        }
        Step::Took(9 + len)
    }

    /// Decode the collected header block; a request (`:method` present) is
    /// checked, trailers only keep the HPACK table in step.
    fn h2_request(&mut self, check: &mut Check, p: &mut Progress) -> bool {
        let h2 = self.h2.as_mut().expect("h2");
        let block = std::mem::take(&mut h2.block);
        h2.block_stream = 0;
        let dec = &mut h2.dec;
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dec.decode(&block)));
        let fields = match decoded {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => {
                self.fail(p, format!("HTTP/2 HPACK: {e}"));
                return false;
            }
            Err(_) => {
                self.fail(p, "HTTP/2 HPACK: decoder failure".into());
                return false;
            }
        };
        let mut req = l7::HttpRequest::default();
        for (n, v) in fields {
            let n = String::from_utf8_lossy(&n).to_ascii_lowercase();
            let v = String::from_utf8_lossy(&v).into_owned();
            match n.as_str() {
                ":method" => req.method = v,
                ":path" => req.path = v,
                ":authority" => req.host = v,
                "host" if req.host.is_empty() => req.host = v,
                _ if n.starts_with(':') => {}
                _ => req.headers.push((n, v)),
            }
        }
        if req.method.is_empty() {
            return true;
        }
        self.verdict(p, Request::Http(req), check)
    }

    fn chunk(&mut self, c: Chunk, i: usize, p: &mut Progress) -> Step {
        let rest = &self.buf[i..];
        match c {
            Chunk::Size => {
                let Some(e) = find(rest, b"\r\n") else {
                    if rest.len() > 1024 {
                        self.fail(p, "chunk size line too long".into());
                        return Step::Stop;
                    }
                    return Step::NeedMore;
                };
                let line = std::str::from_utf8(&rest[..e]).unwrap_or("");
                let hex = line.split(';').next().unwrap_or("").trim();
                let Ok(n) = u64::from_str_radix(hex, 16) else {
                    self.fail(p, format!("bad chunk size `{hex}`"));
                    return Step::Stop;
                };
                self.st = St::Chunk(if n == 0 {
                    Chunk::Trailer
                } else {
                    Chunk::Data(n)
                });
                Step::Took(e + 2)
            }
            Chunk::Data(n) => {
                let take = (n as usize).min(rest.len());
                if take == 0 {
                    return Step::NeedMore;
                }
                let left = n - take as u64;
                self.st = St::Chunk(if left == 0 {
                    Chunk::DataEnd
                } else {
                    Chunk::Data(left)
                });
                Step::Took(take)
            }
            Chunk::DataEnd => {
                if rest.len() < 2 {
                    return Step::NeedMore;
                }
                if &rest[..2] != b"\r\n" {
                    self.fail(p, "chunk data not followed by CRLF".into());
                    return Step::Stop;
                }
                self.st = St::Chunk(Chunk::Size);
                Step::Took(2)
            }
            Chunk::Trailer => {
                let Some(e) = find(rest, b"\r\n") else {
                    return Step::NeedMore;
                };
                if e == 0 {
                    self.st = St::Head;
                }
                Step::Took(e + 2)
            }
        }
    }

    fn kafka_head(&mut self, i: usize, check: &mut Check, p: &mut Progress) -> Step {
        let rest = &self.buf[i..];
        if rest.len() < 4 {
            return Step::NeedMore;
        }
        let size = i32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
        if !(10..=100 << 20).contains(&size) {
            self.fail(p, "not a Kafka request".into());
            return Step::Stop;
        }
        let total = size as usize + 4;
        if total > KAFKA_MAX + 4 {
            self.fail(p, format!("Kafka request over {KAFKA_MAX} bytes"));
            return Step::Stop;
        }
        if rest.len() < total {
            return Step::NeedMore;
        }
        let req = match l7::parse_kafka(&rest[..total]) {
            Ok(mut v) if !v.is_empty() => v.remove(0).0,
            Ok(_) => {
                self.fail(p, "empty Kafka request".into());
                return Step::Stop;
            }
            Err(e) => {
                self.fail(p, format!("Kafka: {e}"));
                return Step::Stop;
            }
        };
        if !self.verdict(p, Request::Kafka(req), check) {
            return Step::Stop;
        }
        Step::Took(total)
    }

    fn tls_head(&mut self, i: usize, check: &mut Check, p: &mut Progress) -> Step {
        let rest = &self.buf[i..];
        if rest.is_empty() {
            return Step::NeedMore;
        }
        if rest[0] != 0x16 {
            self.fail(p, "TLS: not a handshake record".into());
            return Step::Stop;
        }
        if rest.len() < 5 {
            return Step::NeedMore;
        }
        let rec = 5 + u16::from_be_bytes([rest[3], rest[4]]) as usize;
        if rest.len() < rec {
            return Step::NeedMore;
        }
        match l7::parse_sni(&rest[..rec]) {
            Ok(sni) => {
                if self.verdict(p, Request::Tls { server_name: sni }, check) {
                    self.st = St::Open;
                    Step::Took(rec)
                } else {
                    Step::Stop
                }
            }
            Err(e) => {
                self.fail(p, format!("TLS: {e}"));
                Step::Stop
            }
        }
    }

    fn dns_head(&mut self, i: usize, check: &mut Check, p: &mut Progress) -> Step {
        let rest = &self.buf[i..];
        if rest.len() < 2 {
            return Step::NeedMore;
        }
        let n = u16::from_be_bytes([rest[0], rest[1]]) as usize;
        if rest.len() < 2 + n {
            return Step::NeedMore;
        }
        match crate::dns::parse(&rest[2..2 + n]) {
            Some(msg) if !msg.is_response => {
                let name = super::fqdn::normalize(&msg.qname);
                if self.verdict(p, Request::Dns { name }, check) {
                    Step::Took(2 + n)
                } else {
                    Step::Stop
                }
            }
            _ => {
                self.fail(p, "not a DNS query".into());
                Step::Stop
            }
        }
    }
}

/// Header block fragment of a HEADERS frame payload (padding and priority
/// removed).
fn h2_fragment(payload: &[u8], flags: u8) -> Option<Vec<u8>> {
    let mut f = payload;
    let mut pad = 0;
    if flags & H2_PADDED != 0 {
        let (&n, tail) = f.split_first()?;
        pad = n as usize;
        f = tail;
    }
    if flags & H2_PRIORITY != 0 {
        f = f.get(5..)?;
    }
    Some(f.get(..f.len().checked_sub(pad)?)?.to_vec())
}

/// Which parser a flow gets, from the L7 rule kinds on its port and the
/// first payload byte.
pub fn pick_kind(kinds: &[&str], first: u8) -> Kind {
    if kinds.contains(&"tls") && (first == 0x16 || kinds.len() == 1) {
        Kind::Tls
    } else if kinds.contains(&"dns") {
        Kind::DnsTcp
    } else if kinds.contains(&"kafka") {
        Kind::Kafka
    } else {
        Kind::Http
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow_get(r: &Request) -> (bool, Option<String>) {
        match r {
            Request::Http(h) => (h.method == "GET" || h.path.starts_with("/up"), None),
            Request::Kafka(k) => (
                k.topics.as_deref() == Some(&["orders".to_string()][..]),
                None,
            ),
            Request::Tls { server_name } => (server_name == "ok.test", None),
            Request::Dns { name } => (name.ends_with("example.com"), None),
        }
    }

    #[test]
    fn http_head_across_segments_then_body() {
        let mut s = Stream::new(Kind::Http);
        let req = b"POST /upload HTTP/1.1\r\nHost: a\r\nContent-Length: 10\r\n\r\n0123456789GET / HTTP/1.1\r\nHost: a\r\n\r\n";
        let p = s.feed(&req[..20], &mut allow_get);
        assert!(!p.denied && p.allowed == 0 && p.verdicts.is_empty());
        let p = s.feed(&req[20..60], &mut allow_get);
        let head = 54;
        assert_eq!(p.verdicts.len(), 1);
        assert_eq!(p.allowed, 60, "head plus the body bytes seen");
        assert_eq!(p.body, 4);
        let p = s.feed(&req[60..], &mut allow_get);
        assert_eq!(p.verdicts.len(), 1, "second request");
        assert_eq!(p.allowed, req.len() as u64);
        assert_eq!(head + 10 + 27, req.len());
    }

    #[test]
    fn http_body_skipped_in_kernel() {
        let mut s = Stream::new(Kind::Http);
        let p = s.feed(
            b"POST /up HTTP/1.1\r\nContent-Length: 1000\r\n\r\n",
            &mut allow_get,
        );
        assert_eq!(p.body, 1000);
        s.skip(1000);
        let p = s.feed(b"DELETE /x HTTP/1.1\r\n\r\n", &mut allow_get);
        assert!(p.denied && p.verdicts.len() == 1 && !p.verdicts[0].allowed);
    }

    #[test]
    fn http_chunked_body_then_next_request() {
        let mut s = Stream::new(Kind::Http);
        let body = b"POST /up HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n3;ext=1\r\nabc\r\n0\r\nX-T: 1\r\n\r\n";
        let mut all = body.to_vec();
        all.extend_from_slice(b"PUT /nope HTTP/1.1\r\n\r\n");
        for (n, b) in all.chunks(7).enumerate() {
            let p = s.feed(b, &mut allow_get);
            if p.denied {
                assert!(n * 7 >= body.len(), "denied only at the PUT");
                assert_eq!(p.allowed, body.len() as u64);
                return;
            }
        }
        panic!("PUT not denied");
    }

    #[test]
    fn http_upgrade_and_h2() {
        let mut s = Stream::new(Kind::Http);
        let p = s.feed(
            b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n\x81\x05hello",
            &mut allow_get,
        );
        assert!(p.open && !p.denied);
        let mut s = Stream::new(Kind::Http);
        let p = s.feed(H2_PREFACE, &mut allow_get);
        assert!(!p.denied && s.is_h2());
        let mut s = Stream::new(Kind::Http);
        assert!(s.feed(b"\x16\x03\x01", &mut allow_get).denied, "not HTTP");
    }

    fn hp(enc: &mut loona_hpack::Encoder, h: &[(&str, &str)]) -> Vec<u8> {
        enc.encode(h.iter().map(|(n, v)| (n.as_bytes(), v.as_bytes())))
    }

    fn h2_frame(ty: u8, flags: u8, sid: u32, payload: &[u8]) -> Vec<u8> {
        let mut f = (payload.len() as u32).to_be_bytes()[1..].to_vec();
        f.extend_from_slice(&[ty, flags]);
        f.extend_from_slice(&sid.to_be_bytes());
        f.extend_from_slice(payload);
        f
    }

    #[test]
    fn h2_requests_checked_data_skipped() {
        let mut enc = loona_hpack::Encoder::new();
        let get = hp(
            &mut enc,
            &[
                (":method", "GET"),
                (":path", "/up/a"),
                (":authority", "svc"),
                ("x-k", "v"),
            ],
        );
        let post = hp(&mut enc, &[(":method", "POST"), (":path", "/admin")]);
        let mut s = Stream::new(Kind::Http);
        let mut c = H2_PREFACE.to_vec();
        c.extend(h2_frame(0x4, 0, 0, &[0, 3, 0, 0, 0, 100]));
        c.extend(h2_frame(H2_HEADERS, H2_END_HEADERS, 1, &get));
        c.extend(h2_frame(H2_DATA, 0x1, 1, &[0u8; 300]));
        let p = s.feed(&c[..c.len() - 200], &mut allow_get);
        assert!(!p.denied && p.verdicts.len() == 1 && p.verdicts[0].allowed);
        match &p.verdicts[0].req {
            Request::Http(h) => assert_eq!(
                (h.method.as_str(), h.host.as_str(), h.path.as_str()),
                ("GET", "svc", "/up/a")
            ),
            r => panic!("{r:?}"),
        }
        assert_eq!(p.body, 200, "rest of the DATA frame passes unseen");
        s.skip(200);
        let p = s.feed(
            &h2_frame(H2_HEADERS, H2_END_HEADERS, 3, &post),
            &mut allow_get,
        );
        assert!(p.denied && !p.verdicts[0].allowed);
    }

    #[test]
    fn h2_continuation_huffman_trailers() {
        // RFC 7541 C.4.1: GET http://www.example.com/ with Huffman strings.
        let block = [
            0x82, 0x86, 0x84, 0x41, 0x8c, 0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab,
            0x90, 0xf4, 0xff,
        ];
        let mut s = Stream::new(Kind::Http);
        let mut c = H2_PREFACE.to_vec();
        c.extend(h2_frame(H2_HEADERS, 0, 1, &block[..5]));
        c.extend(h2_frame(H2_CONTINUATION, H2_END_HEADERS, 1, &block[5..]));
        let p = s.feed(&c, &mut |r: &Request| match r {
            Request::Http(h) => (h.host == "www.example.com" && h.path == "/", None),
            _ => (false, None),
        });
        assert!(
            !p.denied && p.verdicts.len() == 1 && p.verdicts[0].allowed,
            "{p:?}"
        );
        let trailers = hp(&mut loona_hpack::Encoder::new(), &[("grpc-status", "0")]);
        let p = s.feed(
            &h2_frame(H2_HEADERS, H2_END_HEADERS | 0x1, 1, &trailers),
            &mut allow_get,
        );
        assert!(
            !p.denied && p.verdicts.is_empty(),
            "trailers are not requests"
        );
        let p = s.feed(&h2_frame(H2_DATA, 0, 3, b"x"), &mut allow_get);
        assert!(!p.denied);
        let p = s.feed(
            &h2_frame(H2_CONTINUATION, H2_END_HEADERS, 3, b""),
            &mut allow_get,
        );
        assert!(
            p.denied && p.error.is_some(),
            "CONTINUATION without HEADERS"
        );
    }

    #[test]
    fn h2c_upgrade_then_preface() {
        let mut s = Stream::new(Kind::Http);
        let p = s.feed(b"GET /up HTTP/1.1\r\nConnection: Upgrade, HTTP2-Settings\r\nUpgrade: h2c\r\nHTTP2-Settings: AAMAAABkAAQAoAAAAAIAAAAA\r\n\r\n", &mut allow_get);
        assert!(!p.open && !p.denied, "h2c is not a tunnel");
        let mut c = H2_PREFACE.to_vec();
        let post = hp(
            &mut loona_hpack::Encoder::new(),
            &[(":method", "DELETE"), (":path", "/x")],
        );
        c.extend(h2_frame(H2_HEADERS, H2_END_HEADERS, 3, &post));
        assert!(s.feed(&c, &mut allow_get).denied);
    }

    #[test]
    fn tls_and_dns_tcp() {
        let mut s = Stream::new(Kind::Tls);
        let hello = client_hello("ok.test");
        let p = s.feed(&hello[..10], &mut allow_get);
        assert!(!p.open && !p.denied);
        let p = s.feed(&hello[10..], &mut allow_get);
        assert!(p.open);
        let mut s = Stream::new(Kind::Tls);
        assert!(s.feed(&client_hello("bad.test"), &mut allow_get).denied);

        let mut s = Stream::new(Kind::DnsTcp);
        let mut q = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(b"\x03api\x07example\x03com\x00\x00\x01\x00\x01");
        let mut m = (q.len() as u16).to_be_bytes().to_vec();
        m.extend_from_slice(&q);
        let p = s.feed(&m, &mut allow_get);
        assert!(!p.denied && p.allowed == m.len() as u64 && p.verdicts.len() == 1);
    }

    #[test]
    fn kafka_request_read_whole() {
        let mut s = Stream::new(Kind::Kafka);
        let mut body = Vec::new();
        body.extend_from_slice(&0i16.to_be_bytes());
        body.extend_from_slice(&0i16.to_be_bytes());
        body.extend_from_slice(&1i32.to_be_bytes());
        body.extend_from_slice(&(3i16).to_be_bytes());
        body.extend_from_slice(b"cli");
        body.extend_from_slice(&1i16.to_be_bytes());
        body.extend_from_slice(&1000i32.to_be_bytes());
        body.extend_from_slice(&1i32.to_be_bytes());
        body.extend_from_slice(&6i16.to_be_bytes());
        body.extend_from_slice(b"orders");
        body.extend_from_slice(&1i32.to_be_bytes());
        body.extend_from_slice(&0i32.to_be_bytes());
        body.extend_from_slice(&(200_000i32).to_be_bytes());
        body.resize(body.len() + 200_000, 0);
        let mut req = (body.len() as i32).to_be_bytes().to_vec();
        req.extend_from_slice(&body);
        let p = s.feed(&req[..70_000], &mut allow_get);
        assert!(!p.denied && p.verdicts.is_empty() && p.allowed == 0);
        let p = s.feed(&req[70_000..], &mut allow_get);
        assert!(!p.denied && p.verdicts.len() == 1, "{:?}", p.verdicts);
        assert_eq!(p.allowed, req.len() as u64);
    }

    fn client_hello(sni: &str) -> Vec<u8> {
        let name = sni.as_bytes();
        let mut ext = Vec::new();
        ext.extend_from_slice(&0u16.to_be_bytes());
        let list_len = 3 + name.len();
        ext.extend_from_slice(&((list_len + 2) as u16).to_be_bytes());
        ext.extend_from_slice(&(list_len as u16).to_be_bytes());
        ext.push(0);
        ext.extend_from_slice(&(name.len() as u16).to_be_bytes());
        ext.extend_from_slice(name);
        let mut hs = vec![3, 3];
        hs.extend_from_slice(&[0u8; 32]);
        hs.push(0);
        hs.extend_from_slice(&2u16.to_be_bytes());
        hs.extend_from_slice(&[0x13, 0x01]);
        hs.extend_from_slice(&[1, 0]);
        hs.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        hs.extend_from_slice(&ext);
        let mut msg = vec![1];
        msg.extend_from_slice(&(hs.len() as u32).to_be_bytes()[1..]);
        msg.extend_from_slice(&hs);
        let mut rec = vec![0x16, 3, 1];
        rec.extend_from_slice(&(msg.len() as u16).to_be_bytes());
        rec.extend_from_slice(&msg);
        rec
    }
}

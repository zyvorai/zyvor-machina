// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! A reachability witness for HA. With two racks or sites, a controller that has lost its own
//! network sees every host go silent, and without a tie-breaker it would fence them all and restart
//! their VMs elsewhere. A witness is any HTTP(S) endpoint on a third failure domain (a tiny static
//! server, an object-store URL, a monitoring beacon): HA acts only while the controller can reach
//! it. This is an isolation guard, not distributed consensus: it tells the controller it is not the
//! cut-off side, nothing more.
//!
//! Configure with `MACHINA_WITNESS_URLS` (comma-separated, at most 5). `MACHINA_WITNESS_MODE` is
//! `any` (default: one reachable witness is enough) or `majority`. With no URLs set the guard is
//! off and HA behaves as before. Any HTTP response below 500 counts as reachable.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

pub const URLS_ENV: &str = "MACHINA_WITNESS_URLS";
pub const MODE_ENV: &str = "MACHINA_WITNESS_MODE";
const MAX_URLS: usize = 5;
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const CACHE_FOR: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Any,
    Majority,
}

pub fn parse_mode(s: &str) -> Mode {
    if s.trim().eq_ignore_ascii_case("majority") {
        Mode::Majority
    } else {
        Mode::Any
    }
}

/// Valid, de-duplicated http(s) URLs, at most [`MAX_URLS`]. Anything else is ignored.
pub fn parse_urls(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for u in raw.split(',').map(str::trim) {
        let ok = (u.starts_with("http://") || u.starts_with("https://"))
            && u.len() <= 512
            && !u.chars().any(|c| c.is_whitespace() || c.is_control());
        if ok && !out.iter().any(|x| x == u) && out.len() < MAX_URLS {
            out.push(u.to_string());
        }
    }
    out
}

/// Whether `reachable` of `total` witnesses is enough. No witnesses configured is always enough.
pub fn quorum(reachable: usize, total: usize, mode: Mode) -> bool {
    if total == 0 {
        return true;
    }
    match mode {
        Mode::Any => reachable >= 1,
        Mode::Majority => reachable * 2 > total,
    }
}

/// `host:port` of a URL, for display: a witness URL can carry a token in its path or query.
pub fn display_host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |x| x.1);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub configured: bool,
    pub mode: Mode,
    pub witnesses: Vec<String>,
    pub reachable: usize,
    /// True when HA may act: no witness configured, or enough are reachable.
    pub allowed: bool,
}

/// Probe every URL at once; returns how many answered.
pub async fn probe(urls: &[String]) -> usize {
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let checks = urls.iter().map(|u| {
        let client = client.clone();
        let u = u.clone();
        async move {
            client
                .get(&u)
                .send()
                .await
                .is_ok_and(|r| r.status().as_u16() < 500)
        }
    });
    futures_util::future::join_all(checks)
        .await
        .into_iter()
        .filter(|ok| *ok)
        .count()
}

static CACHE: OnceLock<Mutex<Option<(Instant, Status)>>> = OnceLock::new();

/// The current verdict, probed at most every 15 seconds.
pub async fn check() -> Status {
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(g) = cache.lock() {
        if let Some((at, st)) = g.as_ref() {
            if at.elapsed() < CACHE_FOR {
                return st.clone();
            }
        }
    }
    let urls = parse_urls(&std::env::var(URLS_ENV).unwrap_or_default());
    let mode = parse_mode(&std::env::var(MODE_ENV).unwrap_or_default());
    let reachable = if urls.is_empty() {
        0
    } else {
        probe(&urls).await
    };
    let st = Status {
        configured: !urls.is_empty(),
        mode,
        witnesses: urls.iter().map(|u| display_host(u)).collect(),
        reachable,
        allowed: quorum(reachable, urls.len(), mode),
    };
    if let Ok(mut g) = cache.lock() {
        *g = Some((Instant::now(), st.clone()));
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_filtered_deduplicated_and_capped() {
        assert_eq!(
            parse_urls(" https://a.example/w , http://10.0.0.5:8080/ok,https://a.example/w, ftp://x, nonsense ,https://b.example"),
            vec!["https://a.example/w", "http://10.0.0.5:8080/ok", "https://b.example"]
        );
        assert!(parse_urls("").is_empty());
        assert!(parse_urls("https://has space.example").is_empty());
        let many = (0..9)
            .map(|i| format!("https://w{i}.example"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(parse_urls(&many).len(), MAX_URLS);
    }

    #[test]
    fn quorum_rules() {
        // Off: always allowed.
        assert!(quorum(0, 0, Mode::Any) && quorum(0, 0, Mode::Majority));
        // Any: one is enough.
        assert!(quorum(1, 3, Mode::Any));
        assert!(!quorum(0, 3, Mode::Any));
        // Majority: strictly more than half.
        assert!(quorum(2, 3, Mode::Majority));
        assert!(!quorum(1, 2, Mode::Majority));
        assert!(!quorum(1, 3, Mode::Majority));
        assert_eq!(parse_mode("Majority"), Mode::Majority);
        assert_eq!(parse_mode("whatever"), Mode::Any);
    }

    #[test]
    fn display_hides_paths_queries_and_credentials() {
        assert_eq!(
            display_host("https://witness.example:8443/beacon?token=s3cret"),
            "witness.example:8443"
        );
        assert_eq!(display_host("https://user:pw@w.example/x"), "w.example");
        assert_eq!(display_host("http://10.0.0.5"), "10.0.0.5");
    }

    /// A one-shot local HTTP server so the probe is exercised for real.
    async fn serve(status: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock
                        .write_all(format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").as_bytes())
                        .await;
                });
            }
        });
        format!("http://{addr}/")
    }

    #[tokio::test]
    async fn probe_counts_witnesses_that_answer_below_500() {
        let ok = serve("200 OK").await;
        let teapot = serve("404 Not Found").await; // an answer is an answer
        let broken = serve("503 Service Unavailable").await;
        let closed = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}/", l.local_addr().unwrap()) // dropped: nothing listens
        };
        assert_eq!(probe(&[ok.clone()]).await, 1);
        assert_eq!(probe(&[ok, teapot, broken, closed]).await, 2);
        assert_eq!(probe(&[]).await, 0);
    }
}

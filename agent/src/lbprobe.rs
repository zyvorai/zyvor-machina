// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Health probes for load balancer members (`lb.probe`): a TCP connect, or an HTTP GET that must answer 2xx/3xx. The agent
//! runs them because only the host can reach its guests' private addresses.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

const MAX_TARGETS: usize = 128;

#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    pub id: String,
    pub ip: String,
    pub port: u16,
    /// `tcp` or `http`.
    pub kind: String,
    #[serde(default)]
    pub path: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    3000
}

/// Healthy HTTP statuses: 2xx and 3xx.
pub fn http_status_ok(status_line: &str) -> bool {
    let mut parts = status_line.split_whitespace();
    matches!((parts.next(), parts.next().and_then(|c| c.parse::<u16>().ok())), (Some(v), Some(c)) if v.starts_with("HTTP/") && (200..400).contains(&c))
}

fn valid_path(p: &str) -> bool {
    p.starts_with('/') && p.len() <= 256 && p.bytes().all(|b| b.is_ascii_graphic())
}

pub fn probe(t: &Target) -> (bool, String) {
    let Ok(ip) = t.ip.parse::<IpAddr>() else {
        return (false, "bad address".into());
    };
    let timeout = Duration::from_millis(t.timeout_ms.clamp(200, 10_000));
    let addr = SocketAddr::new(ip, t.port);
    let mut stream = match TcpStream::connect_timeout(&addr, timeout) {
        Ok(s) => s,
        Err(e) => return (false, format!("connect: {e}")),
    };
    if t.kind != "http" {
        return (true, "connected".into());
    }
    if !valid_path(&t.path) {
        return (false, "bad path".into());
    }
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let req = format!("GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: machina-health\r\nConnection: close\r\n\r\n", t.path, t.ip);
    if let Err(e) = stream.write_all(req.as_bytes()) {
        return (false, format!("write: {e}"));
    }
    let mut buf = [0u8; 256];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(e) => return (false, format!("read: {e}")),
    };
    let head = String::from_utf8_lossy(&buf[..n]);
    let line = head.lines().next().unwrap_or("").to_string();
    (http_status_ok(&line), if line.is_empty() { "empty response".into() } else { line })
}

/// Apply an `lb.probe` payload `{targets:[…]}` → `{results:[{id, ok, detail}]}`, probing in parallel.
pub fn run(payload: &Value) -> Result<Value, String> {
    let targets: Vec<Target> = serde_json::from_value(payload.get("targets").cloned().unwrap_or(json!([]))).map_err(|e| format!("lb.probe payload: {e}"))?;
    if targets.len() > MAX_TARGETS {
        return Err(format!("at most {MAX_TARGETS} targets per call"));
    }
    let results: Vec<Value> = std::thread::scope(|s| {
        let handles: Vec<_> = targets.iter().map(|t| s.spawn(move || (t.id.clone(), probe(t)))).collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .map(|(id, (ok, detail))| json!({ "id": id, "ok": ok, "detail": detail }))
            .collect()
    });
    Ok(json!({ "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn http_statuses() {
        assert!(http_status_ok("HTTP/1.1 200 OK"));
        assert!(http_status_ok("HTTP/1.0 302 Found"));
        assert!(!http_status_ok("HTTP/1.1 503 Service Unavailable"));
        assert!(!http_status_ok("HTTP/1.1 404 Not Found"));
        assert!(!http_status_ok("garbage"));
        assert!(!http_status_ok(""));
    }

    fn serve_once(reply: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut b = [0u8; 512];
                let _ = s.read(&mut b);
                let _ = s.write_all(reply.as_bytes());
            }
        });
        port
    }

    fn target(kind: &str, port: u16, path: &str) -> Target {
        Target { id: "t".into(), ip: "127.0.0.1".into(), port, kind: kind.into(), path: path.into(), timeout_ms: 1000 }
    }

    #[test]
    fn tcp_probe_connects_or_reports_the_failure() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(probe(&target("tcp", port, "")).0);
        drop(l);
        assert!(!probe(&target("tcp", port, "")).0, "nothing listens any more");
    }

    #[test]
    fn http_probe_needs_a_good_status() {
        assert!(probe(&target("http", serve_once("HTTP/1.0 200 OK\r\n\r\nok"), "/health")).0);
        assert!(!probe(&target("http", serve_once("HTTP/1.0 500 Oops\r\n\r\n"), "/health")).0);
        assert!(!probe(&target("http", serve_once("HTTP/1.0 200 OK\r\n\r\n"), "no-slash")).0, "bad path is refused");
    }

    #[test]
    fn run_probes_every_target_and_limits_the_batch() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let r = run(&json!({ "targets": [{ "id": "a", "ip": "127.0.0.1", "port": port, "kind": "tcp" }, { "id": "b", "ip": "nope", "port": 1, "kind": "tcp" }] })).unwrap();
        let res = r["results"].as_array().unwrap();
        assert_eq!(res.len(), 2);
        assert!(res.iter().any(|x| x["id"] == "a" && x["ok"] == true));
        assert!(res.iter().any(|x| x["id"] == "b" && x["ok"] == false));
        let many: Vec<Value> = (0..129).map(|i| json!({ "id": i.to_string(), "ip": "127.0.0.1", "port": 1, "kind": "tcp" })).collect();
        assert!(run(&json!({ "targets": many })).is_err());
    }
}

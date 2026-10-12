// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Finding BMCs on a management network and enrolling them in bulk.
//!
//! A scan probes the Redfish service root (`/redfish/v1`, unauthenticated by design) on each
//! address in a range, then tries the supplied credentials against `/redfish/v1/Systems`.
//! Credentials are tried in the order given and at most [`MAX_CREDENTIALS`] per BMC, because
//! BMCs lock accounts after a few failures. Scans are limited to private ranges unless
//! `MACHINA_BMC_SCAN_ANY=1`, and to [`MAX_ADDRESSES`] addresses.

use std::net::Ipv4Addr;
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_ADDRESSES: usize = 512;
pub const MAX_CREDENTIALS: usize = 3;
const CONCURRENCY: usize = 32;

#[derive(Debug, Clone, Deserialize)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Discovered {
    pub address: String,
    pub redfish: bool,
    pub redfish_version: String,
    pub vendor: String,
    pub product: String,
    /// Index into the credentials that worked, if any did.
    pub credential_index: Option<usize>,
    pub manufacturer: String,
    pub model: String,
    pub serial_number: String,
    pub error: String,
}

/// IPv4 addresses of `cidr` (`10.20.0.0/24`) or a single address. Network and broadcast
/// addresses are dropped for prefixes shorter than /31. Refuses ranges larger than `max`.
pub fn expand_cidr(cidr: &str, max: usize) -> anyhow::Result<Vec<Ipv4Addr>> {
    let c = cidr.trim();
    let (addr, prefix) = match c.split_once('/') {
        Some((a, p)) => (
            a,
            p.parse::<u32>()
                .map_err(|_| anyhow::anyhow!("bad prefix in '{c}'"))?,
        ),
        None => (c, 32),
    };
    let ip: Ipv4Addr = addr
        .parse()
        .map_err(|_| anyhow::anyhow!("'{addr}' is not an IPv4 address"))?;
    if prefix > 32 {
        anyhow::bail!("prefix must be 0-32");
    }
    let size: u64 = 1u64 << (32 - prefix);
    if size as usize > max {
        anyhow::bail!(
            "{c} has {size} addresses; at most {max} per scan (use a smaller range or several scans)"
        );
    }
    let base = u32::from(ip) & (u32::MAX.checked_shl(32 - prefix).unwrap_or(0));
    let mut out = Vec::new();
    for i in 0..size as u32 {
        let a = base + i;
        if prefix < 31 && (i == 0 || u64::from(i) == size - 1) {
            continue;
        }
        out.push(Ipv4Addr::from(a));
    }
    Ok(out)
}

/// Management networks are private. Loopback, link-local (cloud metadata lives there),
/// multicast and unspecified addresses are never scanned; public ranges only on request.
pub fn scannable(ip: Ipv4Addr, allow_public: bool) -> bool {
    if ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_broadcast()
    {
        return false;
    }
    allow_public || ip.is_private()
}

/// The service-root fields worth keeping.
pub fn parse_service_root(root: &Value) -> (String, String, String) {
    let g = |p: &str| {
        root.pointer(p)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    (g("/RedfishVersion"), g("/Vendor"), g("/Product"))
}

/// Probe one BMC at `base` (`https://10.20.0.14`): service root, then credentials.
pub async fn probe(client: &reqwest::Client, base: &str, creds: &[Credential]) -> Discovered {
    let mut d = Discovered {
        address: base
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string(),
        ..Default::default()
    };
    let root = match client.get(format!("{base}/redfish/v1")).send().await {
        Ok(r) if r.status().is_success() => match r.json::<Value>().await {
            Ok(v) => v,
            Err(_) => return d,
        },
        _ => return d,
    };
    d.redfish = true;
    (d.redfish_version, d.vendor, d.product) = parse_service_root(&root);

    for (i, c) in creds.iter().take(MAX_CREDENTIALS).enumerate() {
        let resp = client
            .get(format!("{base}/redfish/v1/Systems"))
            .basic_auth(&c.username, Some(&c.password))
            .send()
            .await;
        let Ok(resp) = resp else { continue };
        if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
            continue;
        }
        if !resp.status().is_success() {
            d.error = format!("Systems: HTTP {}", resp.status());
            break;
        }
        d.credential_index = Some(i);
        if let Ok(coll) = resp.json::<Value>().await {
            if let Some(uri) = super::baremetal_bmc::first_system_uri(&coll) {
                if let Ok(r) = client
                    .get(format!("{base}{uri}"))
                    .basic_auth(&c.username, Some(&c.password))
                    .send()
                    .await
                {
                    if let Ok(sys) = r.json::<Value>().await {
                        let id = super::baremetal_bmc::parse_identity(&sys);
                        d.manufacturer = id.manufacturer;
                        d.model = id.model;
                        d.serial_number = id.serial_number;
                    }
                }
            }
        }
        break;
    }
    if d.credential_index.is_none() && !creds.is_empty() && d.error.is_empty() {
        d.error = "none of the credentials was accepted".into();
    }
    d
}

fn client() -> anyhow::Result<reqwest::Client> {
    let verify = matches!(
        std::env::var("MACHINA_BMC_VERIFY_TLS").ok().as_deref(),
        Some("1" | "true" | "yes")
    );
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .connect_timeout(Duration::from_secs(2))
        .danger_accept_invalid_certs(!verify)
        .build()?)
}

/// Scan `cidr`; only addresses that answer as Redfish BMCs are returned.
pub async fn discover(
    cidr: &str,
    creds: &[Credential],
    scheme: &str,
    port: Option<u16>,
) -> anyhow::Result<Vec<Discovered>> {
    if scheme != "https" && scheme != "http" {
        anyhow::bail!("scheme must be https or http");
    }
    let allow_public = matches!(
        std::env::var("MACHINA_BMC_SCAN_ANY").ok().as_deref(),
        Some("1" | "true" | "yes")
    );
    let addrs = expand_cidr(cidr, MAX_ADDRESSES)?;
    let targets: Vec<Ipv4Addr> = addrs
        .into_iter()
        .filter(|a| scannable(*a, allow_public))
        .collect();
    if targets.is_empty() {
        anyhow::bail!(
            "no scannable addresses in {cidr} (private ranges only; MACHINA_BMC_SCAN_ANY=1 allows others)"
        );
    }
    let client = client()?;
    let mut found: Vec<Discovered> = stream::iter(targets)
        .map(|ip| {
            let client = client.clone();
            let creds = creds.to_vec();
            let base = match port {
                Some(p) => format!("{scheme}://{ip}:{p}"),
                None => format!("{scheme}://{ip}"),
            };
            async move { probe(&client, &base, &creds).await }
        })
        .buffer_unordered(CONCURRENCY)
        .filter(|d| std::future::ready(d.redfish))
        .collect()
        .await;
    found.sort_by(|a, b| a.address.cmp(&b.address));
    Ok(found)
}

#[derive(Debug, Serialize)]
pub struct ImportRow {
    pub hostname: String,
    pub address: String,
    pub status: String,
    pub id: Option<uuid::Uuid>,
    pub error: String,
}

async fn exists(pool: &crate::db::DbPool, hostname: &str, address: &str) -> anyhow::Result<bool> {
    let n: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM baremetal_servers
         WHERE hostname = ? OR (bmc_address <> '' AND bmc_address = ?)",
    )
    .bind(hostname)
    .bind(address)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

/// Register many servers. A row that already exists (same hostname or BMC address) is
/// skipped, not an error; a bad row is reported and the rest still go through.
pub async fn bulk_import(
    pool: &crate::db::DbPool,
    rows: Vec<super::baremetal::RegisterBaremetalBody>,
) -> anyhow::Result<Vec<ImportRow>> {
    if rows.is_empty() {
        anyhow::bail!("no servers given");
    }
    if rows.len() > 500 {
        anyhow::bail!("at most 500 servers per import");
    }
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let (hostname, address) = (
            r.hostname.trim().to_string(),
            r.bmc_address.trim().to_string(),
        );
        let mut row = ImportRow {
            hostname: hostname.clone(),
            address: address.clone(),
            status: String::new(),
            id: None,
            error: String::new(),
        };
        if hostname.is_empty() {
            row.status = "error".into();
            row.error = "hostname is required".into();
        } else if exists(pool, &hostname, &address).await? {
            row.status = "exists".into();
        } else {
            match super::baremetal::register(pool, &r).await {
                Ok(s) => {
                    row.status = "created".into();
                    row.id = Some(s.id);
                }
                Err(e) => {
                    row.status = "error".into();
                    row.error = e.to_string();
                }
            }
        }
        out.push(row);
    }
    Ok(out)
}

/// Turn authenticated discoveries into registrations (`hostname_prefix` + the address with
/// dashes) and import them.
pub async fn import_discovered(
    pool: &crate::db::DbPool,
    found: &[Discovered],
    creds: &[Credential],
    scheme: &str,
    hostname_prefix: &str,
    zone: &str,
) -> anyhow::Result<Vec<ImportRow>> {
    let rows: Vec<super::baremetal::RegisterBaremetalBody> = found
        .iter()
        .filter_map(|d| {
            let c = creds.get(d.credential_index?)?;
            let host = d.address.split(':').next().unwrap_or(&d.address);
            Some(super::baremetal::RegisterBaremetalBody {
                hostname: format!("{hostname_prefix}{}", host.replace('.', "-")),
                bmc_address: if scheme == "http" {
                    format!("http://{}", d.address)
                } else {
                    d.address.clone()
                },
                bmc_type: "redfish".into(),
                cpu_cores: 0,
                memory_mib: 0,
                firewall_profile: "BareMetalBmc".into(),
                firewall_enabled: true,
                bmc_vlan: String::new(),
                pxe_vlan: String::new(),
                bmc_username: c.username.clone(),
                bmc_password: c.password.clone(),
                mac_address: String::new(),
                zone: zone.to_string(),
                boot_method: "pxe".into(),
            })
        })
        .collect();
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    bulk_import(pool, rows).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_expansion_and_limits() {
        let v = expand_cidr("10.0.0.0/30", 512).unwrap();
        assert_eq!(
            v,
            vec![Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2)]
        );
        assert_eq!(
            expand_cidr("10.0.0.7", 512).unwrap(),
            vec![Ipv4Addr::new(10, 0, 0, 7)]
        );
        assert_eq!(expand_cidr("10.0.0.0/31", 512).unwrap().len(), 2);
        assert_eq!(expand_cidr("10.0.0.0/24", 512).unwrap().len(), 254);
        assert!(expand_cidr("10.0.0.0/16", 512).is_err());
        assert!(expand_cidr("10.0.0.0/33", 512).is_err());
        assert!(expand_cidr("nope", 512).is_err());
        assert!(expand_cidr("::1", 512).is_err());
    }

    #[test]
    fn only_management_networks_are_scannable_by_default() {
        let ip = |a, b, c, d| Ipv4Addr::new(a, b, c, d);
        assert!(scannable(ip(10, 1, 2, 3), false));
        assert!(scannable(ip(192, 168, 1, 1), false));
        assert!(!scannable(ip(127, 0, 0, 1), true));
        assert!(!scannable(ip(169, 254, 169, 254), true));
        assert!(!scannable(ip(8, 8, 8, 8), false));
        assert!(scannable(ip(8, 8, 8, 8), true));
    }

    /// A tiny BMC: unauthenticated service root, Systems behind one Basic credential.
    async fn fake_bmc() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                    // base64("root:calvin")
                    let authed = req.contains("Basic cm9vdDpjYWx2aW4=");
                    let (status, body) = match path.as_str() {
                        "/redfish/v1" => (
                            200,
                            r#"{"RedfishVersion":"1.15.0","Vendor":"Acme","Product":"BMC9000"}"#,
                        ),
                        "/redfish/v1/Systems" if authed => (
                            200,
                            r#"{"Members":[{"@odata.id":"/redfish/v1/Systems/1"}]}"#,
                        ),
                        "/redfish/v1/Systems/1" if authed => (
                            200,
                            r#"{"Manufacturer":"Acme","Model":"R1","SerialNumber":"SN42"}"#,
                        ),
                        "/redfish/v1/Systems" | "/redfish/v1/Systems/1" => (401, "{}"),
                        _ => (404, "{}"),
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        addr
    }

    fn cred(u: &str, p: &str) -> Credential {
        Credential {
            username: u.into(),
            password: p.into(),
        }
    }

    #[tokio::test]
    async fn probe_finds_the_working_credential_and_reads_identity() {
        let base = fake_bmc().await;
        let c = client().unwrap();
        let creds = [cred("admin", "admin"), cred("root", "calvin")];
        let d = probe(&c, &base, &creds).await;
        assert!(d.redfish);
        assert_eq!(d.vendor, "Acme");
        assert_eq!(d.credential_index, Some(1));
        assert_eq!((d.model.as_str(), d.serial_number.as_str()), ("R1", "SN42"));
        // Wrong credentials: still recognised as a BMC, but not authenticated.
        let bad = probe(&c, &base, &[cred("a", "b")]).await;
        assert!(bad.redfish && bad.credential_index.is_none());
        assert!(bad.error.contains("credentials"));
        // Not a BMC at all.
        let none = probe(&c, "http://127.0.0.1:9", &creds).await;
        assert!(!none.redfish);
    }

    #[tokio::test]
    async fn at_most_three_credentials_are_tried() {
        let base = fake_bmc().await;
        let c = client().unwrap();
        // The working credential is fourth: it must not be reached (account lockout).
        let creds = [
            cred("a", "1"),
            cred("b", "2"),
            cred("c", "3"),
            cred("root", "calvin"),
        ];
        let d = probe(&c, &base, &creds).await;
        assert!(d.credential_index.is_none());
    }

    fn reg(host: &str, addr: &str) -> super::super::baremetal::RegisterBaremetalBody {
        super::super::baremetal::RegisterBaremetalBody {
            hostname: host.into(),
            bmc_address: addr.into(),
            bmc_type: "fake".into(),
            cpu_cores: 0,
            memory_mib: 0,
            firewall_profile: "BareMetalBmc".into(),
            firewall_enabled: true,
            bmc_vlan: String::new(),
            pxe_vlan: String::new(),
            bmc_username: String::new(),
            bmc_password: String::new(),
            mac_address: String::new(),
            zone: String::new(),
            boot_method: "pxe".into(),
        }
    }

    #[tokio::test]
    async fn bulk_import_creates_skips_and_reports_per_row() {
        let pool = crate::db::testing::pool().await;
        let rows = vec![
            reg("r1", "10.0.0.1"),
            reg("r2", "10.0.0.2"),
            reg("r1", "10.0.0.9"),
            reg("r3", "10.0.0.2"),
            reg("", "10.0.0.5"),
        ];
        let out = bulk_import(&pool, rows).await.unwrap();
        let st: Vec<&str> = out.iter().map(|r| r.status.as_str()).collect();
        assert_eq!(st, vec!["created", "created", "exists", "exists", "error"]);
        assert!(out[0].id.is_some());
        assert!(bulk_import(&pool, vec![]).await.is_err());
        let again = bulk_import(&pool, vec![reg("r1", "10.0.0.1")])
            .await
            .unwrap();
        assert_eq!(again[0].status, "exists");
    }

    #[tokio::test]
    async fn discovered_bmcs_import_with_the_credential_that_worked() {
        let pool = crate::db::testing::pool().await;
        let found = vec![
            Discovered {
                address: "10.20.0.14".into(),
                redfish: true,
                credential_index: Some(1),
                ..Default::default()
            },
            Discovered {
                address: "10.20.0.15".into(),
                redfish: true,
                credential_index: None,
                ..Default::default()
            },
        ];
        let creds = [cred("a", "x"), cred("root", "calvin")];
        let out = import_discovered(&pool, &found, &creds, "https", "bmc-", "rack2")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].hostname, "bmc-10-20-0-14");
        let (u, z, configured): (String, String, bool) = crate::db::query_as(
            "SELECT bmc_username, zone, (bmc_password_enc <> '') FROM baremetal_servers WHERE hostname = 'bmc-10-20-0-14'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            (u.as_str(), z.as_str(), configured),
            ("root", "rack2", true)
        );
    }

    #[tokio::test]
    async fn discover_refuses_loopback_and_oversized_ranges() {
        assert!(discover("127.0.0.1", &[], "http", None).await.is_err());
        assert!(discover("10.0.0.0/8", &[], "https", None).await.is_err());
        assert!(discover("10.0.0.1", &[], "ftp", None).await.is_err());
    }
}

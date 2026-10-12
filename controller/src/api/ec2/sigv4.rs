// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! AWS Signature Version 4 verification (the server side), enough for awscli/boto3 against the EC2 Query endpoint.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

/// Requests signed more than this far from the server's clock are refused (replay window, like AWS).
pub const MAX_SKEW_SECS: i64 = 15 * 60;

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedAuth {
    pub access_key: String,
    pub date: String,
    pub region: String,
    pub service: String,
    pub signed_headers: Vec<String>,
    pub signature: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SigError {
    Malformed(&'static str),
    Skew,
    Mismatch,
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("hmac accepts any key length");
    m.update(data);
    m.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// `AWS4-HMAC-SHA256 Credential=AKID/20150830/us-east-1/iam/aws4_request, SignedHeaders=host;x-amz-date, Signature=…`
pub fn parse_authorization(h: &str) -> Result<ParsedAuth, SigError> {
    let rest = h
        .strip_prefix("AWS4-HMAC-SHA256")
        .ok_or(SigError::Malformed("unsupported signing algorithm"))?
        .trim();
    let (mut cred, mut signed, mut sig) = (None, None, None);
    for part in rest.split(',') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("Credential=") {
            cred = Some(v);
        } else if let Some(v) = part.strip_prefix("SignedHeaders=") {
            signed = Some(v);
        } else if let Some(v) = part.strip_prefix("Signature=") {
            sig = Some(v);
        }
    }
    let cred = cred.ok_or(SigError::Malformed("missing Credential"))?;
    let mut c = cred.splitn(5, '/');
    let access_key = c.next().unwrap_or_default().to_string();
    let date = c.next().ok_or(SigError::Malformed("bad Credential"))?.to_string();
    let region = c.next().ok_or(SigError::Malformed("bad Credential"))?.to_string();
    let service = c.next().ok_or(SigError::Malformed("bad Credential"))?.to_string();
    if c.next() != Some("aws4_request") || access_key.is_empty() {
        return Err(SigError::Malformed("bad Credential scope"));
    }
    let signed_headers: Vec<String> = signed
        .ok_or(SigError::Malformed("missing SignedHeaders"))?
        .split(';')
        .map(|s| s.to_ascii_lowercase())
        .collect();
    if !signed_headers.iter().any(|h| h == "host") {
        return Err(SigError::Malformed("host must be a signed header"));
    }
    Ok(ParsedAuth {
        access_key,
        date,
        region,
        service,
        signed_headers,
        signature: sig.ok_or(SigError::Malformed("missing Signature"))?.to_string(),
    })
}

fn uri_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Query pairs sorted by encoded name then value, as SigV4 wants.
fn canonical_query(query: &str) -> String {
    let mut pairs: Vec<(String, String)> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (k.to_string(), v.to_string())
        })
        .collect();
    pairs.sort();
    pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

/// The signature the client should have produced.
pub fn expected_signature(
    auth: &ParsedAuth,
    secret: &str,
    method: &str,
    path: &str,
    query: &str,
    headers: &[(String, String)],
    payload: &[u8],
    amz_date: &str,
) -> Result<String, SigError> {
    let mut canon_headers = String::new();
    for name in &auth.signed_headers {
        let v = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.split_whitespace().collect::<Vec<_>>().join(" "))
            .ok_or(SigError::Malformed("a signed header is missing from the request"))?;
        canon_headers.push_str(&format!("{name}:{v}\n"));
    }
    let canonical = format!(
        "{method}\n{}\n{}\n{canon_headers}\n{}\n{}",
        uri_encode(path, true),
        canonical_query(query),
        auth.signed_headers.join(";"),
        sha256_hex(payload)
    );
    let scope = format!("{}/{}/{}/aws4_request", auth.date, auth.region, auth.service);
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", sha256_hex(canonical.as_bytes()));
    let k = hmac(format!("AWS4{secret}").as_bytes(), auth.date.as_bytes());
    let k = hmac(&k, auth.region.as_bytes());
    let k = hmac(&k, auth.service.as_bytes());
    let k = hmac(&k, b"aws4_request");
    Ok(hex::encode(hmac(&k, to_sign.as_bytes())))
}

/// `amz_date` is `YYYYMMDDTHHMMSSZ`; `now_unix` the server clock. Constant-time signature comparison.
pub fn verify(
    auth: &ParsedAuth,
    secret: &str,
    method: &str,
    path: &str,
    query: &str,
    headers: &[(String, String)],
    payload: &[u8],
    amz_date: &str,
    now_unix: i64,
) -> Result<(), SigError> {
    let t = parse_amz_date(amz_date).ok_or(SigError::Malformed("bad X-Amz-Date"))?;
    if (now_unix - t).abs() > MAX_SKEW_SECS {
        return Err(SigError::Skew);
    }
    if !amz_date.starts_with(&auth.date) {
        return Err(SigError::Malformed("credential date does not match X-Amz-Date"));
    }
    let want = expected_signature(auth, secret, method, path, query, headers, payload, amz_date)?;
    let (a, b) = (want.as_bytes(), auth.signature.as_bytes());
    let diff = a.len() ^ b.len() | a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) as usize;
    if diff == 0 {
        Ok(())
    } else {
        Err(SigError::Mismatch)
    }
}

/// `20150830T123600Z` → unix seconds.
pub fn parse_amz_date(s: &str) -> Option<i64> {
    if s.len() != 16 || !s.ends_with('Z') || s.as_bytes()[8] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(4..6)?, n(6..8)?, n(9..11)?, n(11..13)?, n(13..15)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    // days from civil (Howard Hinnant)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
    const AUTH: &str = "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7";

    fn headers() -> Vec<(String, String)> {
        vec![
            ("Content-Type".into(), "application/x-www-form-urlencoded; charset=utf-8".into()),
            ("Host".into(), "iam.amazonaws.com".into()),
            ("X-Amz-Date".into(), "20150830T123600Z".into()),
        ]
    }

    /// The worked example from AWS's SigV4 documentation (IAM ListUsers).
    #[test]
    fn matches_the_aws_documented_example() {
        let auth = parse_authorization(AUTH).unwrap();
        let sig = expected_signature(&auth, SECRET, "GET", "/", "Action=ListUsers&Version=2010-05-08", &headers(), b"", "20150830T123600Z").unwrap();
        assert_eq!(sig, "5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7");
    }

    #[test]
    fn verify_accepts_good_refuses_tampered_and_stale() {
        let auth = parse_authorization(AUTH).unwrap();
        let now = parse_amz_date("20150830T123600Z").unwrap();
        let q = "Action=ListUsers&Version=2010-05-08";
        assert_eq!(verify(&auth, SECRET, "GET", "/", q, &headers(), b"", "20150830T123600Z", now + 60), Ok(()));
        assert_eq!(verify(&auth, SECRET, "GET", "/", "Action=DeleteUser&Version=2010-05-08", &headers(), b"", "20150830T123600Z", now), Err(SigError::Mismatch));
        assert_eq!(verify(&auth, "wrong", "GET", "/", q, &headers(), b"", "20150830T123600Z", now), Err(SigError::Mismatch));
        assert_eq!(verify(&auth, SECRET, "GET", "/", q, &headers(), b"", "20150830T123600Z", now + MAX_SKEW_SECS + 1), Err(SigError::Skew));
    }

    #[test]
    fn query_order_does_not_matter() {
        let auth = parse_authorization(AUTH).unwrap();
        let a = expected_signature(&auth, SECRET, "GET", "/", "Version=2010-05-08&Action=ListUsers", &headers(), b"", "20150830T123600Z").unwrap();
        assert_eq!(a, "5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7");
    }

    #[test]
    fn malformed_headers_are_refused() {
        assert!(parse_authorization("Bearer abc").is_err());
        assert!(parse_authorization("AWS4-HMAC-SHA256 Credential=A/20150830/us-east-1/iam/aws4_request, SignedHeaders=content-type, Signature=00").is_err(), "host must be signed");
        assert!(parse_authorization("AWS4-HMAC-SHA256 Credential=A/20150830/us-east-1/iam, SignedHeaders=host, Signature=00").is_err());
    }

    #[test]
    fn amz_date_parses_to_unix_time() {
        assert_eq!(parse_amz_date("19700101T000000Z"), Some(0));
        assert_eq!(parse_amz_date("20000301T000000Z"), Some(951_868_800));
        assert_eq!(parse_amz_date("2015-08-30"), None);
        assert_eq!(parse_amz_date("20151330T123600Z"), None);
    }
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! DNS threat feeds: domain lists matched against the names in VM DNS
//! replies. A listed domain also covers its subdomains.

use std::collections::{BTreeSet, HashMap};

/// Domains in one feed at most.
pub const MAX_DOMAINS: usize = 500_000;
/// Largest feed download.
pub const MAX_FEED_BYTES: usize = 64 << 20;
/// URL feeds are fetched again this often.
pub const REFRESH_SECS: u64 = 12 * 3600;

/// `PUT .../threat-feeds/{name}`: one of a URL, a domain list or feed text.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct FeedBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub block: bool,
}

pub fn check_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
    if ok {
        Ok(())
    } else {
        Err("threat feed name: 1-64 of [A-Za-z0-9._-]".into())
    }
}

impl FeedBody {
    pub fn validate(&self) -> Result<(), String> {
        let n = usize::from(self.url.is_some())
            + usize::from(!self.domains.is_empty())
            + usize::from(self.text.is_some());
        if n != 1 {
            return Err("give exactly one of url, domains or text".into());
        }
        if let Some(u) = &self.url {
            if !(u.starts_with("https://") || u.starts_with("http://")) || u.len() > 2048 {
                return Err("feed url must be http(s)://…".into());
            }
        }
        Ok(())
    }

    /// The feed's domains; `fetched` is the downloaded body of a URL feed.
    pub fn domains(&self, fetched: Option<&str>) -> Result<Vec<String>, String> {
        let list = match (&self.text, fetched) {
            (Some(t), _) => parse_list(t),
            (None, Some(t)) => parse_list(t),
            (None, None) => parse_list(&self.domains.join("\n")),
        };
        if list.is_empty() {
            return Err("the feed has no valid domains".into());
        }
        Ok(list)
    }

    pub fn source(&self) -> String {
        self.url.clone().unwrap_or_default()
    }
}

/// Domains from a feed body. Accepts one domain per line, hosts files
/// (`0.0.0.0 evil.example`), Adblock rules (`||evil.example^`) and URLs;
/// `#` and `!` start comments. Sorted, deduplicated, invalid entries dropped.
pub fn parse_list(text: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let first = fields.next().unwrap_or("");
        let candidate = if first.parse::<std::net::IpAddr>().is_ok() {
            match fields.next() {
                Some(h) => h,
                None => continue,
            }
        } else {
            first
        };
        if let Some(d) = normalize(candidate) {
            out.insert(d);
            if out.len() >= MAX_DOMAINS {
                break;
            }
        }
    }
    out.into_iter().collect()
}

/// `||Evil.Example^`, `*.evil.example.`, `https://evil.example/x` → `evil.example`.
pub fn normalize(s: &str) -> Option<String> {
    let mut d = s.trim();
    d = d.strip_prefix("||").unwrap_or(d);
    if let Some((_, rest)) = d.split_once("://") {
        d = rest;
    }
    d = d.split(['/', '^', '$', ':', '?']).next().unwrap_or("");
    d = d.strip_prefix("*.").unwrap_or(d);
    let d = d.trim_end_matches('.').to_ascii_lowercase();
    let ok = d.len() <= 253
        && d.contains('.')
        && !matches!(d.as_str(), "localhost" | "localhost.localdomain" | "local")
        && d.parse::<std::net::IpAddr>().is_err()
        && d.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
    ok.then_some(d)
}

/// The listed domain covering `name` (itself or a parent), if any.
pub fn lookup<'a, V>(list: &'a HashMap<String, V>, name: &str) -> Option<(&'a str, &'a V)> {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let mut rest = name.as_str();
    loop {
        if let Some((k, v)) = list.get_key_value(rest) {
            return Some((k.as_str(), v));
        }
        match rest.split_once('.') {
            Some((_, parent)) if parent.contains('.') => rest = parent,
            _ => return None,
        }
    }
}

/// `a.b.example.org` → `example.org`: the unit `new_domain` alerts count.
pub fn base_domain(name: &str) -> String {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = name.rsplitn(3, '.').collect();
    match labels.as_slice() {
        [tld, sld, ..] => format!("{sld}.{tld}"),
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_feed_formats() {
        let text = "# urlhaus\n0.0.0.0 Evil.Example\n127.0.0.1 localhost\n||ads.example^\n\
                    *.c2.example.\nhttps://phish.example/login\nplain.example # note\n\
                    ! adblock comment\nnot_a_domain\n10.0.0.1\n";
        assert_eq!(
            parse_list(text),
            [
                "ads.example",
                "c2.example",
                "evil.example",
                "phish.example",
                "plain.example"
            ]
        );
    }

    #[test]
    fn lookup_covers_subdomains_only() {
        let list: HashMap<String, u8> = [("evil.example".to_string(), 1)].into();
        assert_eq!(
            lookup(&list, "evil.example.").map(|h| h.0),
            Some("evil.example")
        );
        assert_eq!(
            lookup(&list, "A.b.Evil.Example").map(|h| h.0),
            Some("evil.example")
        );
        assert!(lookup(&list, "notevil.example").is_none());
        assert!(lookup(&list, "example").is_none());
        assert_eq!(base_domain("a.b.example.org."), "example.org");
        assert_eq!(base_domain("example.org"), "example.org");
    }

    #[test]
    fn feed_bodies_take_one_source() {
        let both = FeedBody {
            url: Some("https://x.example/list".into()),
            text: Some("a.example".into()),
            ..Default::default()
        };
        assert!(both.validate().is_err());
        let ftp = FeedBody {
            url: Some("ftp://x.example/list".into()),
            ..Default::default()
        };
        assert!(ftp.validate().is_err());
        let inline = FeedBody {
            domains: vec!["B.example".into(), "bad".into()],
            ..Default::default()
        };
        assert!(inline.validate().is_ok());
        assert_eq!(inline.domains(None).unwrap(), ["b.example"]);
        let url = FeedBody {
            url: Some("https://x.example/list".into()),
            ..Default::default()
        };
        assert_eq!(url.domains(Some("||c.example^")).unwrap(), ["c.example"]);
        assert!(url.domains(Some("# nothing")).is_err());
    }
}

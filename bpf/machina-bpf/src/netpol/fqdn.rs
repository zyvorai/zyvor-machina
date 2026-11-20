// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `toFQDNs` selectors, matched the way Cilium does: `matchName` is exact;
//! in `matchPattern` `*` matches DNS characters within one label, a lone
//! `*` matches every name and a leading `**.` matches one or more labels.

use serde_json::Value;

/// Lower-case, no trailing dot.
pub fn normalize(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn dns_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

/// `None` when valid, otherwise why not.
pub fn invalid(sel: &str, pattern: bool) -> Option<&'static str> {
    let s = normalize(sel);
    if s.is_empty() {
        return Some("must not be empty");
    }
    if s.len() > 253 {
        return Some("longer than 253 characters");
    }
    let body = if pattern {
        s.strip_prefix("**.").unwrap_or(&s)
    } else {
        &s
    };
    if !body
        .bytes()
        .all(|c| dns_char(c) || c == b'.' || (pattern && c == b'*'))
    {
        return Some(if pattern {
            "only letters, digits, `-`, `_`, `.` and `*` are allowed"
        } else {
            "only letters, digits, `-`, `_` and `.` are allowed (use matchPattern for wildcards)"
        });
    }
    if body.split('.').any(str::is_empty) {
        return Some("empty label");
    }
    None
}

/// One-label glob: `*` never crosses a dot.
fn glob(p: &[u8], n: &[u8]) -> bool {
    match p.split_first() {
        None => n.is_empty(),
        Some((b'*', rest)) => {
            let mut i = 0;
            loop {
                if glob(rest, &n[i..]) {
                    return true;
                }
                if i < n.len() && dns_char(n[i]) {
                    i += 1;
                } else {
                    return false;
                }
            }
        }
        Some((c, rest)) => n.first() == Some(c) && glob(rest, &n[1..]),
    }
}

/// `pattern` is a normalized selector (`matchName` text has no `*`).
pub fn matches(pattern: &str, name: &str) -> bool {
    let name = normalize(name);
    if pattern == "*" {
        return !name.is_empty();
    }
    if let Some(rest) = pattern.strip_prefix("**.") {
        return name
            .char_indices()
            .filter(|(_, c)| *c == '.')
            .any(|(i, _)| i > 0 && glob(rest.as_bytes(), &name.as_bytes()[i + 1..]));
    }
    glob(pattern.as_bytes(), name.as_bytes())
}

/// Selectors of one `toFQDNs` list, normalized; `matchName` stays literal.
pub fn selectors(list: &[Value]) -> Vec<String> {
    list.iter()
        .filter_map(|s| {
            s.get("matchName")
                .or_else(|| s.get("matchPattern"))
                .and_then(Value::as_str)
        })
        .map(normalize)
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cilium_pattern_semantics() {
        assert!(matches("example.com", "Example.COM."));
        assert!(!matches("example.com", "www.example.com"));
        assert!(matches("*.example.com", "api.example.com"));
        assert!(!matches("*.example.com", "a.b.example.com"));
        assert!(!matches("*.example.com", "example.com"));
        assert!(matches("**.example.com", "a.b.example.com"));
        assert!(matches("**.example.com", "api.example.com"));
        assert!(!matches("**.example.com", "example.com"));
        assert!(matches("api-*.example.com", "api-eu1.example.com"));
        assert!(matches("*", "anything.at.all"));
        assert!(matches("s*3.amazonaws.com", "s3.amazonaws.com"));
    }

    #[test]
    fn validation() {
        assert!(invalid("example.com", false).is_none());
        assert!(invalid("*.example.com", false).is_some());
        assert!(invalid("**.example.com", true).is_none());
        assert!(invalid("bad..name", false).is_some());
        assert!(invalid("bad name", true).is_some());
    }
}

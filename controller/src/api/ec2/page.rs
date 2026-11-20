// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Opaque `NextToken` for EC2 describe calls. A token is the last id already returned, so the
//! next page is every id strictly greater than it. Short pages are the end: no empty token.

use base64::Engine;

/// Default and cap match the EC2 query API's practical page size, not AWS's per-action limits.
pub const DEFAULT_MAX_RESULTS: usize = 100;
pub const MAX_MAX_RESULTS: usize = 1000;

pub fn max_results(raw: Option<&str>) -> Result<usize, String> {
    let n = match raw {
        None => return Ok(DEFAULT_MAX_RESULTS),
        Some(s) => s.parse::<usize>().map_err(|_| "MaxResults must be a number".to_string())?,
    };
    if n == 0 || n > MAX_MAX_RESULTS {
        return Err(format!("MaxResults must be between 1 and {MAX_MAX_RESULTS}"));
    }
    Ok(n)
}

pub fn encode(last_id: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(last_id.as_bytes())
}

pub fn decode(token: &str) -> Result<String, String> {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.trim())
        .map_err(|_| "NextToken is not valid".to_string())?;
    let id = String::from_utf8(raw).map_err(|_| "NextToken is not valid".to_string())?;
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("NextToken is not valid".to_string());
    }
    Ok(id)
}

/// `ids` must already be in the order pages walk (EC2 id ascending). Returns the page and the
/// token to send back, if another page remains.
pub fn page<'a>(ids: &'a [String], token: Option<&str>, limit: usize) -> Result<(&'a [String], Option<String>), String> {
    let start = match token {
        None => 0,
        Some(t) => {
            let last = decode(t)?;
            ids.iter().position(|id| id == &last).map(|i| i + 1).unwrap_or(0)
        }
    };
    let rest = &ids[start.min(ids.len())..];
    if rest.len() <= limit {
        return Ok((rest, None));
    }
    let slice = &rest[..limit];
    Ok((slice, Some(encode(&slice[slice.len() - 1]))))
}

pub fn token_xml(token: &Option<String>) -> String {
    match token {
        Some(t) => format!("<nextToken>{}</nextToken>", super::xml_escape(t)),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        (1..=5).map(|n| format!("i-{n:017x}")).collect()
    }

    #[test]
    fn first_page_carries_a_token_when_more_remain() {
        let all = ids();
        let (slice, token) = page(&all, None, 2).unwrap();
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0], all[0]);
        let (next, end) = page(&all, token.as_deref(), 2).unwrap();
        assert_eq!(next[0], all[2]);
        assert!(end.is_some());
        let (last, done) = page(&all, end.as_deref(), 2).unwrap();
        assert_eq!(last, &all[4..]);
        assert!(done.is_none());
    }

    #[test]
    fn a_short_page_is_the_end() {
        let all = ids();
        let (slice, token) = page(&all, None, 100).unwrap();
        assert_eq!(slice.len(), 5);
        assert!(token.is_none());
    }

    #[test]
    fn garbage_token_is_rejected() {
        assert!(decode("@@@").is_err());
        assert!(max_results(Some("0")).is_err());
        assert!(max_results(Some("1001")).is_err());
        assert_eq!(max_results(None).unwrap(), DEFAULT_MAX_RESULTS);
    }
}

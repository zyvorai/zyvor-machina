// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Extract networking hints from libvirt XML snippets (domain / network definitions).

use std::collections::HashSet;

/// First `<ip address='v4' …>` on an IPv4-looking address (typical libvirt NAT bridge IP on the host).
pub fn ipv4_gateway_from_network_xml(xml: &str) -> Option<String> {
    let mut start = 0usize;
    while let Some(rel) = xml.get(start..).and_then(|s| s.find("address='")) {
        let i = start + rel + "address='".len();
        let rest = xml.get(i..)?;
        let end = rest.find('\'')?;
        let addr = &rest[..end];
        if addr.contains('.') && addr.parse::<std::net::Ipv4Addr>().is_ok() {
            return Some(addr.to_string());
        }
        start = i + end;
    }
    None
}

/// `<source network='foo'/>` or `network="foo"` names referenced by a domain XML.
pub fn network_names_from_domain_xml(xml: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut start = 0usize;
    while let Some((name, next)) = next_network_source_name(xml, start) {
        if seen.insert(name.clone()) {
            out.push(name);
        }
        start = next;
    }
    out
}

fn next_network_source_name(xml: &str, start: usize) -> Option<(String, usize)> {
    // Only accept a `network=` attribute that lives inside a `<source …>` tag. A raw
    // substring search for `network='` over the whole domain XML would also match
    // metadata/title/description text or unrelated attributes, causing `start_vm` to
    // try to activate a bogus "network". Scope each match to a single `<source>` tag.
    let mut cursor = start;
    loop {
        let tail = xml.get(cursor..)?;
        let src_rel = tail.find("<source")?;
        let src_abs = cursor + src_rel;
        let tag_tail = xml.get(src_abs..)?;
        let gt = tag_tail.find('>')?;
        let tag = &tag_tail[..gt]; // just this tag's contents, no '>'
        let next_cursor = src_abs + gt + 1;

        let attr = tag
            .find("network='")
            .map(|r| (r, '\'', "network='".len()))
            .or_else(|| tag.find("network=\"").map(|r| (r, '"', "network=\"".len())));
        if let Some((rel, quote, key_len)) = attr {
            let rest = &tag[rel + key_len..];
            if let Some(end) = rest.find(quote) {
                return Some((rest[..end].to_string(), next_cursor));
            }
        }
        cursor = next_cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::network_names_from_domain_xml;

    #[test]
    fn domain_xml_single_quoted_network_sources() {
        let xml = r#"<interface type='network'><source network='default'/></interface>"#;
        assert_eq!(
            network_names_from_domain_xml(xml),
            vec!["default".to_string()]
        );
    }

    #[test]
    fn domain_xml_double_quoted_network_sources() {
        let xml = r#"<interface type="network"><source network="default"/></interface>"#;
        assert_eq!(
            network_names_from_domain_xml(xml),
            vec!["default".to_string()]
        );
    }

    #[test]
    fn domain_xml_prefers_earliest_network_source() {
        let xml = concat!(
            r#"<interface type='network'><source network='net-a'/>"#,
            r#"<source network='net-b'/></interface>"#
        );
        assert_eq!(
            network_names_from_domain_xml(xml),
            vec!["net-a".to_string(), "net-b".to_string()]
        );
    }
}

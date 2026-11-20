// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Escape special XML characters in a string.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}

/// Extract the text content of the first occurrence of `<tag ...>content</tag>`.
pub fn extract_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)?;
    let after_open = &xml[start..];
    let gt = after_open.find('>')?;
    let content_start = start + gt + 1;
    let end = xml[content_start..].find(&close)?;
    Some(xml[content_start..content_start + end].trim().to_string())
}

/// Extract an attribute value from the first matching `<tag attr="value" ...>`.
pub fn extract_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let mut search_from = 0;
    while let Some(pos) = xml[search_from..].find(&open) {
        let abs = search_from + pos;
        let after_tag = abs + open.len();
        // Ensure word boundary (next char is space, >, /)
        if let Some(ch) = xml[after_tag..].chars().next() {
            if ch != ' ' && ch != '>' && ch != '/' && ch != '\n' {
                search_from = after_tag;
                continue;
            }
        }
        let after = &xml[abs..];
        // No closing '>' after this '<tag' → no complete tag remains; stop.
        let Some(end) = after.find('>') else { break };
        let tag_content = &after[..end];
        // Try double quotes. A malformed value (opening quote never closed within
        // this tag) must skip to the NEXT candidate, not abort the whole search.
        let dq_pattern = format!("{}=\"", attr);
        if let Some(attr_start) = tag_content.find(&dq_pattern) {
            let value_start = attr_start + dq_pattern.len();
            if let Some(value_end) = tag_content[value_start..].find('"') {
                return Some(tag_content[value_start..value_start + value_end].to_string());
            }
        }
        // Try single quotes
        let sq_pattern = format!("{}='", attr);
        if let Some(attr_start) = tag_content.find(&sq_pattern) {
            let value_start = attr_start + sq_pattern.len();
            if let Some(value_end) = tag_content[value_start..].find('\'') {
                return Some(tag_content[value_start..value_start + value_end].to_string());
            }
        }
        search_from = after_tag;
    }
    None
}

/// Split XML into blocks delimited by `<tag ...>...</tag>` or self-closing `<tag .../>`.
pub fn split_blocks(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut blocks = Vec::new();
    let mut search_from = 0;

    while let Some(start) = xml[search_from..].find(&open) {
        let abs_start = search_from + start;
        let after_tag = abs_start + open.len();
        // Ensure word boundary
        if let Some(ch) = xml[after_tag..].chars().next() {
            if ch != ' ' && ch != '>' && ch != '/' && ch != '\n' {
                search_from = after_tag;
                continue;
            }
        }
        // Try to find closing tag
        if let Some(end) = xml[abs_start..].find(&close) {
            let abs_end = abs_start + end + close.len();
            blocks.push(xml[abs_start..abs_end].to_string());
            search_from = abs_end;
        } else {
            // Self-closing or no close tag, find the next '>'
            if let Some(gt) = xml[abs_start..].find('>') {
                let abs_end = abs_start + gt + 1;
                blocks.push(xml[abs_start..abs_end].to_string());
                search_from = abs_end;
            } else {
                break;
            }
        }
    }
    blocks
}

/// Strip runtime-only `<alias .../>` and `<address .../>` self-closing elements from a
/// device XML block extracted from a LIVE domain description.
///
/// libvirt assigns both to every device on a running domain but never writes them to the
/// domain's persistent (offline) definition. When a domain is active,
/// `detach_device_flags()` is typically called with combined LIVE+CONFIG affect flags —
/// matching an XML that still carries `<alias>`/`<address>` against the CONFIG side (which
/// has neither) fails with a spurious "device not found ... matching ... alias 'xxx'" even
/// though the device plainly exists. Passing a block through this function first keeps the
/// identifying fields (mac/model/backend/etc.) while dropping the two that only make sense
/// for the live side.
pub fn strip_runtime_only_attrs(block: &str) -> String {
    block
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !(t.starts_with("<alias") || t.starts_with("<address"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract the text content of a simple `<tag>content</tag>` (no attributes).
pub fn extract_simple_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)?;
    let content_start = start + open.len();
    let end = xml[content_start..].find(&close)?;
    Some(xml[content_start..content_start + end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_special_chars() {
        assert_eq!(escape("hello"), "hello");
        assert_eq!(escape("<script>"), "&lt;script&gt;");
        assert_eq!(escape("a&b"), "a&amp;b");
        assert_eq!(escape("it's"), "it&apos;s");
        assert_eq!(escape(r#"say "hi""#), "say &quot;hi&quot;");
        assert_eq!(
            escape("foo</name><name>bar"),
            "foo&lt;/name&gt;&lt;name&gt;bar"
        );
    }

    #[test]
    fn test_escape_empty() {
        assert_eq!(escape(""), "");
    }

    #[test]
    fn test_extract_text() {
        let xml = "<domain><name>myvm</name></domain>";
        assert_eq!(extract_text(xml, "name"), Some("myvm".to_string()));
        assert_eq!(extract_text(xml, "missing"), None);
    }

    #[test]
    fn extract_attr_basic() {
        let xml = "<graphics type='vnc' port='5900'/>";
        assert_eq!(extract_attr(xml, "graphics", "type"), Some("vnc".into()));
        assert_eq!(extract_attr(xml, "graphics", "port"), Some("5900".into()));
        assert_eq!(extract_attr(xml, "graphics", "missing"), None);
    }

    #[test]
    fn extract_attr_skips_malformed_earlier_tag() {
        // The first <disk> has no attr; a malformed <disk> (unterminated quote)
        // must not abort the search — the well-formed one after it should match.
        // Previously the `?` on the missing closing quote returned None outright.
        let xml = "<disk device='cdrom'/><disk device='disk'/>";
        assert_eq!(extract_attr(xml, "disk", "device"), Some("cdrom".into()));
        // Attribute present only on a later occurrence.
        let xml2 = "<disk/><disk bus='virtio'/>";
        assert_eq!(extract_attr(xml2, "disk", "bus"), Some("virtio".into()));
    }

    #[test]
    fn test_extract_attr() {
        let xml = r#"<type arch='x86_64' machine='pc'>hvm</type>"#;
        assert_eq!(
            extract_attr(xml, "type", "arch"),
            Some("x86_64".to_string())
        );
        assert_eq!(extract_attr(xml, "type", "machine"), Some("pc".to_string()));
        assert_eq!(extract_attr(xml, "type", "missing"), None);
    }

    #[test]
    fn test_extract_attr_double_quotes() {
        let xml = r#"<source file="/var/lib/test.img"/>"#;
        assert_eq!(
            extract_attr(xml, "source", "file"),
            Some("/var/lib/test.img".to_string())
        );
    }

    #[test]
    fn test_extract_attr_word_boundary() {
        let xml = r#"<interface type='network'><source network='default'/></interface>"#;
        assert_eq!(
            extract_attr(xml, "interface", "type"),
            Some("network".to_string())
        );
        assert_eq!(
            extract_attr(xml, "source", "network"),
            Some("default".to_string())
        );
    }

    #[test]
    fn test_split_blocks() {
        let xml = r#"<devices><disk type='file'><source file='a'/></disk><disk type='block'><source dev='b'/></disk></devices>"#;
        let blocks = split_blocks(xml, "disk");
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].contains("file='a'"));
        assert!(blocks[1].contains("dev='b'"));
    }

    #[test]
    fn test_split_blocks_self_closing() {
        let xml = r#"<root><item/><item name='x'/></root>"#;
        let blocks = split_blocks(xml, "item");
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn test_extract_simple_text() {
        let xml = "<parent><name>snap1</name></parent>";
        assert_eq!(extract_simple_text(xml, "name"), Some("snap1".to_string()));
        assert_eq!(extract_simple_text(xml, "missing"), None);
    }

    #[test]
    fn test_extract_simple_text_whitespace() {
        let xml = "<state>  running  </state>";
        assert_eq!(
            extract_simple_text(xml, "state"),
            Some("running".to_string())
        );
    }
}

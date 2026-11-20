// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Parse `kubectl top` table rows (`--no-headers`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct K8sTopRow {
    pub name: String,
    pub cpu: String,
    pub cpu_percent: String,
    pub memory: String,
    pub memory_percent: String,
}

/// Parse one line from `kubectl top nodes` or `kubectl top pods` (`--no-headers`).
pub fn parse_kubectl_top_line(line: &str) -> Option<K8sTopRow> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    Some(K8sTopRow {
        name: parts[0].to_string(),
        cpu: parts[1].to_string(),
        cpu_percent: parts[2].to_string(),
        memory: parts[3].to_string(),
        memory_percent: parts[4].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_line() {
        let row = parse_kubectl_top_line("node1 100m 5% 1024Mi 12%").unwrap();
        assert_eq!(row.name, "node1");
        assert_eq!(row.cpu, "100m");
        assert_eq!(row.cpu_percent, "5%");
        assert_eq!(row.memory, "1024Mi");
        assert_eq!(row.memory_percent, "12%");
    }

    #[test]
    fn parses_pod_line_with_namespace_in_name() {
        let row = parse_kubectl_top_line("kube-system/coredns-abc 2m 1% 15Mi 0%").unwrap();
        assert_eq!(row.name, "kube-system/coredns-abc");
    }

    #[test]
    fn rejects_short_line() {
        assert!(parse_kubectl_top_line("only three parts").is_none());
    }
}

// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Minimal Prometheus text exposition parser (gauge/counter lines only).

/// One sample from a text scrape (`name value` or `name{labels} value`).
#[derive(Debug, Clone, PartialEq)]
pub struct PrometheusSample {
    pub name: String,
    pub value: f64,
}

/// Parse `application/openmetrics-text` or Prometheus text format lines.
pub fn parse_prometheus_text(body: &str) -> Vec<PrometheusSample> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name_part, value_str)) = line.rsplit_once(' ') else {
            continue;
        };
        let Ok(value) = value_str.trim().parse::<f64>() else {
            continue;
        };
        let name = name_part
            .split('{')
            .next()
            .unwrap_or(name_part)
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        out.push(PrometheusSample { name, value });
    }
    out
}

/// Inject `machina_peer` into each metric line of a Prometheus text exposition.
pub fn inject_peer_label(exposition: &str, peer: &str) -> String {
    let peer_label = format!("machina_peer=\"{}\"", escape_label_value(peer));
    let mut out = String::new();
    for line in exposition.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Some((name_part, value_part)) = line.rsplit_once(' ') {
            let injected = if let Some(idx) = name_part.rfind('}') {
                format!("{},{}{}", &name_part[..idx], peer_label, &name_part[idx..])
            } else {
                format!("{name_part}{{{peer_label}}}")
            };
            out.push_str(&injected);
            out.push(' ');
            out.push_str(value_part.trim());
            out.push('\n');
        }
    }
    out
}

fn escape_label_value(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Map Machina-exported metric names to host utilization fields when present.
pub fn host_percents_from_samples(samples: &[PrometheusSample]) -> Option<(f64, f64, f64)> {
    let mut cpu = None;
    let mut mem = None;
    let mut disk = None;
    for s in samples {
        match s.name.as_str() {
            "machina_host_cpu_percent" => cpu = Some(s.value),
            "machina_host_memory_percent" => mem = Some(s.value),
            "machina_host_disk_percent" => disk = Some(s.value),
            _ => {}
        }
    }
    match (cpu, mem, disk) {
        (Some(c), Some(m), Some(d)) => Some((c, m, d)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gauge_line() {
        let samples = parse_prometheus_text("machina_host_cpu_percent 42.5\n");
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "machina_host_cpu_percent");
        assert!((samples[0].value - 42.5).abs() < 0.01);
    }

    #[test]
    fn skips_comments() {
        assert!(parse_prometheus_text("# HELP x\n").is_empty());
    }

    #[test]
    fn inject_peer_label_adds_label() {
        let out = inject_peer_label("machina_host_cpu_percent 42\n", "peer-a");
        assert!(out.contains("machina_peer=\"peer-a\""));
        assert!(out.contains("machina_host_cpu_percent"));
    }

    #[test]
    fn extracts_host_triplet() {
        let body = "machina_host_cpu_percent 10\nmachina_host_memory_percent 20\nmachina_host_disk_percent 30\n";
        let (c, m, d) = host_percents_from_samples(&parse_prometheus_text(body)).unwrap();
        assert!((c - 10.0).abs() < 0.01);
        assert!((m - 20.0).abs() < 0.01);
        assert!((d - 30.0).abs() < 0.01);
    }
}

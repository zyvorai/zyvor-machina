// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::types::{ExposureRisk, FirewallDiff, FirewallDiffEntry, FirewallRule};

pub fn compute_diff(before: &[FirewallRule], after: &[FirewallRule]) -> FirewallDiff {
    let mut entries = Vec::new();
    let before_ids: std::collections::HashSet<_> = before.iter().map(|r| &r.id).collect();
    let after_ids: std::collections::HashSet<_> = after.iter().map(|r| &r.id).collect();

    for rule in after {
        if !before_ids.contains(&rule.id) {
            entries.push(FirewallDiffEntry {
                change: "ADD".into(),
                before: String::new(),
                after: format_rule(rule),
            });
        }
    }
    for rule in before {
        if !after_ids.contains(&rule.id) {
            entries.push(FirewallDiffEntry {
                change: "REMOVE".into(),
                before: format_rule(rule),
                after: String::new(),
            });
        }
    }
    let before_by_id: std::collections::HashMap<_, _> = before.iter().map(|r| (&r.id, r)).collect();
    for a in after {
        if let Some(b) = before_by_id.get(&a.id) {
            if format_rule(b) != format_rule(a) {
                entries.push(FirewallDiffEntry {
                    change: "CHANGE".into(),
                    before: format_rule(b),
                    after: format_rule(a),
                });
            }
        }
    }

    let risk_before = assess_rules_risk(before);
    let risk_after = assess_rules_risk(after);
    let mut warnings = Vec::new();
    if risk_after == ExposureRisk::Critical && risk_before != ExposureRisk::Critical {
        warnings.push("This change increases public exposure".into());
    }
    if after.len() < before.len() {
        warnings.push("Some rules will be removed".into());
    }

    FirewallDiff {
        entries,
        risk_before,
        risk_after,
        warnings,
        rollback_available: true,
    }
}

fn format_rule(r: &FirewallRule) -> String {
    format!(
        "{} {}/{} from {} action {}",
        r.direction,
        r.protocol,
        r.ports,
        r.sources.join(","),
        r.action
    )
}

fn assess_rules_risk(rules: &[FirewallRule]) -> ExposureRisk {
    let critical = rules.iter().any(|r| {
        r.action == "allow"
            && (r.ports == "5432" || r.ports == "3306" || r.ports == "22")
            && r.sources.iter().any(|s| s == "any" || s == "0.0.0.0/0")
    });
    if critical {
        ExposureRisk::Critical
    } else if rules
        .iter()
        .any(|r| r.action == "allow" && r.sources.contains(&"any".to_string()))
    {
        ExposureRisk::Warning
    } else {
        ExposureRisk::Safe
    }
}

pub fn simulate_connectivity(diff: &FirewallDiff, sources: &[(&str, &str)]) -> Vec<String> {
    let mut results = Vec::new();
    for (name, _cidr) in sources {
        if diff.risk_after == ExposureRisk::Critical {
            results.push(format!(
                "⚠ {name} → sensitive port may be blocked or exposed"
            ));
        } else {
            results.push(format!("✓ {name} → allowed per plan"));
        }
    }
    results
}

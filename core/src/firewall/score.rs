// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::types::{
    ExposureRisk, FirewallPosture, FirewallRule, FirewallScore, OpenPort, ScoreBreakdownItem,
    ScoreRecommendation,
};

pub fn compute_firewall_score(
    posture: &FirewallPosture,
    rules: &[FirewallRule],
    ports: &[OpenPort],
) -> FirewallScore {
    let mut score: i32 = 100;
    let mut breakdown = Vec::new();
    let mut recommendations = Vec::new();

    if !posture.enabled {
        score -= 25;
        breakdown.push(ScoreBreakdownItem {
            category: "firewall_enabled".into(),
            status: "critical".into(),
            points: -25,
            detail: "Host firewall is disabled".into(),
        });
        recommendations.push(ScoreRecommendation {
            label: "Enable host firewall".into(),
            points: 25,
            action: "enable_firewall".into(),
        });
    } else {
        breakdown.push(ScoreBreakdownItem {
            category: "firewall_enabled".into(),
            status: "good".into(),
            points: 0,
            detail: "Firewall is active".into(),
        });
    }

    let default_deny = posture.default_inbound.as_deref() == Some("deny")
        || posture.default_inbound.as_deref() == Some("DROP");
    if posture.enabled && !default_deny {
        score -= 10;
        breakdown.push(ScoreBreakdownItem {
            category: "default_inbound".into(),
            status: "warning".into(),
            points: -10,
            detail: "Default inbound policy is not deny".into(),
        });
        recommendations.push(ScoreRecommendation {
            label: "Set default inbound to deny".into(),
            points: 10,
            action: "default_deny_inbound".into(),
        });
    }

    let ssh_public = rules.iter().any(|r| {
        r.action == "allow"
            && (r.ports == "22" || r.ports.contains("22"))
            && r.sources
                .iter()
                .any(|s| s == "any" || s == "0.0.0.0/0" || s == "Anywhere")
    }) || ports
        .iter()
        .any(|p| p.port == 22 && p.risk == ExposureRisk::Critical);
    if ssh_public {
        score -= 8;
        breakdown.push(ScoreBreakdownItem {
            category: "ssh_exposure".into(),
            status: "warning".into(),
            points: -8,
            detail: "SSH is allowed from anywhere".into(),
        });
        recommendations.push(ScoreRecommendation {
            label: "Restrict SSH to admin subnet".into(),
            points: 8,
            action: "restrict_ssh".into(),
        });
    }

    let db_public = ports
        .iter()
        .any(|p| matches!(p.port, 3306 | 5432 | 6379 | 27017) && p.risk == ExposureRisk::Critical);
    if db_public {
        score -= 20;
        breakdown.push(ScoreBreakdownItem {
            category: "database_exposure".into(),
            status: "critical".into(),
            points: -20,
            detail: "Database port exposed publicly".into(),
        });
        recommendations.push(ScoreRecommendation {
            label: "Restrict database to app servers only".into(),
            points: 20,
            action: "restrict_database".into(),
        });
    }

    let critical_ports = ports
        .iter()
        .filter(|p| p.risk == ExposureRisk::Critical)
        .count();
    if critical_ports > 0 && !db_public {
        score -= (critical_ports as i32) * 5;
        breakdown.push(ScoreBreakdownItem {
            category: "open_ports".into(),
            status: "warning".into(),
            points: -((critical_ports as i32) * 5),
            detail: format!("{critical_ports} critical port exposures"),
        });
    } else if critical_ports == 0 {
        breakdown.push(ScoreBreakdownItem {
            category: "open_ports".into(),
            status: "good".into(),
            points: 0,
            detail: "No critical public port exposures".into(),
        });
    }

    if posture.drift_detected {
        score -= 15;
        breakdown.push(ScoreBreakdownItem {
            category: "drift".into(),
            status: "warning".into(),
            points: -15,
            detail: "Firewall drift detected outside Zeus".into(),
        });
    }

    if posture.stealth_level == super::types::StealthLevel::Off && posture.enabled {
        recommendations.push(ScoreRecommendation {
            label: "Enable Stealth Mode".into(),
            points: 5,
            action: "enable_stealth".into(),
        });
    }

    FirewallScore {
        score: score.clamp(0, 100) as u32,
        breakdown,
        recommendations,
    }
}

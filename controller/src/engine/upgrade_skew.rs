// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Version-skew rules between the controller and the agents it drives.
//!
//! Policy: **the controller is upgraded first, and an agent may trail it by at most one minor version** (N-1). Anything
//! further behind is `unsupported`; an agent newer than its controller means the controller was upgraded out of order.
//! Before 1.0 the minor number is the breaking level, so `0.2.x` and `0.1.x` are N-1 of each other. Patch releases never
//! matter. An agent that reports no version (too old to self-report, or not yet synced) is `unknown`, not silently fine.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// Parse "1.2.3", "v1.2.3" or "1.2.3-rc1" (pre-release suffix ignored). None for anything else.
pub fn parse(v: &str) -> Option<Version> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().map(|p| p.parse().ok()).unwrap_or(Some(0))?;
    if it.next().is_some() {
        return None;
    }
    Some(Version {
        major,
        minor,
        patch,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Same major.minor as the controller.
    Current,
    /// One minor behind: supported, upgrade when convenient.
    Supported,
    /// Two or more minors (or a major) behind: upgrade before relying on it.
    Unsupported,
    /// Newer than the controller: the controller must be upgraded first.
    ControllerBehind,
    /// The agent reports no usable version.
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Skew {
    pub status: Status,
    pub reason: String,
}

pub fn classify(controller: &str, agent: &str) -> Skew {
    let (Some(c), Some(a)) = (parse(controller), parse(agent)) else {
        return Skew {
            status: Status::Unknown,
            reason: if agent.trim().is_empty() {
                "The agent has not reported a version yet.".into()
            } else {
                format!("Could not read the agent version '{agent}'.")
            },
        };
    };
    if (a.major, a.minor) == (c.major, c.minor) {
        return Skew {
            status: Status::Current,
            reason: "Matches the controller.".into(),
        };
    }
    if (a.major, a.minor) > (c.major, c.minor) {
        return Skew {
            status: Status::ControllerBehind,
            reason: format!("The agent ({agent}) is newer than the controller ({controller}); upgrade the controller first."),
        };
    }
    if a.major == c.major && c.minor - a.minor == 1 {
        return Skew {
            status: Status::Supported,
            reason: format!("One minor version behind ({agent} vs {controller}); supported."),
        };
    }
    Skew {
        status: Status::Unsupported,
        reason: format!("Too far behind the controller ({agent} vs {controller}); only one minor version of skew is supported."),
    }
}

/// Why an agent upgrade request must be refused right now, if it must. `None` = safe to proceed.
pub fn upgrade_blocker(
    controller: &str,
    target: &str,
    maintenance: bool,
    running_vms: i64,
) -> Option<String> {
    match (parse(controller), parse(target)) {
        (Some(c), Some(t)) if (t.major, t.minor) > (c.major, c.minor) => {
            return Some(format!("Upgrade the controller to {target} first: agents must never be newer than it (controller is {controller})."));
        }
        (_, None) => return Some(format!("'{target}' is not a version (use e.g. 0.2.0).")),
        _ => {}
    }
    if !maintenance && running_vms > 0 {
        return Some(format!(
            "This host runs {running_vms} machine(s). Put it into maintenance first so they are moved off safely, then upgrade."
        ));
    }
    None
}

/// Upgrade order: the controller first, then hosts that are safest to touch first (no machines, then fewest).
pub fn order<'a>(hosts: &'a [(String, i64)]) -> Vec<&'a str> {
    let mut v: Vec<&(String, i64)> = hosts.iter().collect();
    v.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().map(|h| h.0.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions_leniently() {
        assert_eq!(
            parse("1.2.3"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(
            parse("v0.2.0-rc1"),
            Some(Version {
                major: 0,
                minor: 2,
                patch: 0
            })
        );
        assert_eq!(
            parse("1.2"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 0
            })
        );
        assert_eq!(parse(""), None);
        assert_eq!(parse("abc"), None);
        assert_eq!(parse("1.2.3.4"), None);
    }

    #[test]
    fn same_minor_is_current_whatever_the_patch() {
        assert_eq!(classify("0.2.5", "0.2.0").status, Status::Current);
        assert_eq!(classify("1.4.0", "1.4.9").status, Status::Current);
    }

    #[test]
    fn one_minor_behind_is_supported_two_is_not() {
        assert_eq!(classify("0.2.0", "0.1.9").status, Status::Supported);
        assert_eq!(classify("1.4.0", "1.3.0").status, Status::Supported);
        assert_eq!(classify("1.4.0", "1.2.0").status, Status::Unsupported);
        assert_eq!(classify("2.0.0", "1.9.0").status, Status::Unsupported);
    }

    #[test]
    fn an_agent_ahead_of_the_controller_means_wrong_upgrade_order() {
        let s = classify("0.1.0", "0.2.0");
        assert_eq!(s.status, Status::ControllerBehind);
        assert!(s.reason.contains("controller first"));
    }

    #[test]
    fn a_missing_or_garbled_version_is_unknown_not_ok() {
        assert_eq!(classify("0.1.0", "").status, Status::Unknown);
        assert_eq!(classify("0.1.0", "banana").status, Status::Unknown);
    }

    #[test]
    fn upgrades_are_refused_when_unsafe() {
        // agent target newer than the controller
        assert!(upgrade_blocker("0.1.0", "0.2.0", true, 0)
            .unwrap()
            .contains("controller"));
        // machines running and not in maintenance
        assert!(upgrade_blocker("0.2.0", "0.2.0", false, 3)
            .unwrap()
            .contains("maintenance"));
        // fine: in maintenance, or empty
        assert_eq!(upgrade_blocker("0.2.0", "0.2.0", true, 3), None);
        assert_eq!(upgrade_blocker("0.2.0", "0.1.0", false, 0), None);
        // nonsense target
        assert!(upgrade_blocker("0.2.0", "latest", true, 0).is_some());
    }

    #[test]
    fn emptiest_hosts_go_first() {
        let hosts = vec![
            ("b".to_string(), 5),
            ("a".to_string(), 0),
            ("c".to_string(), 5),
        ];
        assert_eq!(order(&hosts), vec!["a", "b", "c"]);
    }
}

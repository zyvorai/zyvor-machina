// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Simple VDI desktop pools: the rules behind `api/desktop_pools.rs`. A pool clones a stopped
//! template VM into numbered desktops (`<prefix>-01`, ...), hands one to each user who checks out,
//! and, for a non-persistent pool, deletes and re-clones it when the user releases it. These
//! functions decide names, scaling, who gets which desktop and how a member's state advances; the
//! API performs the clone, power and delete tasks and the database writes.

use uuid::Uuid;

pub const MAX_POOL_SIZE: i64 = 100;

/// What the world looks like for one member's VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmView {
    /// `vms.observed_state` (`creating` while a clone is still being made).
    pub observed_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub id: Uuid,
    pub vm_name: String,
    pub assigned_user: String,
    pub state: String,
}

/// `<prefix>-01` .. `<prefix>-NN`, numbers padded to two digits (three above 99).
pub fn member_names(prefix: &str, size: i64) -> Vec<String> {
    let width = if size > 99 { 3 } else { 2 };
    (1..=size)
        .map(|n| format!("{prefix}-{n:0width$}"))
        .collect()
}

/// A prefix becomes part of a VM name: lowercase letters, digits and '-', 1 to 40 characters.
pub fn validate_prefix(p: &str) -> Result<(), String> {
    let ok = !p.is_empty()
        && p.len() <= 40
        && !p.starts_with('-')
        && !p.ends_with('-')
        && p.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err("prefix must be 1-40 lowercase letters, digits or '-'".into())
    }
}

pub fn validate_user(u: &str) -> Result<(), String> {
    let u = u.trim();
    if u.is_empty() || u.len() > 128 || u.chars().any(char::is_control) {
        return Err("user must be 1-128 printable characters".into());
    }
    Ok(())
}

pub fn validate_size(size: i64) -> Result<(), String> {
    if (1..=MAX_POOL_SIZE).contains(&size) {
        Ok(())
    } else {
        Err(format!("size must be 1 to {MAX_POOL_SIZE}"))
    }
}

pub fn validate_clone_mode(m: &str) -> Result<(), String> {
    if matches!(m, "linked" | "full") {
        Ok(())
    } else {
        Err("clone_mode must be linked or full".into())
    }
}

/// Template states a clone is safe from.
pub fn template_ok(observed_state: &str) -> Result<(), String> {
    match observed_state {
        "running" | "paused" | "blocked" | "creating" | "deleting" => Err(format!(
            "the template VM is {observed_state}; stop it first so clones start from a clean disk"
        )),
        _ => Ok(()),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScalePlan {
    /// Member names to add.
    pub create: Vec<String>,
    /// Members to remove (highest number first), all unassigned.
    pub remove: Vec<Uuid>,
    /// How many members could not be removed because users still hold them.
    pub blocked: usize,
}

/// Grow to `size` by adding missing names; shrink by removing unassigned members from the end.
pub fn plan_scale(existing: &[Member], prefix: &str, size: i64) -> ScalePlan {
    let wanted = member_names(prefix, size);
    let have: std::collections::HashSet<&str> =
        existing.iter().map(|m| m.vm_name.as_str()).collect();
    let create: Vec<String> = wanted
        .iter()
        .filter(|n| !have.contains(n.as_str()))
        .cloned()
        .collect();
    let keep: std::collections::HashSet<&str> = wanted.iter().map(String::as_str).collect();
    let mut surplus: Vec<&Member> = existing
        .iter()
        .filter(|m| !keep.contains(m.vm_name.as_str()))
        .collect();
    surplus.sort_by(|a, b| b.vm_name.cmp(&a.vm_name));
    let mut remove = Vec::new();
    let mut blocked = 0;
    for m in surplus {
        if m.assigned_user.is_empty() {
            remove.push(m.id);
        } else {
            blocked += 1;
        }
    }
    ScalePlan {
        create,
        remove,
        blocked,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Pick {
    /// The user already holds this desktop.
    Existing(Uuid),
    /// A free, ready desktop to assign.
    Free(Uuid),
    /// Nothing is free right now.
    None,
}

/// Choose a desktop for `user`: the one they hold, else the first ready, unassigned one by name.
pub fn pick_for_user(members: &[Member], user: &str) -> Pick {
    if let Some(m) = members.iter().find(|m| m.assigned_user == user) {
        return Pick::Existing(m.id);
    }
    let mut free: Vec<&Member> = members
        .iter()
        .filter(|m| m.state == "ready" && m.assigned_user.is_empty())
        .collect();
    free.sort_by(|a, b| a.vm_name.cmp(&b.vm_name));
    free.first().map_or(Pick::None, |m| Pick::Free(m.id))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    /// Start a clone from the template.
    EnqueueClone,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    pub state: &'static str,
    pub action: Action,
    /// The VM vanished under an assigned user: drop the assignment.
    pub clear_assignment: bool,
}

fn step(state: &'static str, action: Action, clear_assignment: bool) -> Step {
    Step {
        state,
        action,
        clear_assignment,
    }
}

/// Advance one member given what exists. Called by reconcile, checkout and release.
pub fn next_step(state: &str, assigned: bool, vm: Option<&VmView>) -> Step {
    let steady = if assigned { "assigned" } else { "ready" };
    match (state, vm) {
        // A clone in flight: wait for the row to leave `creating`.
        ("cloning", Some(v)) if v.observed_state != "creating" => step(steady, Action::None, false),
        ("cloning", _) => step("cloning", Action::None, false),
        // Waiting for a delete to finish before the name is free again.
        ("recycling", Some(_)) => step("recycling", Action::None, false),
        ("recycling", None) => step("needs_clone", Action::EnqueueClone, false),
        ("needs_clone", Some(_)) => step(steady, Action::None, false),
        ("needs_clone", None) => step("cloning", Action::EnqueueClone, false),
        ("error", Some(_)) => step(steady, Action::None, false),
        ("error", None) => step("needs_clone", Action::EnqueueClone, false),
        // ready / assigned: the VM disappeared (deleted outside the pool).
        (_, None) => step("needs_clone", Action::EnqueueClone, assigned),
        (_, Some(v)) if v.observed_state == "creating" => step("cloning", Action::None, false),
        (_, Some(_)) => step(steady, Action::None, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, user: &str, state: &str) -> Member {
        Member {
            id: Uuid::new_v4(),
            vm_name: name.into(),
            assigned_user: user.into(),
            state: state.into(),
        }
    }

    fn vm(state: &str) -> VmView {
        VmView {
            observed_state: state.into(),
        }
    }

    #[test]
    fn names_are_padded() {
        assert_eq!(member_names("vdi", 3), ["vdi-01", "vdi-02", "vdi-03"]);
        let big = member_names("vdi", 100);
        assert_eq!(big[0], "vdi-001");
        assert_eq!(big[99], "vdi-100");
    }

    #[test]
    fn validators() {
        assert!(validate_prefix("vdi-sales").is_ok());
        for bad in ["", "-x", "x-", "Vdi", "a b", "a_b"] {
            assert!(validate_prefix(bad).is_err(), "{bad}");
        }
        assert!(validate_user("alice").is_ok());
        assert!(validate_user("  ").is_err());
        assert!(validate_user("a\tb").is_err());
        assert!(validate_size(1).is_ok() && validate_size(100).is_ok());
        assert!(validate_size(0).is_err() && validate_size(101).is_err());
        assert!(validate_clone_mode("linked").is_ok() && validate_clone_mode("xml").is_err());
        assert!(template_ok("shutoff").is_ok());
        assert!(template_ok("running").is_err());
    }

    #[test]
    fn scale_up_adds_only_missing_names() {
        let existing = vec![member("vdi-01", "", "ready"), member("vdi-03", "", "ready")];
        let p = plan_scale(&existing, "vdi", 4);
        assert_eq!(p.create, ["vdi-02", "vdi-04"]);
        assert!(p.remove.is_empty());
    }

    #[test]
    fn scale_down_removes_unassigned_from_the_end_and_reports_blocked() {
        let existing = vec![
            member("vdi-01", "", "ready"),
            member("vdi-02", "", "ready"),
            member("vdi-03", "bob", "assigned"),
            member("vdi-04", "", "ready"),
        ];
        let p = plan_scale(&existing, "vdi", 2);
        assert!(p.create.is_empty());
        assert_eq!(p.remove, vec![existing[3].id]);
        assert_eq!(p.blocked, 1);
    }

    #[test]
    fn picks_existing_then_first_free_ready() {
        let mut ms = vec![
            member("vdi-02", "", "ready"),
            member("vdi-01", "", "cloning"),
            member("vdi-03", "carol", "assigned"),
            member("vdi-04", "", "ready"),
        ];
        assert_eq!(pick_for_user(&ms, "carol"), Pick::Existing(ms[2].id));
        // vdi-01 is still cloning, so vdi-02 is the first free one.
        assert_eq!(pick_for_user(&ms, "dave"), Pick::Free(ms[0].id));
        ms[0].state = "recycling".into();
        ms[3].state = "cloning".into();
        assert_eq!(pick_for_user(&ms, "dave"), Pick::None);
    }

    #[test]
    fn state_machine_clone_lifecycle() {
        // New member: clone it.
        assert_eq!(
            next_step("needs_clone", false, None),
            step("cloning", Action::EnqueueClone, false)
        );
        // Clone still being made.
        assert_eq!(
            next_step("cloning", false, Some(&vm("creating"))),
            step("cloning", Action::None, false)
        );
        // Clone finished.
        assert_eq!(
            next_step("cloning", false, Some(&vm("shutoff"))),
            step("ready", Action::None, false)
        );
        assert_eq!(
            next_step("cloning", true, Some(&vm("shutoff"))),
            step("assigned", Action::None, false)
        );
    }

    #[test]
    fn state_machine_recycle_waits_for_the_delete() {
        assert_eq!(
            next_step("recycling", false, Some(&vm("shutoff"))),
            step("recycling", Action::None, false)
        );
        assert_eq!(
            next_step("recycling", false, None),
            step("needs_clone", Action::EnqueueClone, false)
        );
    }

    #[test]
    fn a_vanished_vm_is_recloned_and_an_assigned_user_is_released() {
        assert_eq!(
            next_step("ready", false, None),
            step("needs_clone", Action::EnqueueClone, false)
        );
        assert_eq!(
            next_step("assigned", true, None),
            step("needs_clone", Action::EnqueueClone, true)
        );
        // An error member recovers once its VM exists, or is re-cloned if not.
        assert_eq!(
            next_step("error", false, Some(&vm("running"))),
            step("ready", Action::None, false)
        );
        assert_eq!(
            next_step("error", false, None),
            step("needs_clone", Action::EnqueueClone, false)
        );
    }

    #[test]
    fn steady_members_stay_put() {
        assert_eq!(
            next_step("ready", false, Some(&vm("shutoff"))),
            step("ready", Action::None, false)
        );
        assert_eq!(
            next_step("assigned", true, Some(&vm("running"))),
            step("assigned", Action::None, false)
        );
    }
}

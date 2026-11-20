// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Controller fencing on the agent. Every controller call carries `x-machina-epoch` (the leadership epoch). The
//! agent remembers the highest epoch it has seen and refuses anything lower, so a controller that lost leadership
//! (or a restored-from-backup copy that never learned about the failover) cannot act on this host.

use std::sync::atomic::{AtomicI64, Ordering};

pub const HEADER: &str = "x-machina-epoch";

/// Highest controller epoch seen since this agent started.
static HIGHEST: AtomicI64 = AtomicI64::new(0);

/// Decide whether a request with `provided` (the parsed header, if any) may proceed, recording a higher epoch.
/// A missing header is allowed unless `required` (older controllers do not send one).
pub fn admit(seen: &AtomicI64, provided: Option<i64>, required: bool) -> Result<(), String> {
    match provided {
        None if required => Err("controller epoch missing (this agent requires one)".into()),
        None => Ok(()),
        Some(e) => {
            let prev = seen.fetch_max(e, Ordering::SeqCst);
            if e < prev {
                Err(format!(
                    "stale controller: epoch {e} is older than {prev} already seen on this host"
                ))
            } else {
                Ok(())
            }
        }
    }
}

/// Request-level check used by the gRPC interceptor.
pub fn check(req: &tonic::Request<()>, required: bool) -> Result<(), tonic::Status> {
    let provided = req
        .metadata()
        .get(HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<i64>().ok());
    admit(&HIGHEST, provided, required).map_err(tonic::Status::failed_precondition)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_or_equal_epochs_pass_and_older_are_refused() {
        let seen = AtomicI64::new(0);
        assert!(admit(&seen, Some(3), false).is_ok());
        assert!(admit(&seen, Some(3), false).is_ok());
        assert!(admit(&seen, Some(4), false).is_ok());
        let err = admit(&seen, Some(3), false).unwrap_err();
        assert!(err.contains("stale controller"), "{err}");
    }

    #[test]
    fn a_missing_header_is_allowed_unless_required() {
        let seen = AtomicI64::new(7);
        assert!(admit(&seen, None, false).is_ok());
        assert!(admit(&seen, None, true).is_err());
    }

    #[test]
    fn a_refused_stale_call_does_not_lower_the_floor() {
        let seen = AtomicI64::new(0);
        admit(&seen, Some(5), false).unwrap();
        let _ = admit(&seen, Some(2), false);
        assert_eq!(seen.load(Ordering::SeqCst), 5);
    }
}

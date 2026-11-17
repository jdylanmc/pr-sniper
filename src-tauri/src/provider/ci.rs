//! The head's CI verdict from its checks, one rule for every gate (#389).
//!
//! - Any failed check is red.
//! - A cancelled check run is **superseded** when a check run of the same name
//!   from the same app passed on the same head: a duplicate run, such as one
//!   cancelled by a concurrency group. It does not bear on CI. A legacy status
//!   (no app) never supersedes a check run.
//! - Any other cancelled check is not green and not red: it is pending a rerun,
//!   required or not. PR Sniper is read-only and never reruns it; a human (or the
//!   author's automation) does, and the next green read acts.
//! - Anything still running is pending.
//!
//! Required-ness is not an input. Only the action read sees active rulesets; the
//! poll snapshot sees classic protection alone, and both must agree. Provider
//! merge state (`CLEAN`, `HAS_HOOKS`) is not an input either: it can be clean
//! while a non-required check fails, which is still red. It stays a merge gate.
//! Azure DevOps reports one evaluation per policy and the latest status per
//! context, so it has no superseded duplicates and no cancelled conclusion.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    Passed,
    Pending,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiVerdict {
    /// No check reported.
    None,
    Green,
    Pending,
    Red,
}

/// One check as the CI rule reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckSignal<'a> {
    pub name: &'a str,
    /// The app that reported it (GitHub check suite app slug), when known.
    pub app: Option<&'a str>,
    pub outcome: CheckOutcome,
}

/// A cancelled check run that a passed same-name check run from the same app
/// replaced. `None` app (a legacy status) matches only `None`.
pub fn superseded(signal: &CheckSignal, all: &[CheckSignal]) -> bool {
    signal.outcome == CheckOutcome::Cancelled
        && all.iter().any(|other| {
            other.outcome == CheckOutcome::Passed
                && other.name == signal.name
                && other.app == signal.app
        })
}

pub fn verdict(all: &[CheckSignal]) -> CiVerdict {
    if all.is_empty() {
        CiVerdict::None
    } else if all.iter().any(|s| s.outcome == CheckOutcome::Failed) {
        CiVerdict::Red
    } else if all.iter().any(|s| {
        s.outcome == CheckOutcome::Pending
            || s.outcome == CheckOutcome::Cancelled && !superseded(s, all)
    }) {
        CiVerdict::Pending
    } else {
        CiVerdict::Green
    }
}

/// Cancelled checks no passed run superseded: CI waits on their rerun.
pub fn needs_rerun<'a>(all: &[CheckSignal<'a>]) -> Vec<&'a str> {
    all.iter()
        .filter(|s| s.outcome == CheckOutcome::Cancelled && !superseded(s, all))
        .map(|s| s.name)
        .collect()
}

/// A provider check context (GraphQL `CheckRun` or `StatusContext` shape) as a signal.
pub fn context_signal(context: &Value) -> CheckSignal<'_> {
    match context["__typename"].as_str() {
        Some("StatusContext") => CheckSignal {
            name: context["context"].as_str().unwrap_or(""),
            app: None,
            outcome: match context["state"].as_str() {
                Some("SUCCESS") => CheckOutcome::Passed,
                Some("FAILURE" | "ERROR") => CheckOutcome::Failed,
                _ => CheckOutcome::Pending,
            },
        },
        _ => CheckSignal {
            name: context["name"].as_str().unwrap_or(""),
            // A check run always has an app identity, never the status `None`.
            app: Some(context["checkSuite"]["app"]["slug"].as_str().unwrap_or("")),
            outcome: if context["status"] != "COMPLETED" {
                CheckOutcome::Pending
            } else {
                match context["conclusion"].as_str() {
                    Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => CheckOutcome::Passed,
                    Some("CANCELLED") => CheckOutcome::Cancelled,
                    Some(
                        "FAILURE" | "ERROR" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE",
                    ) => CheckOutcome::Failed,
                    _ => CheckOutcome::Pending,
                }
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CheckOutcome::*;

    fn s(name: &str, outcome: CheckOutcome) -> CheckSignal<'_> {
        CheckSignal {
            name,
            app: Some("github-actions"),
            outcome,
        }
    }

    #[test]
    fn a_cancelled_run_superseded_by_a_passed_sibling_is_green() {
        let all = [
            s("verify", Passed),
            s("verify", Cancelled),
            s("tag", Passed),
        ];
        assert_eq!(verdict(&all), CiVerdict::Green);
        assert!(needs_rerun(&all).is_empty());
    }

    #[test]
    fn only_a_same_app_check_run_supersedes() {
        let mut other = s("verify", Passed);
        other.app = Some("another-ci");
        let all = [other, s("verify", Cancelled)];
        assert_eq!(verdict(&all), CiVerdict::Pending);
        // A legacy status (no app) never supersedes a check run.
        let status = CheckSignal {
            name: "verify",
            app: None,
            outcome: Passed,
        };
        let all = [status, s("verify", Cancelled)];
        assert_eq!(verdict(&all), CiVerdict::Pending);
        assert_eq!(needs_rerun(&all), ["verify"]);
    }

    #[test]
    fn an_unsuperseded_cancel_waits_for_a_rerun_and_is_never_red() {
        let all = [s("build", Passed), s("verify", Cancelled)];
        assert_eq!(verdict(&all), CiVerdict::Pending);
        assert_eq!(needs_rerun(&all), ["verify"]);
    }

    #[test]
    fn a_real_failure_stays_red_beside_a_passed_sibling() {
        let all = [s("verify", Passed), s("verify", Failed)];
        assert_eq!(verdict(&all), CiVerdict::Red);
    }

    #[test]
    fn the_snapshot_drops_a_superseded_cancel_and_holds_any_other_as_pending() {
        let check =
            |name: &str, app: Option<&str>, conclusion: &str| crate::provider::SnapshotCheck {
                name: name.into(),
                status: "COMPLETED".into(),
                conclusion: conclusion.into(),
                app: app.map(str::to_owned),
            };
        let actions = Some("github-actions");
        let mut pr = crate::provider::PrSnapshot {
            required_checks: vec!["build".into()],
            checks: vec![
                check("build", actions, "SUCCESS"),
                check("verify", actions, "CANCELLED"),
                check("verify", actions, "SUCCESS"),
            ],
            ..Default::default()
        };
        let green = pr.normalize();
        assert!(green.ci_green());
        assert_eq!(green.checks.len(), 2);
        // A same-name legacy status does not supersede the cancelled run.
        pr.checks[2].app = None;
        let held = pr.normalize();
        assert!(held.ci_pending() && !held.ci_failed() && !held.ci_green());
        // Not required, cancelled, unsuperseded: still pending, as the action read.
        pr.checks.truncate(1);
        pr.checks.push(check("lint", actions, "CANCELLED"));
        assert!(pr.normalize().ci_pending());
        pr.checks.push(check("lint", actions, "FAILURE"));
        assert!(pr.normalize().ci_failed());
    }
}

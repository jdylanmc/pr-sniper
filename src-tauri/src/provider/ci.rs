//! The head's CI verdict from its checks, one rule for every gate (#389).
//!
//! - Any failed check is red.
//! - A cancelled check run is **excused** when a check of the same name (and same
//!   app, when known) passed on the same head: a superseded duplicate run, such
//!   as one cancelled by a concurrency group. It is also excused when branch
//!   protection names its required checks and this one is not among them.
//! - An unexcused cancelled check is not green and not red: it is pending a
//!   rerun. PR Sniper is read-only and never reruns it; a human (or the author's
//!   automation) does, and the next green read acts.
//! - Anything still running is pending.
//!
//! Provider merge state (`CLEAN`, `HAS_HOOKS`) is not an input: it can be clean
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

/// A cancelled check that does not bear on CI: superseded by a passed sibling, or
/// not required while the required checks are known (`required` non-empty).
pub fn excused(signal: &CheckSignal, all: &[CheckSignal], required: &[String]) -> bool {
    signal.outcome == CheckOutcome::Cancelled
        && (all.iter().any(|other| {
            other.outcome == CheckOutcome::Passed
                && other.name == signal.name
                && (other.app.is_none() || signal.app.is_none() || other.app == signal.app)
        }) || (!required.is_empty() && !required.iter().any(|r| r == signal.name)))
}

pub fn verdict(all: &[CheckSignal], required: &[String]) -> CiVerdict {
    if all.is_empty() {
        CiVerdict::None
    } else if all.iter().any(|s| s.outcome == CheckOutcome::Failed) {
        CiVerdict::Red
    } else if all.iter().any(|s| {
        s.outcome == CheckOutcome::Pending
            || s.outcome == CheckOutcome::Cancelled && !excused(s, all, required)
    }) {
        CiVerdict::Pending
    } else {
        CiVerdict::Green
    }
}

/// Unexcused cancelled checks: CI waits on their rerun.
pub fn needs_rerun<'a>(all: &[CheckSignal<'a>], required: &[String]) -> Vec<&'a str> {
    all.iter()
        .filter(|s| s.outcome == CheckOutcome::Cancelled && !excused(s, all, required))
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
            app: context["checkSuite"]["app"]["slug"].as_str(),
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

/// Required check names from a GitHub branch protection rule (empty when unknown).
pub fn protection_required(protection: &Value) -> Vec<String> {
    if protection["requiresStatusChecks"] != true {
        return Vec::new();
    }
    protection["requiredStatusCheckContexts"]
        .as_array()
        .map_or_else(Vec::new, |names| {
            names
                .iter()
                .filter_map(|n| n.as_str().map(str::to_owned))
                .collect()
        })
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
        assert_eq!(verdict(&all, &[]), CiVerdict::Green);
        assert!(needs_rerun(&all, &[]).is_empty());
    }

    #[test]
    fn a_sibling_from_another_app_does_not_supersede() {
        let mut other = s("verify", Passed);
        other.app = Some("another-ci");
        let all = [other, s("verify", Cancelled)];
        assert_eq!(verdict(&all, &[]), CiVerdict::Pending);
    }

    #[test]
    fn a_lone_cancelled_check_waits_for_a_rerun_and_is_never_red() {
        let all = [s("build", Passed), s("verify", Cancelled)];
        assert_eq!(verdict(&all, &["verify".into()]), CiVerdict::Pending);
        assert_eq!(verdict(&all, &[]), CiVerdict::Pending);
        assert_eq!(needs_rerun(&all, &[]), ["verify"]);
    }

    #[test]
    fn a_cancelled_check_not_required_is_excused_when_required_checks_are_known() {
        let all = [s("build", Passed), s("verify", Cancelled)];
        assert_eq!(verdict(&all, &["build".into()]), CiVerdict::Green);
    }

    #[test]
    fn snapshot_ci_excuses_a_superseded_cancel_and_holds_a_lone_one_as_pending() {
        let check = |name: &str, conclusion: &str| crate::provider::SnapshotCheck {
            name: name.into(),
            status: "COMPLETED".into(),
            conclusion: conclusion.into(),
        };
        let mut pr = crate::provider::PrSnapshot {
            required_checks: vec!["verify".into()],
            checks: vec![check("verify", "CANCELLED"), check("verify", "SUCCESS")],
            ..Default::default()
        };
        assert!(pr.normalize().ci_green());
        pr.checks.pop();
        let lone = pr.normalize();
        assert!(lone.ci_pending() && !lone.ci_failed() && !lone.ci_green());
        pr.checks.push(check("verify", "FAILURE"));
        assert!(pr.normalize().ci_failed());
    }

    #[test]
    fn a_real_failure_stays_red_beside_a_passed_sibling() {
        let all = [s("verify", Passed), s("verify", Failed)];
        assert_eq!(verdict(&all, &[]), CiVerdict::Red);
    }
}

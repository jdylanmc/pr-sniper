//! The head's CI verdict from its checks, one rule for every gate (#389).
//!
//! - Any failed check is red.
//! - A cancelled check run is **excused** when a check of the same name from the
//!   same app passed on the same head: a superseded duplicate run, such as one
//!   cancelled by a concurrency group. A legacy status (no app) never excuses a
//!   check run. It is also excused when every source of required checks was
//!   read (branch protection and active rulesets), they name some, and this one
//!   is not among them.
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

/// A cancelled check that does not bear on CI: superseded by a passed sibling from
/// the same app, or not required while the required checks are fully known
/// (`required` is `Some` and non-empty; `None` when any source was unread).
pub fn excused(signal: &CheckSignal, all: &[CheckSignal], required: Option<&[String]>) -> bool {
    signal.outcome == CheckOutcome::Cancelled
        && (all.iter().any(|other| {
            other.outcome == CheckOutcome::Passed
                && other.name == signal.name
                && other.app == signal.app
        }) || required
            .is_some_and(|r| !r.is_empty() && !r.iter().any(|name| name == signal.name)))
}

pub fn verdict(all: &[CheckSignal], required: Option<&[String]>) -> CiVerdict {
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
pub fn needs_rerun<'a>(all: &[CheckSignal<'a>], required: Option<&[String]>) -> Vec<&'a str> {
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

/// Every required check name on the base: the classic branch protection rule's
/// contexts plus each active ruleset's `required_status_checks`. `None` unless the
/// rulesets were read: a partial list never makes a check optional.
pub fn required_checks(protection: &Value, rules: Option<&[Value]>) -> Option<Vec<String>> {
    let rules = rules?;
    let mut required = Vec::new();
    if protection["requiresStatusChecks"] == true {
        required.extend(
            protection["requiredStatusCheckContexts"]
                .as_array()?
                .iter()
                .map(|n| n.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()?,
        );
    }
    for rule in rules
        .iter()
        .filter(|r| r["type"] == "required_status_checks")
    {
        for check in rule["parameters"]["required_status_checks"].as_array()? {
            required.push(check["context"].as_str()?.to_owned());
        }
    }
    Some(required)
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
        assert_eq!(verdict(&all, None), CiVerdict::Green);
        assert!(needs_rerun(&all, None).is_empty());
    }

    #[test]
    fn a_sibling_from_another_app_does_not_supersede() {
        let mut other = s("verify", Passed);
        other.app = Some("another-ci");
        let all = [other, s("verify", Cancelled)];
        assert_eq!(verdict(&all, None), CiVerdict::Pending);
        // A legacy status (no app) never excuses a check run.
        let status = CheckSignal {
            name: "verify",
            app: None,
            outcome: Passed,
        };
        let all = [status, s("verify", Cancelled)];
        let required = ["verify".to_string()];
        assert_eq!(verdict(&all, Some(&required)), CiVerdict::Pending);
        assert_eq!(needs_rerun(&all, Some(&required)), ["verify"]);
    }

    #[test]
    fn a_lone_cancelled_check_waits_for_a_rerun_and_is_never_red() {
        let all = [s("build", Passed), s("verify", Cancelled)];
        assert_eq!(verdict(&all, Some(&["verify".into()])), CiVerdict::Pending);
        assert_eq!(verdict(&all, None), CiVerdict::Pending);
        assert_eq!(needs_rerun(&all, None), ["verify"]);
    }

    #[test]
    fn a_cancelled_check_not_required_is_excused_when_required_checks_are_known() {
        let all = [s("build", Passed), s("verify", Cancelled)];
        assert_eq!(verdict(&all, Some(&["build".into()])), CiVerdict::Green);
        // Unknown (rulesets unread) or empty: never optional.
        assert_eq!(verdict(&all, None), CiVerdict::Pending);
        assert_eq!(verdict(&all, Some(&[])), CiVerdict::Pending);
    }

    #[test]
    fn required_checks_join_classic_protection_and_rulesets_or_stay_unknown() {
        let protection = serde_json::json!({"requiresStatusChecks": true,
            "requiredStatusCheckContexts": ["build"]});
        let rules = [serde_json::json!({"type": "required_status_checks",
            "parameters": {"required_status_checks": [{"context": "verify"}]}})];
        let required = required_checks(&protection, Some(&rules)).unwrap();
        assert_eq!(required, ["build", "verify"]);
        let all = [s("build", Passed), s("verify", Cancelled)];
        assert_eq!(verdict(&all, Some(&required)), CiVerdict::Pending);
        assert_eq!(required_checks(&protection, None), None);
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
        assert_eq!(verdict(&all, None), CiVerdict::Red);
    }
}

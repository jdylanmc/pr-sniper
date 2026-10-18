use pr_sniper_lib::github::provider::{
    Capabilities, CommentCapability, GithubClient, Response, Transport,
};
use pr_sniper_lib::github::{verify_identity, ConnectionError, Identity};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const USER: &str = "/user";
const REPOSITORY: &str = "/repos/jdylanmc/pr-sniper";
const PULLS: &str = "/repos/jdylanmc/pr-sniper/pulls?state=open&per_page=1";

mod action_observation {
    use super::*;
    use pr_sniper_lib::github::{
        actions::Action, provider::RemoteRepository, threads::QueryTransport,
    };

    struct ActionTransport(Value);

    impl Transport for ActionTransport {
        fn get(&self, path: &str) -> Result<Response, ConnectionError> {
            assert!(
                matches!(
                    path,
                    "/repos/example/repo/pulls/1/reviews?per_page=100&page=1"
                        | "/repos/example/repo/issues/1/comments?per_page=100&page=1"
                        | "/repos/example/repo/rules/branches/main"
                ),
                "unexpected GET path: {path}"
            );
            Ok(response(200, json!([])))
        }
    }

    impl QueryTransport for ActionTransport {
        fn query(&self, _: &str, _: Value) -> Result<Response, ConnectionError> {
            let checks = json!({"state": "SUCCESS", "contexts": {
                "totalCount": 1, "pageInfo": {"hasNextPage": false},
                "nodes": [{"__typename": "CheckRun", "name": "ci-gate",
                    "status": "COMPLETED", "conclusion": "SUCCESS"}]
            }});
            let pull = json!({
                "id": "PR_1", "fullDatabaseId": 1,
                "baseRefOid": "b".repeat(40), "baseRefName": "main",
                "headRefOid": "a".repeat(40), "isDraft": false,
                "state": "OPEN", "mergeStateStatus": "CLEAN",
                "mergeable": "MERGEABLE", "reviewDecision": null,
                "isInMergeQueue": false, "author": {"databaseId": 11},
                "headRepository": {"databaseId": 100},
                "mergedBy": null, "mergedAt": null, "mergeCommit": null,
                "baseRef": {"branchProtectionRule": self.0},
                "commits": {"nodes": [{"commit": {
                    "oid": "a".repeat(40), "statusCheckRollup": checks
                }}]},
                "reviewThreads": {"totalCount": 0,
                    "pageInfo": {"hasNextPage": false}, "nodes": []}
            });
            Ok(response(
                200,
                json!({"data": {
                    "viewer": {"databaseId": 22},
                    "repository": {
                        "databaseId": 100, "viewerPermission": "WRITE",
                        "viewerDefaultMergeMethod": "SQUASH", "squashMergeAllowed": true,
                        "pullRequest": pull
                    }
                }}),
            ))
        }
    }

    fn protection() -> Value {
        json!({
            "requiresApprovingReviews": false, "requiredApprovingReviewCount": null,
            "requiresCodeOwnerReviews": false, "requiresConversationResolution": false,
            "requiresStatusChecks": true, "requiresStrictStatusChecks": false,
            "requiredStatusCheckContexts": ["ci-gate"]
        })
    }

    fn observe(
        protection: Value,
    ) -> Result<pr_sniper_lib::github::actions::Observation, ConnectionError> {
        GithubClient::new(ActionTransport(protection)).action_observation(
            &RemoteRepository {
                id: "100".into(),
                name: "example/repo".into(),
            },
            1,
        )
    }

    #[test]
    fn nullable_unused_approval_count_does_not_block_review_preparation() {
        let observation = observe(protection()).unwrap();
        assert_eq!(observation.checks.as_deref(), Some("SUCCESS"));
        assert_eq!(
            observation.protection["requiredApprovingReviewCount"],
            Value::Null
        );
    }

    #[test]
    fn required_approval_count_still_requires_complete_numeric_evidence() {
        for required in [false, true] {
            for count in [None, Some(Value::Null), Some(json!("1")), Some(json!(-1))] {
                if !required && count == Some(Value::Null) {
                    continue;
                }
                let mut rule = protection();
                rule["requiresApprovingReviews"] = json!(required);
                match count {
                    Some(ref count) => rule["requiredApprovingReviewCount"] = count.clone(),
                    None => {
                        rule.as_object_mut()
                            .unwrap()
                            .remove("requiredApprovingReviewCount");
                    }
                }
                assert_eq!(
                    observe(rule).unwrap_err(),
                    ConnectionError::IncompleteRead,
                    "required: {required}, count: {count:?}"
                );
            }
        }
    }

    #[test]
    fn required_approvals_still_gate_merge_after_observation() {
        let mut rule = protection();
        rule["requiresApprovingReviews"] = json!(true);
        rule["requiredApprovingReviewCount"] = json!(1);
        let mut observation = observe(rule).unwrap();
        observation.write_capability = true;
        assert!(observation.blocker(Action::Merge).is_some());
        observation.review_decision = Some("APPROVED".into());
        assert_eq!(observation.blocker(Action::Merge), None);
    }
}

fn response(status: u16, body: Value) -> Response {
    Response {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&body).unwrap(),
    }
}

fn repository_body() -> Value {
    json!({
        "id": 1376547672,
        "full_name": "jdylanmc/pr-sniper",
        "private": false,
        "archived": false,
        "disabled": false,
        "permissions": {"pull": true, "push": true}
    })
}

struct FixtureTransport {
    responses: BTreeMap<&'static str, Result<Response, ConnectionError>>,
}

impl Transport for FixtureTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        match self.responses.get(path) {
            Some(Ok(response)) => Ok(Response {
                status: response.status,
                headers: response.headers.clone(),
                body: response.body.clone(),
            }),
            Some(Err(error)) => Err(*error),
            None => panic!("unexpected GET path: {path}"),
        }
    }
}

fn ready_transport() -> FixtureTransport {
    let mut repository = response(200, repository_body());
    repository
        .headers
        .insert("x-oauth-scopes".into(), "repo".into());
    FixtureTransport {
        responses: BTreeMap::from([
            (
                USER,
                Ok(response(200, json!({"id": 6954990, "login": "jdylanmc"}))),
            ),
            (REPOSITORY, Ok(repository)),
            (PULLS, Ok(response(200, json!([])))),
        ]),
    }
}

#[test]
fn malformed_account_ids_never_become_verified_decimal_identities() {
    for id in [
        Value::Null,
        json!(0),
        json!(-1),
        json!(0.5),
        json!(6954990.0),
        json!(18446744073709551616.0),
        json!("6954990"),
        json!("MDQ6VXNlcjY5NTQ5OTA="),
        json!(true),
    ] {
        let result = verify_identity(&json!({"id": id, "login": "jdylanmc"}), None);

        assert_eq!(result, Err(ConnectionError::InvalidResponse), "id: {id}");
    }
}

#[test]
fn renamed_login_does_not_change_the_expected_stable_account() {
    let result = verify_identity(
        &json!({"id": 6954990, "login": "renamed-account"}),
        Some("6954990"),
    );

    assert_eq!(
        result,
        Ok(Identity {
            id: "6954990".into(),
            login: "renamed-account".into(),
        })
    );
}

#[test]
fn wrong_identity_stops_before_any_repository_access() {
    let transport = FixtureTransport {
        responses: BTreeMap::from([(
            USER,
            Ok(response(200, json!({"id": 42, "login": "jdylanmc"}))),
        )]),
    };

    let result = GithubClient::new(transport).connect("jdylanmc/pr-sniper", Some("6954990"));

    assert_eq!(result, Err(ConnectionError::WrongIdentity));
}

#[test]
fn unauthorized_account_is_signed_out_not_a_verified_connection() {
    let transport = FixtureTransport {
        responses: BTreeMap::from([(
            USER,
            Ok(response(401, json!({"message": "Bad credentials"}))),
        )]),
    };

    let result = GithubClient::new(transport).connect("jdylanmc/pr-sniper", None);

    assert_eq!(result, Err(ConnectionError::SignedOut));
}

#[test]
fn repository_or_pull_404_is_ambiguous_and_403_is_read_denial() {
    for path in [REPOSITORY, PULLS] {
        for status in [403, 404] {
            let mut transport = ready_transport();
            transport.responses.insert(
                path,
                Ok(response(
                    status,
                    json!({"message": "Resource inaccessible"}),
                )),
            );

            let result =
                GithubClient::new(transport).connect("jdylanmc/pr-sniper", Some("6954990"));

            assert_eq!(
                result,
                Err(if status == 404 {
                    ConnectionError::RepositoryUnavailable
                } else {
                    ConnectionError::MissingReadPermission
                }),
                "GET {path}, status {status}"
            );
        }
    }
}

#[test]
fn rate_limits_are_distinct_from_permission_failures() {
    for (status, header) in [
        (429, None),
        (403, Some(("x-ratelimit-remaining", "0"))),
        (403, Some(("retry-after", "60"))),
    ] {
        let mut transport = ready_transport();
        let mut limited = response(status, json!({"message": "Rate limited"}));
        if let Some((name, value)) = header {
            limited.headers.insert(name.into(), value.into());
        }
        transport.responses.insert(PULLS, Ok(limited));

        let result = GithubClient::new(transport).connect("jdylanmc/pr-sniper", Some("6954990"));

        let expected = match header {
            Some(("retry-after", "60")) => Err(ConnectionError::RateLimitedAfter(60)),
            _ => Err(ConnectionError::RateLimited),
        };
        assert_eq!(result, expected, "{header:?}");
    }
}

#[test]
fn provider_failure_is_not_reported_as_missing_permission() {
    let mut transport = ready_transport();
    transport.responses.insert(
        PULLS,
        Ok(response(500, json!({"message": "Provider unavailable"}))),
    );

    let result = GithubClient::new(transport).connect("jdylanmc/pr-sniper", Some("6954990"));

    assert_eq!(result, Err(ConnectionError::ProviderFailure));
}

#[test]
fn provider_server_retry_after_is_preserved() {
    let mut transport = ready_transport();
    let mut unavailable = response(503, json!({"message": "Provider unavailable"}));
    unavailable
        .headers
        .insert("retry-after".into(), "120".into());
    transport.responses.insert(PULLS, Ok(unavailable));

    let result = GithubClient::new(transport).connect("jdylanmc/pr-sniper", Some("6954990"));

    assert_eq!(result, Err(ConnectionError::ProviderFailureAfter(120)));
}

#[test]
fn explicit_provider_rejection_is_not_classified_as_a_retryable_server_failure() {
    let mut transport = ready_transport();
    transport.responses.insert(
        PULLS,
        Ok(response(422, json!({"message": "Validation failed"}))),
    );

    let result = GithubClient::new(transport).connect("jdylanmc/pr-sniper", Some("6954990"));

    assert_eq!(result, Err(ConnectionError::ProviderRejectedStatus(422)));
}

#[test]
fn transport_network_error_remains_explicit() {
    let transport = FixtureTransport {
        responses: BTreeMap::from([(USER, Err(ConnectionError::Network))]),
    };

    let result = GithubClient::new(transport).connect("jdylanmc/pr-sniper", None);

    assert_eq!(result, Err(ConnectionError::Network));
}

#[test]
fn repository_verification_rejects_a_missing_or_revoked_repo_scope() {
    for scopes in ["", "read:user"] {
        let mut transport = ready_transport();
        let repository = transport
            .responses
            .get_mut(REPOSITORY)
            .unwrap()
            .as_mut()
            .unwrap();
        repository
            .headers
            .insert("x-oauth-scopes".into(), scopes.into());

        assert_eq!(
            GithubClient::new(transport).connect("jdylanmc/pr-sniper", Some("6954990")),
            Err(ConnectionError::MissingScope)
        );
    }
}

#[test]
fn absent_scope_evidence_does_not_claim_scope_revocation_or_a_grant() {
    let mut transport = ready_transport();
    let repository = transport
        .responses
        .get_mut(REPOSITORY)
        .unwrap()
        .as_mut()
        .unwrap();
    repository.headers.remove("x-oauth-scopes");

    assert_eq!(
        GithubClient::new(transport).connect("jdylanmc/pr-sniper", Some("6954990")),
        Err(ConnectionError::ScopeUnverified)
    );
}

#[test]
fn archived_repository_remains_readable_without_comment_capability() {
    let mut transport = ready_transport();
    let mut body = repository_body();
    body["archived"] = json!(true);
    let repository = transport
        .responses
        .get_mut(REPOSITORY)
        .unwrap()
        .as_mut()
        .unwrap();
    repository.body = serde_json::to_vec(&body).unwrap();

    let connection = GithubClient::new(transport)
        .connect("jdylanmc/pr-sniper", Some("6954990"))
        .unwrap();

    assert_eq!(
        connection.capabilities,
        Capabilities {
            read: true,
            comment: CommentCapability::Unavailable,
        }
    );
}

#[test]
fn provider_response_bodies_never_escape_safe_serialized_errors() {
    for (status, expected, safe_code) in [
        (200, ConnectionError::InvalidResponse, "invalid_response"),
        (401, ConnectionError::SignedOut, "signed_out"),
        (
            403,
            ConnectionError::MissingReadPermission,
            "missing_read_permission",
        ),
        (
            404,
            ConnectionError::MissingReadPermission,
            "missing_read_permission",
        ),
        (429, ConnectionError::RateLimited, "rate_limited"),
        (500, ConnectionError::ProviderFailure, "provider_failure"),
    ] {
        let transport = FixtureTransport {
            responses: BTreeMap::from([(
                USER,
                Ok(response(
                    status,
                    json!({"message": "fixture-secret-do-not-copy"}),
                )),
            )]),
        };

        let error = GithubClient::new(transport)
            .connect("jdylanmc/pr-sniper", None)
            .unwrap_err();

        assert_eq!(error, expected);
        assert_eq!(serde_json::to_value(error).unwrap(), json!(safe_code));
        assert!(!format!("{error:?}").contains("fixture-secret-do-not-copy"));
    }
}

#[test]
fn secondary_rate_limit_without_retry_header_is_not_a_permission_error() {
    let mut limited = response(
        403,
        json!({
            "message": "You have exceeded a secondary rate limit. Please wait a few minutes before you try again."
        }),
    );
    limited
        .headers
        .insert("x-ratelimit-remaining".into(), "4999".into());
    let transport = FixtureTransport {
        responses: BTreeMap::from([(USER, Ok(limited))]),
    };
    let result = GithubClient::new(transport).connect("jdylanmc/pr-sniper", None);
    assert_eq!(result, Err(ConnectionError::RateLimited));
}

use pr_sniper_lib::github::{
    provider::{GithubClient, Response, Transport},
    ConnectionError,
};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap};

const ACCOUNT: &str = "120949562";
const REPO: &str = "/repos/gaming-microsoft/xgang-harness";
const PULLS: &str = "/repos/gaming-microsoft/xgang-harness/pulls?state=open&per_page=1";
const CATALOG: &str = "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1";

struct Provider {
    responses: RefCell<BTreeMap<&'static str, Result<Response, ConnectionError>>>,
    reads: RefCell<Vec<String>>,
}

fn response(status: u16, body: Value, headers: &[(&str, &str)]) -> Response {
    Response {
        status,
        body: serde_json::to_vec(&body).unwrap(),
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
    }
}

fn repository() -> Value {
    json!({
        "id": 100, "full_name": "gaming-microsoft/xgang-harness", "private": true,
        "archived": false, "disabled": false,
        "owner": {"login": "gaming-microsoft", "type": "Organization"},
        "description": null, "homepage": null, "language": null, "license": null
    })
}

impl Provider {
    fn ready() -> Self {
        Self {
            responses: RefCell::new(BTreeMap::from([
                (
                    "/user",
                    Ok(response(
                        200,
                        json!({"id": 120949562, "login": "fixture_corp"}),
                        &[],
                    )),
                ),
                (
                    REPO,
                    Ok(response(200, repository(), &[("x-oauth-scopes", "repo")])),
                ),
                (PULLS, Ok(response(200, json!([]), &[]))),
                (
                    CATALOG,
                    Ok(response(
                        200,
                        json!([repository()]),
                        &[("x-oauth-scopes", "repo")],
                    )),
                ),
            ])),
            reads: RefCell::new(Vec::new()),
        }
    }
}

impl Transport for &Provider {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.reads.borrow_mut().push(path.into());
        match self
            .responses
            .borrow()
            .get(path)
            .expect("unexpected provider endpoint")
        {
            Ok(r) => Ok(Response {
                status: r.status,
                headers: r.headers.clone(),
                body: r.body.clone(),
            }),
            Err(e) => Err(*e),
        }
    }
}

#[test]
fn corporate_url_variants_and_browser_resolve_the_same_private_identity() {
    for url in [
        "https://github.com/gaming-microsoft/xgang-harness/",
        "https://github.com/Gaming-Microsoft/Xgang-Harness.git",
        "  https://github.com/gaming-microsoft/xgang-harness.git/  ",
        "gaming-microsoft/xgang-harness",
    ] {
        for permissions in [None, Some(Value::Null), Some(json!({"pull": true}))] {
            let provider = Provider::ready();
            let mut metadata = repository();
            if let Some(permissions) = permissions {
                metadata["permissions"] = permissions;
            }
            provider.responses.borrow_mut().insert(
                REPO,
                Ok(response(200, metadata, &[("x-oauth-scopes", "repo")])),
            );
            let client = GithubClient::new(&provider);
            let connection = client.connect(url, Some(ACCOUNT)).unwrap();
            assert_eq!(connection.identity.id, ACCOUNT);
            assert!(connection.capabilities.read);
            let browser = client.repository_browser(&connection.identity).unwrap();
            assert_eq!(browser.repositories, vec![connection.repository]);
            assert!(browser.warnings.is_empty());
            assert!(provider.reads.borrow().iter().any(|path| path == PULLS));
        }
    }
}

#[test]
fn corporate_url_failure_classes_remain_truthful_and_retryable() {
    for path in [REPO, PULLS] {
        for (status, headers, code) in [
            (404, vec![], "repository_unavailable"),
            (
                403,
                vec![("x-oauth-scopes", "repo")],
                "missing_read_permission",
            ),
            (403, vec![("x-oauth-scopes", "read:user")], "missing_scope"),
            (
                403,
                vec![
                    ("x-oauth-scopes", "read:user"),
                    (
                        "x-github-sso",
                        "required; url=https://github.com/orgs/example/sso",
                    ),
                ],
                "organization_policy_denied_with_missing_scope",
            ),
            (
                403,
                vec![("x-oauth-scopes", "read:user"), ("retry-after", "60")],
                "rate_limited_after",
            ),
            (401, vec![], "signed_out"),
            (503, vec![], "provider_failure"),
            (200, vec![], "invalid_response"),
        ] {
            let provider = Provider::ready();
            provider.responses.borrow_mut().insert(
                path,
                Ok(response(
                    status,
                    json!({"message":"synthetic-private-error-do-not-publish"}),
                    &headers,
                )),
            );
            let client = GithubClient::new(&provider);
            let error = client
                .connect(
                    "https://github.com/gaming-microsoft/xgang-harness/",
                    Some(ACCOUNT),
                )
                .unwrap_err();
            let serialized = serde_json::to_value(error).unwrap();
            if code == "rate_limited_after" {
                assert_eq!(serialized, json!({"rate_limited_after":60}));
            } else {
                assert_eq!(serialized, json!(code), "{path}, {status}, {headers:?}");
            }
            assert!(!serialized.to_string().contains("synthetic-private-error"));
            let replacement = if path == REPO {
                repository()
            } else {
                json!([])
            };
            provider.responses.borrow_mut().insert(
                path,
                Ok(response(200, replacement, &[("x-oauth-scopes", "repo")])),
            );
            assert!(client
                .connect("gaming-microsoft/xgang-harness", Some(ACCOUNT))
                .is_ok());
        }
    }
}

#[test]
fn corporate_url_wrong_stable_identity_never_reads_repository() {
    let provider = Provider::ready();
    provider.responses.borrow_mut().insert(
        "/user",
        Ok(response(200, json!({"id":42,"login":"fixture_corp"}), &[])),
    );
    assert_eq!(
        GithubClient::new(&provider).connect("gaming-microsoft/xgang-harness", Some(ACCOUNT)),
        Err(ConnectionError::WrongIdentity)
    );
    assert_eq!(*provider.reads.borrow(), vec!["/user"]);
}

#[test]
fn explicit_denial_and_unverified_metadata_never_admit_a_repository() {
    for (field, value, expected) in [
        (
            "permissions",
            json!({"pull":false}),
            ConnectionError::MissingReadPermission,
        ),
        (
            "disabled",
            json!(true),
            ConnectionError::MissingReadPermission,
        ),
        ("id", json!(0), ConnectionError::InvalidResponse),
        (
            "full_name",
            json!("another/repository"),
            ConnectionError::RepositoryChanged,
        ),
        ("disabled", Value::Null, ConnectionError::InvalidResponse),
    ] {
        let provider = Provider::ready();
        let mut metadata = repository();
        metadata[field] = value;
        provider.responses.borrow_mut().insert(
            REPO,
            Ok(response(200, metadata, &[("x-oauth-scopes", "repo")])),
        );
        assert_eq!(
            GithubClient::new(&provider).connect("gaming-microsoft/xgang-harness", Some(ACCOUNT)),
            Err(expected)
        );
        assert!(!provider.reads.borrow().iter().any(|path| path == PULLS));
    }
}

#[test]
fn unknown_scope_evidence_blocks_url_and_catalog_without_claiming_revocation() {
    for path in [REPO, CATALOG] {
        let provider = Provider::ready();
        provider
            .responses
            .borrow_mut()
            .get_mut(path)
            .unwrap()
            .as_mut()
            .unwrap()
            .headers
            .clear();
        let client = GithubClient::new(&provider);
        let error = if path == REPO {
            client
                .connect("gaming-microsoft/xgang-harness", Some(ACCOUNT))
                .unwrap_err()
        } else {
            client
                .repository_browser(&client.current_identity().unwrap())
                .unwrap_err()
        };
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!("scope_unverified")
        );
        assert!(!provider.reads.borrow().iter().any(|p| p == PULLS));
    }
}

#[test]
fn rejected_http_status_is_not_a_successful_payload_schema_failure() {
    for path in [REPO, PULLS] {
        let provider = Provider::ready();
        provider.responses.borrow_mut().insert(
            path,
            Ok(response(422, json!({"message":"Validation failed"}), &[])),
        );
        let error = GithubClient::new(&provider)
            .connect("gaming-microsoft/xgang-harness", Some(ACCOUNT))
            .unwrap_err();
        assert_eq!(error, ConnectionError::ProviderRejectedStatus(422));
    }
}

const POLICY_MESSAGE: &str = "Although you appear to have the correct authorization credentials, the fixture-org organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited. For more information visit https://example.invalid/private";

#[test]
fn reset_only_rate_limits_and_policy_scope_combinations_retain_all_safe_evidence() {
    for path in [REPO, PULLS] {
        for (headers, expected) in [
            (
                vec![
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset", "1800000060"),
                ],
                ConnectionError::RateLimitedWithContext {
                    retry_after_seconds: None,
                    reset_at: Some(1800000060),
                    organization_access_incomplete: true,
                    missing_repo_scope: false,
                },
            ),
            (
                vec![("x-oauth-scopes", "read:user"), ("retry-after", "60")],
                ConnectionError::RateLimitedWithContext {
                    retry_after_seconds: Some(60),
                    reset_at: None,
                    organization_access_incomplete: true,
                    missing_repo_scope: true,
                },
            ),
        ] {
            let provider = Provider::ready();
            provider.responses.borrow_mut().insert(
                path,
                Ok(response(403, json!({"message":POLICY_MESSAGE}), &headers)),
            );
            assert_eq!(
                GithubClient::new(&provider)
                    .connect("gaming-microsoft/xgang-harness", Some(ACCOUNT)),
                Err(expected)
            );
        }
        for marker in [
            "partial-results; organizations=123",
            "unknown",
            "required-but-not-a-contract-marker",
        ] {
            let provider = Provider::ready();
            provider.responses.borrow_mut().insert(
                path,
                Ok(response(
                    403,
                    json!({"message":"Resource not accessible"}),
                    &[("x-github-sso", marker)],
                )),
            );
            assert_eq!(
                GithubClient::new(&provider)
                    .connect("gaming-microsoft/xgang-harness", Some(ACCOUNT)),
                Err(ConnectionError::MissingReadPermission)
            );
        }
        let provider = Provider::ready();
        provider.responses.borrow_mut().insert(
            path,
            Ok(response(
                403,
                json!({}),
                &[
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset", "1800000060"),
                ],
            )),
        );
        assert_eq!(
            GithubClient::new(&provider).connect("gaming-microsoft/xgang-harness", Some(ACCOUNT)),
            Err(ConnectionError::RateLimitedWithContext {
                retry_after_seconds: None,
                reset_at: Some(1800000060),
                organization_access_incomplete: false,
                missing_repo_scope: false,
            })
        );
    }
}

#[test]
fn evidenced_oauth_app_restrictions_preserve_policy_and_scope_without_raw_body() {
    for path in [REPO, PULLS] {
        for (headers, expected) in [
            (
                vec![("x-oauth-scopes", "repo")],
                "organization_policy_denied",
            ),
            (vec![], "organization_policy_denied"),
            (
                vec![("x-oauth-scopes", "read:user")],
                "organization_policy_denied_with_missing_scope",
            ),
        ] {
            let provider = Provider::ready();
            provider.responses.borrow_mut().insert(
                path,
                Ok(response(403, json!({"message":POLICY_MESSAGE}), &headers)),
            );
            let error = GithubClient::new(&provider)
                .connect("gaming-microsoft/xgang-harness", Some(ACCOUNT))
                .unwrap_err();
            assert_eq!(serde_json::to_value(error).unwrap(), json!(expected));
        }
        for message in [
            "Resource not accessible",
            "OAuth App access restrictions",
            "Check whether the organization has enabled OAuth App access restrictions",
        ] {
            let provider = Provider::ready();
            provider
                .responses
                .borrow_mut()
                .insert(path, Ok(response(403, json!({"message":message}), &[])));
            assert_eq!(
                GithubClient::new(&provider)
                    .connect("gaming-microsoft/xgang-harness", Some(ACCOUNT)),
                Err(ConnectionError::MissingReadPermission)
            );
        }
    }
}

#[test]
fn partial_sso_does_not_hide_confirmed_rate_limits_or_retry_timestamps() {
    for path in [REPO, PULLS] {
        let provider = Provider::ready();
        provider.responses.borrow_mut().insert(
            path,
            Ok(response(
                403,
                json!({"message":"API rate limit exceeded"}),
                &[
                    ("x-github-sso", "partial-results; organizations=123"),
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset", "1800000060"),
                    ("retry-after", "60"),
                ],
            )),
        );
        let error = GithubClient::new(&provider)
            .connect("gaming-microsoft/xgang-harness", Some(ACCOUNT))
            .unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "rate_limited_with_context":{
                    "retry_after_seconds":60,"reset_at":1800000060,
                    "organization_access_incomplete":true,"missing_repo_scope":false
                }
            })
        );
    }
}

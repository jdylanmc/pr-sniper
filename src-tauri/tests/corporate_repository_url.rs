use pr_sniper_lib::github::{
    provider::{GithubClient, Response, Transport},
    ConnectionError,
};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap};

const ACCOUNT: &str = "900000002";
const REPO: &str = "/repos/example-org/example-repo";
const PULLS: &str = "/repos/example-org/example-repo/pulls?state=open&per_page=1";
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
        "id": 100, "full_name": "example-org/example-repo", "private": true,
        "archived": false, "disabled": false,
        "owner": {"login": "example-org", "type": "Organization"},
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
                        json!({"id": 900000002, "login": "fixture_corp"}),
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
        "https://github.com/example-org/example-repo/",
        "https://github.com/example-org/example-repo.git",
        "  https://github.com/example-org/example-repo.git/  ",
        "example-org/example-repo",
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
                "organization_policy_denied",
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
                    "https://github.com/example-org/example-repo/",
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
                .connect("example-org/example-repo", Some(ACCOUNT))
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
        GithubClient::new(&provider).connect("example-org/example-repo", Some(ACCOUNT)),
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
            GithubClient::new(&provider).connect("example-org/example-repo", Some(ACCOUNT)),
            Err(expected)
        );
        assert!(!provider.reads.borrow().iter().any(|path| path == PULLS));
    }
}

use pr_sniper_lib::github::provider::{
    Capabilities, CommentCapability, GithubClient, Response, Transport,
};
use pr_sniper_lib::github::{verify_identity, ConnectionError, Identity};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const USER: &str = "/user";
const REPOSITORY: &str = "/repos/jdylanmc/pr-sniper";
const PULLS: &str = "/repos/jdylanmc/pr-sniper/pulls?state=open&per_page=1";

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
fn repository_or_pull_read_denial_is_missing_read_permission() {
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
                Err(ConnectionError::MissingReadPermission),
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

        assert_eq!(result, Err(ConnectionError::RateLimited), "{header:?}");
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
fn absent_scope_evidence_does_not_assume_the_required_repo_scope() {
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
        Err(ConnectionError::MissingScope)
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

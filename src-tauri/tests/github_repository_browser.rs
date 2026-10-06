use pr_sniper_lib::github::{
    provider::{GithubClient, Response, Transport},
    ConnectionError, Identity,
};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap};

const CATALOG: &str = "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=";

struct Fixture {
    responses: RefCell<Vec<(String, Result<Response, ConnectionError>)>>,
}

impl Fixture {
    fn new(responses: Vec<(String, Result<Response, ConnectionError>)>) -> Self {
        Self {
            responses: RefCell::new(responses),
        }
    }
}

impl Transport for Fixture {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let (expected, response) = self.responses.borrow_mut().remove(0);
        assert_eq!(path, expected);
        response
    }
}

fn identity() -> Identity {
    Identity {
        id: "22".into(),
        login: "fixture_corp".into(),
    }
}

fn repository(id: u64, owner: &str, kind: &str) -> Value {
    json!({
        "id": id, "full_name": format!("{owner}/repository-{id}"), "private": true,
        "owner": {"login": owner, "type": kind, "name": null, "extra": "ignored"},
        "description": null, "unconsumed_metadata": {"nullable": null}
    })
}

fn response(body: Value) -> Response {
    Response {
        status: 200,
        headers: BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]),
        body: serde_json::to_vec(&body).unwrap(),
    }
}

fn first(body: Value, link: Option<&str>) -> (String, Result<Response, ConnectionError>) {
    let mut response = response(body);
    if let Some(link) = link {
        response.headers.insert("link".into(), link.into());
    }
    (format!("{CATALOG}1"), Ok(response))
}

#[test]
fn corporate_personal_and_org_results_follow_short_pages_and_actual_links() {
    let link = format!("<https://api.github.com{CATALOG}2>; rel=\"next\", <https://api.github.com{CATALOG}2>; rel=\"last\"");
    let client = GithubClient::new(Fixture::new(vec![
        first(
            json!([repository(101, "Fixture_Corp", "User")]),
            Some(&link),
        ),
        (
            format!("{CATALOG}2"),
            Ok(response(json!([
                repository(102, "Orbit", "Organization"),
                repository(103, "collaborator", "User")
            ]))),
        ),
    ]));
    let browser = client.repository_browser(&identity()).unwrap();
    assert_eq!(
        browser
            .owners
            .iter()
            .map(|owner| owner.login.as_str())
            .collect::<Vec<_>>(),
        ["fixture_corp", "orbit"]
    );
    assert_eq!(browser.repositories.len(), 3);
    assert!(browser.warnings.is_empty());
}

#[test]
fn malformed_repository_records_are_visible_without_losing_good_results() {
    for broken in [
        json!({"id": 0}),
        json!({"id": 7, "full_name": "orbit/bad", "private": null, "owner": {"login": "orbit", "type": "Organization"}}),
        json!({"id": 7, "full_name": "orbit/bad", "private": true, "owner": null}),
        json!({"id": 7, "full_name": "orbit/bad", "private": true, "owner": {"login": "other", "type": "Organization"}}),
        repository(7, "orbit", "Unknown"),
    ] {
        let client = GithubClient::new(Fixture::new(vec![first(
            json!([
                repository(101, "fixture_corp", "User"),
                broken,
                repository(102, "orbit", "Organization")
            ]),
            None,
        )]));
        let browser = client.repository_browser(&identity()).unwrap();
        assert_eq!(browser.repositories.len(), 2);
        assert_eq!(browser.owners.len(), 2);
        assert_eq!(browser.warnings.len(), 1);
        assert_eq!(browser.warnings[0].error, ConnectionError::InvalidResponse);
        assert_eq!(browser.warnings[0].boundary, "repository_metadata");
        assert_eq!(browser.warnings[0].page, 1);
    }
}

#[test]
fn partial_sso_results_preserve_accessible_repositories_and_report_omission() {
    let mut partial = response(json!([repository(101, "orbit", "Organization")]));
    partial.headers.insert(
        "x-github-sso".into(),
        "partial-results; organizations=123,456".into(),
    );
    let client = GithubClient::new(Fixture::new(vec![(format!("{CATALOG}1"), Ok(partial))]));
    let browser = client
        .owner_repository_browser(&identity(), "orbit")
        .unwrap();
    assert_eq!(browser.repositories.len(), 1);
    assert_eq!(
        browser.warnings[0].error,
        ConnectionError::OrganizationPolicyDenied
    );
    assert_eq!(browser.warnings[0].boundary, "organization_access");
    let encoded = serde_json::to_string(&browser).unwrap();
    assert!(!encoded.contains("123,456"));
}

#[test]
fn failed_later_pages_keep_good_results_but_never_claim_completeness() {
    let link = format!("<https://api.github.com{CATALOG}2>; rel=\"next\"");
    let mut denied = response(json!({"message": "Denied"}));
    denied.status = 403;
    denied.headers.insert(
        "x-github-sso".into(),
        "required; url=https://github.com/orgs/private/sso".into(),
    );
    for failure in [
        Err(ConnectionError::Network),
        Err(ConnectionError::Timeout),
        Ok(denied),
        Ok(response(json!({"not": "a list"}))),
        Ok(Response {
            body: b"not-json".to_vec(),
            ..response(Value::Null)
        }),
        Ok(Response {
            status: 503,
            ..response(Value::Null)
        }),
    ] {
        let client = GithubClient::new(Fixture::new(vec![
            first(
                json!([repository(101, "orbit", "Organization")]),
                Some(&link),
            ),
            (format!("{CATALOG}2"), failure),
        ]));
        let browser = client.repository_browser(&identity()).unwrap();
        assert_eq!(browser.repositories.len(), 1);
        assert_eq!(browser.warnings.len(), 1);
        assert_eq!(browser.warnings[0].page, 2);
        assert_eq!(browser.warnings[0].boundary, "repository_page");
    }
}

#[test]
fn catalog_schema_transport_provider_and_authorization_failures_remain_distinct() {
    for (status, body, headers, expected) in [
        (
            200,
            b"not-json".to_vec(),
            BTreeMap::new(),
            ConnectionError::InvalidResponse,
        ),
        (
            200,
            b"[]".to_vec(),
            BTreeMap::from([("x-oauth-scopes".into(), "read:user".into())]),
            ConnectionError::MissingScope,
        ),
        (
            401,
            b"{}".to_vec(),
            BTreeMap::new(),
            ConnectionError::SignedOut,
        ),
        (
            403,
            b"{}".to_vec(),
            BTreeMap::new(),
            ConnectionError::MissingReadPermission,
        ),
        (
            403,
            b"{}".to_vec(),
            BTreeMap::from([("x-github-sso".into(), "required".into())]),
            ConnectionError::OrganizationPolicyDenied,
        ),
        (
            429,
            b"{}".to_vec(),
            BTreeMap::new(),
            ConnectionError::RateLimited,
        ),
        (
            503,
            b"{}".to_vec(),
            BTreeMap::new(),
            ConnectionError::ProviderFailure,
        ),
        (
            422,
            b"{}".to_vec(),
            BTreeMap::new(),
            ConnectionError::ProviderRejected,
        ),
    ] {
        let client = GithubClient::new(Fixture::new(vec![(
            format!("{CATALOG}1"),
            Ok(Response {
                status,
                body,
                headers,
            }),
        )]));
        assert_eq!(client.repository_browser(&identity()), Err(expected));
    }
    let client = GithubClient::new(Fixture::new(vec![(
        format!("{CATALOG}1"),
        Err(ConnectionError::Network),
    )]));
    assert_eq!(
        client.repository_browser(&identity()),
        Err(ConnectionError::Network)
    );
}

#[test]
fn signed_out_later_page_is_not_presented_as_usable_authorization() {
    let link = format!("<https://api.github.com{CATALOG}2>; rel=\"next\"");
    let client = GithubClient::new(Fixture::new(vec![
        first(
            json!([repository(101, "orbit", "Organization")]),
            Some(&link),
        ),
        (
            format!("{CATALOG}2"),
            Ok(Response {
                status: 401,
                ..response(Value::Null)
            }),
        ),
    ]));
    assert_eq!(
        client.repository_browser(&identity()),
        Err(ConnectionError::SignedOut)
    );
}

#[test]
fn unsafe_changed_or_disappearing_pagination_is_visible_and_retains_only_verified_records() {
    for link in [
        "<https://evil.example/user/repos?page=2>; rel=\"next\"".to_string(),
        format!("<https://api.github.com{CATALOG}3>; rel=\"next\""),
        "<https://api.github.com/user/repos?affiliation=owner&visibility=all&per_page=100&page=2>; rel=\"next\"".to_string(),
        format!("<https://api.github.com{CATALOG}2>; rel=\"last\""),
    ] {
        let client = GithubClient::new(Fixture::new(vec![first(
            json!([repository(101, "orbit", "Organization")]), Some(&link)
        )]));
        let browser = client.repository_browser(&identity()).unwrap();
        assert_eq!(browser.repositories.len(), 1);
        assert_eq!(browser.warnings[0].error, ConnectionError::IncompleteRead);
    }
    let link = format!("<https://api.github.com{CATALOG}2>; rel=\"next\", <https://api.github.com{CATALOG}3>; rel=\"last\"");
    let client = GithubClient::new(Fixture::new(vec![
        first(
            json!([repository(101, "orbit", "Organization")]),
            Some(&link),
        ),
        (
            format!("{CATALOG}2"),
            Ok(response(json!([repository(102, "orbit", "Organization")]))),
        ),
    ]));
    let browser = client.repository_browser(&identity()).unwrap();
    assert_eq!(browser.repositories.len(), 2);
    assert_eq!(browser.warnings[0].page, 2);
    assert_eq!(browser.warnings[0].error, ConnectionError::IncompleteRead);
}

#[test]
fn inaccessible_owner_is_empty_only_when_catalog_is_complete() {
    let client = GithubClient::new(Fixture::new(vec![first(
        json!([repository(101, "orbit", "Organization")]),
        None,
    )]));
    let browser = client
        .owner_repository_browser(&identity(), "other")
        .unwrap();
    assert!(browser.repositories.is_empty());
    assert!(browser.warnings.is_empty());
    let client = GithubClient::new(Fixture::new(vec![first(
        json!([repository(101, "orbit", "Organization")]),
        None,
    )]));
    assert_eq!(
        client.owner_repository_browser(&identity(), "../orbit"),
        Err(ConnectionError::InvalidRepository)
    );
}

#[test]
fn full_repository_pages_preserve_all_101_entries() {
    let link = format!("<https://api.github.com{CATALOG}2>; rel=\"next\"");
    let client = GithubClient::new(Fixture::new(vec![
        first(
            json!((1..=100)
                .map(|id| repository(id, "orbit", "Organization"))
                .collect::<Vec<_>>()),
            Some(&link),
        ),
        (
            format!("{CATALOG}2"),
            Ok(response(json!([repository(101, "orbit", "Organization")]))),
        ),
    ]));
    let browser = client
        .owner_repository_browser(&identity(), "orbit")
        .unwrap();
    assert_eq!(browser.repositories.len(), 101);
    assert_eq!(browser.repositories[100].id, "101");
    assert!(browser.warnings.is_empty());
}

#[test]
fn duplicate_or_empty_advertised_pages_are_not_silent_success() {
    let link = format!("<https://api.github.com{CATALOG}2>; rel=\"next\"");
    for second in [json!([]), json!([repository(101, "orbit", "Organization")])] {
        let client = GithubClient::new(Fixture::new(vec![
            first(
                json!([repository(101, "orbit", "Organization")]),
                Some(&link),
            ),
            (format!("{CATALOG}2"), Ok(response(second))),
        ]));
        let browser = client.repository_browser(&identity()).unwrap();
        assert_eq!(browser.repositories.len(), 1);
        assert_eq!(browser.warnings[0].error, ConnectionError::IncompleteRead);
    }
}

#[test]
fn contradictory_advertised_last_stops_before_following_the_next_page() {
    for link in [
            format!("<https://api.github.com{CATALOG}2>; rel=\"next\", <https://api.github.com{CATALOG}1>; rel=\"last\""),
            format!("<https://api.github.com{CATALOG}1>; rel=\"last\", <https://api.github.com{CATALOG}2>; rel=\"next\""),
        ] {
            let client = GithubClient::new(Fixture::new(vec![first(
                json!([repository(101, "fixture_corp", "User")]), Some(&link)
            )]));
            let browser = client.repository_browser(&identity()).unwrap();
            assert_eq!(browser.repositories.len(), 1);
            assert_eq!(browser.owners.len(), 1);
            assert_eq!(browser.warnings[0].error, ConnectionError::IncompleteRead);
            assert_eq!(browser.warnings[0].boundary, "pagination");
            assert_eq!(browser.warnings[0].page, 1);
        }
}

#[test]
fn next_cannot_exceed_a_retained_advertised_last() {
    let link = format!("<https://api.github.com{CATALOG}2>; rel=\"next\", <https://api.github.com{CATALOG}2>; rel=\"last\"");
    let mut second = response(json!([repository(102, "orbit", "Organization")]));
    second.headers.insert(
        "link".into(),
        format!("<https://api.github.com{CATALOG}3>; rel=\"next\""),
    );
    let client = GithubClient::new(Fixture::new(vec![
        first(
            json!([repository(101, "fixture_corp", "User")]),
            Some(&link),
        ),
        (format!("{CATALOG}2"), Ok(second)),
    ]));
    let browser = client.repository_browser(&identity()).unwrap();
    assert_eq!(browser.repositories.len(), 2);
    assert_eq!(browser.warnings[0].error, ConnectionError::IncompleteRead);
    assert_eq!(browser.warnings[0].page, 2);
}

#[test]
fn full_known_final_page_without_link_stops_complete_without_another_request() {
    let link = format!("<https://api.github.com{CATALOG}2>; rel=\"next\", <https://api.github.com{CATALOG}2>; rel=\"last\"");
    for terminal_link in [
        None,
        Some(format!("<https://api.github.com{CATALOG}1>; rel=\"prev\", <https://api.github.com{CATALOG}1>; rel=\"first\"")),
    ] {
        let mut terminal = response(json!((101..=200).map(|id| repository(id, "orbit", "Organization")).collect::<Vec<_>>()));
        if let Some(link) = terminal_link {
            terminal.headers.insert("link".into(), link);
        }
        let client = GithubClient::new(Fixture::new(vec![
            first(json!((1..=100).map(|id| repository(id, "orbit", "Organization")).collect::<Vec<_>>()), Some(&link)),
            (format!("{CATALOG}2"), Ok(terminal)),
        ]));
        let browser = client.owner_repository_browser(&identity(), "orbit").unwrap();
        assert_eq!(browser.repositories.len(), 200);
        assert!(browser.warnings.is_empty());
    }
    let link = format!("<https://api.github.com{CATALOG}1>; rel=\"last\"");
    let client = GithubClient::new(Fixture::new(vec![first(
        json!((1..=100)
            .map(|id| repository(id, "orbit", "Organization"))
            .collect::<Vec<_>>()),
        Some(&link),
    )]));
    let browser = client
        .owner_repository_browser(&identity(), "orbit")
        .unwrap();
    assert_eq!(browser.repositories.len(), 100);
    assert!(browser.warnings.is_empty());
}

#[test]
fn full_pages_probe_only_when_the_last_page_is_unknown() {
    for count in [100, 200] {
        let mut pages = vec![first(
            json!((1..=100)
                .map(|id| repository(id, "orbit", "Organization"))
                .collect::<Vec<_>>()),
            None,
        )];
        if count == 200 {
            pages.push((
                format!("{CATALOG}2"),
                Ok(response(json!((101..=200)
                    .map(|id| repository(id, "orbit", "Organization"))
                    .collect::<Vec<_>>()))),
            ));
        }
        pages.push((
            format!("{CATALOG}{}", count / 100 + 1),
            Ok(response(json!([]))),
        ));
        let browser = GithubClient::new(Fixture::new(pages))
            .owner_repository_browser(&identity(), "orbit")
            .unwrap();
        assert_eq!(browser.repositories.len(), count);
        assert!(browser.warnings.is_empty());
    }
}

use pr_sniper_lib::github::provider::{
    Capabilities, CommentCapability, Connection, GithubClient, RemoteRepository, Response,
    Transport,
};
use pr_sniper_lib::github::{verify_identity, ConnectionError, Identity};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn verified_account_uses_decimal_id_not_graphql_node_id() {
    let response = json!({
        "id": 6954990,
        "login": "jdylanmc",
        "node_id": "MDQ6VXNlcjY5NTQ5OTA="
    });

    let result = verify_identity(&response, Some("6954990"));

    assert_eq!(
        result,
        Ok(Identity {
            id: "6954990".into(),
            login: "jdylanmc".into(),
        })
    );
}

struct ConnectionTransport;

impl Transport for ConnectionTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let body = match path {
            "/user" => json!({
                "id": 6954990,
                "login": "jdylanmc"
            }),
            "/repos/jdylanmc/pr-sniper" => json!({
                "id": 1376547672,
                "full_name": "jdylanmc/pr-sniper",
                "private": false,
                "archived": false,
                "disabled": false,
                "permissions": {"pull": true, "push": true}
            }),
            "/repos/jdylanmc/pr-sniper/pulls?state=open&per_page=1" => json!([]),
            "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1" => json!([
                {
                    "id": 1376547672,
                    "full_name": "jdylanmc/pr-sniper",
                    "private": true
                },
                {
                    "id": 200,
                    "full_name": "octo/public-repository",
                    "private": false
                },
                {
                    "id": 300,
                    "full_name": "example-org/private-repository",
                    "private": true
                }
            ]),
            _ => panic!("unexpected GET path: {path}"),
        };
        Ok(Response {
            status: 200,
            headers: BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]),
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn oauth_user_token_lists_owned_collaborator_and_organization_repositories() {
    let client = GithubClient::new(ConnectionTransport);

    assert_eq!(
        client.accessible_repositories(),
        Ok(vec![
            RemoteRepository {
                id: "1376547672".into(),
                name: "jdylanmc/pr-sniper".into(),
            },
            RemoteRepository {
                id: "200".into(),
                name: "octo/public-repository".into(),
            },
            RemoteRepository {
                id: "300".into(),
                name: "example-org/private-repository".into(),
            },
        ])
    );
}

struct PaginatedRepositories;

impl Transport for PaginatedRepositories {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let body = match path {
            "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1" => {
                json!((1..=100).map(|id| json!({
                    "id": id,
                    "full_name": format!("octo/repository-{id}"),
                    "private": id % 2 == 0
                })).collect::<Vec<_>>())
            }
            "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=2" => {
                json!([{
                    "id": 101,
                    "full_name": "octo/repository-101",
                    "private": true
                }])
            }
            _ => panic!("unexpected GET path: {path}"),
        };
        Ok(Response {
            status: 200,
            headers: BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]),
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn user_repository_discovery_exhausts_every_page() {
    let repositories = GithubClient::new(PaginatedRepositories)
        .accessible_repositories()
        .unwrap();

    assert_eq!(repositories.len(), 101);
    assert_eq!(repositories[100].id, "101");
}

struct MissingScope;

impl Transport for MissingScope {
    fn get(&self, _path: &str) -> Result<Response, ConnectionError> {
        Ok(Response {
            status: 200,
            headers: BTreeMap::from([("x-oauth-scopes".into(), "read:user".into())]),
            body: serde_json::to_vec(&json!([])).unwrap(),
        })
    }
}

#[test]
fn user_repository_discovery_rejects_a_missing_repo_scope() {
    assert_eq!(
        GithubClient::new(MissingScope).accessible_repositories(),
        Err(ConnectionError::MissingScope)
    );
}

struct OrganizationPolicyDenied;

impl Transport for OrganizationPolicyDenied {
    fn get(&self, _path: &str) -> Result<Response, ConnectionError> {
        Ok(Response {
            status: 403,
            headers: BTreeMap::from([(
                "x-github-sso".into(),
                "required; url=https://github.com/orgs/example/sso".into(),
            )]),
            body: serde_json::to_vec(&json!({
                "message": "Resource protected by organization SAML enforcement."
            }))
            .unwrap(),
        })
    }
}

#[test]
fn organization_sso_or_policy_denial_is_distinct() {
    assert_eq!(
        GithubClient::new(OrganizationPolicyDenied).accessible_repositories(),
        Err(ConnectionError::OrganizationPolicyDenied)
    );
}

#[test]
fn connection_verifies_identity_repository_and_effective_capabilities() {
    let client = GithubClient::new(ConnectionTransport);

    let result = client.connect("jdylanmc/pr-sniper", Some("6954990"));

    assert_eq!(
        result,
        Ok(Connection {
            identity: Identity {
                id: "6954990".into(),
                login: "jdylanmc".into(),
            },
            repository: RemoteRepository {
                id: "1376547672".into(),
                name: "jdylanmc/pr-sniper".into(),
            },
            capabilities: Capabilities {
                read: true,
                comment: CommentCapability::Available,
            },
        })
    );
}

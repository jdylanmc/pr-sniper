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

#[test]
fn explicit_pr_url_resolves_containing_repository() {
    let connection = GithubClient::new(ConnectionTransport)
        .connect(
            "https://github.com/jdylanmc/pr-sniper/pull/302",
            Some("6954990"),
        )
        .unwrap();
    assert_eq!(connection.repository.name, "jdylanmc/pr-sniper");
    assert_eq!(connection.identity.id, "6954990");
}

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
                "owner": {"login": "jdylanmc", "type": "User"},
                "private": false,
                "archived": false,
                "disabled": false,
                "permissions": {"pull": true, "push": true}
            }),
            "/repos/third-party/public-repository" => json!({
                "id": 400,
                "full_name": "third-party/public-repository",
                "private": false,
                "archived": false,
                "disabled": false,
                "permissions": {"pull": true, "push": false}
            }),
            "/repos/jdylanmc/pr-sniper/pulls?state=open&per_page=1" => json!([]),
            "/repos/third-party/public-repository/pulls?state=open&per_page=1" => json!([]),
            "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1" => json!([
                {
                    "id": 1376547672,
                    "full_name": "jdylanmc/pr-sniper",
                    "owner": {"login": "jdylanmc", "type": "User"},
                    "private": true
                },
                {
                    "id": 200,
                    "full_name": "octo/public-repository",
                    "owner": {"login": "octo", "type": "User"},
                    "private": false
                },
                {
                    "id": 300,
                    "full_name": "example-org/private-repository",
                    "owner": {"login": "example-org", "type": "Organization"},
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

#[test]
fn owner_browser_distinguishes_personal_org_and_collaborator_ownership() {
    let client = GithubClient::new(ConnectionTransport);
    let identity = Identity {
        id: "6954990".into(),
        login: "jdylanmc".into(),
    };
    let owners = client.repository_owners(&identity).unwrap();
    assert_eq!(
        owners
            .iter()
            .map(|o| (o.login.as_str(), o.kind))
            .collect::<Vec<_>>(),
        vec![("example-org", "organization"), ("jdylanmc", "personal")]
    );
    assert_eq!(client.owner_repositories("jdylanmc").unwrap().len(), 1);
    assert_eq!(
        client.owner_repositories("example-org").unwrap()[0].id,
        "300"
    );
    assert!(client.owner_repositories("").is_err());
    assert!(client.owner_repositories("../octo").is_err());
    assert_eq!(
        GithubClient::new(PaginatedRepositories)
            .owner_repositories("octo")
            .unwrap()
            .len(),
        101
    );
    assert_eq!(
        GithubClient::new(MissingScope).repository_owners(&identity),
        Err(ConnectionError::MissingScope)
    );
}

#[test]
fn unaffiliated_public_repository_resolves_directly_when_discovery_omits_it() {
    let client = GithubClient::new(ConnectionTransport);
    let discovered = client.accessible_repositories().unwrap();
    assert!(!discovered
        .iter()
        .any(|repository| repository.name == "third-party/public-repository"));

    let connection = client
        .connect("third-party/public-repository", Some("6954990"))
        .unwrap();

    assert_eq!(connection.repository.id, "400");
    assert_eq!(connection.repository.name, "third-party/public-repository");
    assert!(connection.capabilities.read);
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

struct ManagedAccountRepositories;

impl Transport for ManagedAccountRepositories {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        assert!(path.starts_with("/user/repos?"));
        Ok(Response {
            status: 200,
            headers: BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]),
            body: serde_json::to_vec(&json!([
                {
                    "id": 101, "full_name": "fixture_corp/one", "private": true,
                    "owner": {"login": "fixture_corp", "type": "User",
                              "name": null, "extra_metadata": {"ignored": true}}
                },
                {
                    "id": 102, "full_name": "orbit/two", "private": true,
                    "owner": {"login": "orbit", "type": "Organization"}
                }
            ]))
            .unwrap(),
        })
    }
}

#[test]
fn managed_account_repository_metadata_does_not_invalidate_all_owners() {
    let client = GithubClient::new(ManagedAccountRepositories);
    let owners = client
        .repository_owners(&Identity {
            id: "22".into(),
            login: "fixture_corp".into(),
        })
        .unwrap();
    assert_eq!(
        owners.iter().map(|o| o.login.as_str()).collect::<Vec<_>>(),
        ["fixture_corp", "orbit"]
    );
    assert_eq!(
        client.owner_repositories("fixture_corp").unwrap()[0].id,
        "101"
    );
}

struct ShortPages {
    fail_second: bool,
}

impl Transport for ShortPages {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let second = path.ends_with("page=2");
        if second && self.fail_second {
            return Err(ConnectionError::Network);
        }
        assert!(second || path.ends_with("page=1"));
        Ok(Response {
            status: 200,
            headers: if second {
                BTreeMap::from([("x-oauth-scopes".into(), "repo".into())])
            } else {
                BTreeMap::from([
                    ("x-oauth-scopes".into(), "repo".into()),
                    ("link".into(), "<https://api.github.com/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=2>; rel=\"next\"".into()),
                ])
            },
            body: serde_json::to_vec(&json!([{
                "id": if second { 102 } else { 101 },
                "full_name": if second { "orbit/two" } else { "orbit/one" },
                "private": true,
                "owner": {"login": "orbit", "type": "Organization"},
            }]))
            .unwrap(),
        })
    }
}

#[test]
fn owner_catalog_follows_short_page_next_and_never_returns_partial_success() {
    let identity = Identity {
        id: "22".into(),
        login: "personal".into(),
    };
    let client = GithubClient::new(ShortPages { fail_second: false });
    assert_eq!(client.owner_repositories("orbit").unwrap().len(), 2);
    assert_eq!(client.repository_owners(&identity).unwrap().len(), 2);
    let client = GithubClient::new(ShortPages { fail_second: true });
    assert_eq!(
        client.owner_repositories("orbit"),
        Err(ConnectionError::Network)
    );
    assert_eq!(
        client.repository_owners(&identity),
        Err(ConnectionError::Network)
    );
}

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

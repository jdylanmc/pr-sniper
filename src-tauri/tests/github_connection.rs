use pr_sniper_lib::github::provider::{
    Capabilities, CommentCapability, Connection, GithubClient, InstalledRepository,
    RemoteRepository, Response, Transport,
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
            "/user/installations?per_page=100&page=1" => json!({
                "installations": [{"id": 9001}]
            }),
            "/user/installations/9001/repositories?per_page=100&page=1" => json!({
                "repositories": [{
                    "id": 1376547672,
                    "full_name": "jdylanmc/pr-sniper"
                }]
            }),
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
fn app_user_token_lists_installed_repositories_with_stable_ids() {
    let client = GithubClient::new(ConnectionTransport);

    assert_eq!(
        client.installed_repositories(),
        Ok(vec![InstalledRepository {
            installation_id: "9001".into(),
            repository: RemoteRepository {
                id: "1376547672".into(),
                name: "jdylanmc/pr-sniper".into(),
            },
        }])
    );
}

struct PaginatedInstallations;

impl Transport for PaginatedInstallations {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let body = match path {
            "/user/installations?per_page=100&page=1" => {
                json!({"installations": [{"id": 9001}]})
            }
            "/user/installations/9001/repositories?per_page=100&page=1" => {
                json!({"repositories": (1..=100).map(|id| json!({
                    "id": id,
                    "full_name": format!("octo/repository-{id}")
                })).collect::<Vec<_>>()})
            }
            "/user/installations/9001/repositories?per_page=100&page=2" => {
                json!({"repositories": [{
                    "id": 101,
                    "full_name": "octo/repository-101"
                }]})
            }
            _ => panic!("unexpected GET path: {path}"),
        };
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn installed_repository_discovery_exhausts_every_page() {
    let repositories = GithubClient::new(PaginatedInstallations)
        .installed_repositories()
        .unwrap();

    assert_eq!(repositories.len(), 101);
    assert_eq!(repositories[100].repository.id, "101");
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

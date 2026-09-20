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

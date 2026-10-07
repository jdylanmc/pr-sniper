use pr_sniper_lib::github::{
    intake::repository_target,
    provider::{GithubClient, Response, Transport},
    ConnectionError,
};
use serde_json::json;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

struct Provider {
    calls: Rc<RefCell<Vec<String>>>,
    status: u16,
    number: u64,
}

impl Transport for Provider {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.calls.borrow_mut().push(path.into());
        let (status, body) = match path {
            "/user" => (200, json!({"id":22,"login":"chosen"})),
            "/repos/owner/repo" => (
                200,
                json!({
                    "id":100,"full_name":"owner/repo","private":true,
                    "archived":false,"disabled":false,"permissions":{"pull":true}
                }),
            ),
            "/repos/owner/repo/pulls?state=open&per_page=1" => (200, json!([])),
            "/repos/owner/repo/pulls/302" => (
                self.status,
                json!({
                    "id":900,"number":self.number,"title":"Exact requested PR",
                    "state":"open","draft":false,"user":{"id":99,"login":"unwatched"},
                    "requested_reviewers":[],"requested_teams":[],
                    "head":{"sha":"a".repeat(40),"repo":{"id":100}},
                    "base":{"sha":"b".repeat(40),"repo":{"id":100}},
                    "updated_at":"2026-10-07T00:00:00Z"
                }),
            ),
            _ => panic!("Unexpected provider read: {path}"),
        };
        Ok(Response {
            status,
            headers: BTreeMap::from([("x-oauth-scopes".into(), "repo".into())]),
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn supported_pr_url_variants_preserve_exact_number_without_changing_repository_urls() {
    for suffix in [
        "",
        "/",
        "/files",
        "/commits",
        "/checks",
        "?diff=split#discussion",
    ] {
        let target =
            repository_target(&format!("https://github.com/Owner/Repo/pull/302{suffix}")).unwrap();
        assert_eq!(target.name, "owner/repo");
        assert_eq!(target.pull_request_number, Some(302));
    }
    for input in ["owner/repo", "https://github.com/Owner/Repo.git/"] {
        assert_eq!(repository_target(input).unwrap().pull_request_number, None);
    }
    for input in [
        "https://github.com/owner/repo/pull/0",
        "https://github.com/owner/repo/pull/-1",
        "https://github.com/owner/repo/pull/18446744073709551616",
        "https://github.com/owner/repo/pull/302/unknown",
        "https://github.com.example/owner/repo/pull/302",
        "https://actor:secret@github.com/owner/repo/pull/302",
        "http://github.com/owner/repo/pull/302",
        "https://github.com:444/owner/repo/pull/302",
        "owner/repo/pull/302",
    ] {
        assert!(repository_target(input).is_err(), "{input}");
    }
}

#[test]
fn selected_account_resolves_exact_pr_and_repository_urls_do_not_fetch_pr_detail() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        calls: calls.clone(),
        status: 200,
        number: 302,
    };
    let client = GithubClient::new(provider);
    let resolved = client
        .resolve_repository_target("https://github.com/owner/repo/pull/302", "22")
        .unwrap();
    assert_eq!(resolved.connection.identity.id, "22");
    assert_eq!(resolved.connection.repository.id, "100");
    let pull = resolved.pull_request.unwrap();
    assert_eq!(pull.number, 302);
    assert_eq!(pull.id, "900");
    assert_eq!(pull.author.unwrap().id, "99");
    assert_eq!(
        *calls.borrow(),
        [
            "/user",
            "/repos/owner/repo",
            "/repos/owner/repo/pulls?state=open&per_page=1",
            "/repos/owner/repo/pulls/302",
        ]
    );
    calls.borrow_mut().clear();
    let resolved = client
        .resolve_repository_target("owner/repo", "22")
        .unwrap();
    assert!(resolved.pull_request.is_none());
    assert_eq!(
        *calls.borrow(),
        [
            "/user",
            "/repos/owner/repo",
            "/repos/owner/repo/pulls?state=open&per_page=1",
        ]
    );
}

#[test]
fn exact_pr_http_failures_and_wrong_account_or_number_are_not_successful_resolution() {
    for (status, expected) in [
        (401, ConnectionError::SignedOut),
        (404, ConnectionError::MissingReadPermission),
        (500, ConnectionError::ProviderFailure),
    ] {
        let client = GithubClient::new(Provider {
            calls: Rc::default(),
            status,
            number: 302,
        });
        assert_eq!(
            client
                .resolve_repository_target("https://github.com/owner/repo/pull/302", "22")
                .unwrap_err(),
            expected
        );
    }
    let client = GithubClient::new(Provider {
        calls: Rc::default(),
        status: 200,
        number: 303,
    });
    assert_eq!(
        client
            .resolve_repository_target("https://github.com/owner/repo/pull/302", "22")
            .unwrap_err(),
        ConnectionError::RepositoryChanged
    );
    assert_eq!(
        client
            .resolve_repository_target("https://github.com/owner/repo/pull/302", "44")
            .unwrap_err(),
        ConnectionError::WrongIdentity
    );
}

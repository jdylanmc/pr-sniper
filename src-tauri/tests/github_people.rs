use pr_sniper_lib::github::provider::{GithubClient, Response, Transport};
use pr_sniper_lib::github::ConnectionError;
use std::cell::RefCell;
use std::collections::BTreeMap;

struct People {
    calls: RefCell<Vec<String>>,
    response: &'static str,
}
impl Transport for &People {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.calls.borrow_mut().push(path.into());
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: self.response.as_bytes().to_vec(),
        })
    }
}

#[test]
fn person_lookup_uses_provider_identity_and_rejects_path_injection() {
    let transport = People {
        calls: RefCell::new(vec![]),
        response: r#"{"id":42,"login":"Octocat"}"#,
    };
    let client = GithubClient::new(&transport);
    let person = client.resolve_person("octocat").unwrap();
    assert_eq!(person.id, "42");
    assert_eq!(person.login, "Octocat");
    for invalid in ["", "../user", "name?query", "name/path", "-name", "name-"] {
        assert!(client.resolve_person(invalid).is_err());
    }
    assert_eq!(*transport.calls.borrow(), vec!["/users/octocat"]);
}

#[test]
fn person_lookup_never_accepts_a_missing_stable_identity() {
    let transport = People {
        calls: RefCell::new(vec![]),
        response: r#"{"login":"octocat"}"#,
    };
    assert_eq!(
        GithubClient::new(&transport).resolve_person("octocat"),
        Err(ConnectionError::InvalidResponse)
    );
}

#[test]
fn user_search_encodes_login_queries_and_validates_real_empty_and_partial_results() {
    let transport = People {
        calls: RefCell::new(vec![]),
        response: r#"{"incomplete_results":false,"items":[{"id":42,"login":"Octocat"}]}"#,
    };
    let people = GithubClient::new(&transport)
        .search_people(" @octo ")
        .unwrap();
    assert_eq!(people[0].id, "42");
    assert_eq!(
        *transport.calls.borrow(),
        vec!["/search/users?q=octo+in%3Alogin+type%3Auser&per_page=30"]
    );
    for invalid in [
        "",
        "../user",
        "name?query",
        "name/path",
        "org:private",
        "a b",
    ] {
        assert!(GithubClient::new(&transport)
            .search_people(invalid)
            .is_err());
    }
    assert_eq!(transport.calls.borrow().len(), 1);
    for (body, expected) in [
        (r#"{"incomplete_results":false,"items":[]}"#, Ok(vec![])),
        (
            r#"{"incomplete_results":true,"items":[]}"#,
            Err(ConnectionError::IncompleteRead),
        ),
        (
            r#"{"incomplete_results":false,"items":[{"login":"no-id"}]}"#,
            Err(ConnectionError::InvalidResponse),
        ),
        (
            r#"{"incomplete_results":false,"items":[{"id":42,"login":"a"},{"id":42,"login":"b"}]}"#,
            Err(ConnectionError::InvalidResponse),
        ),
    ] {
        let transport = People {
            calls: RefCell::new(vec![]),
            response: body,
        };
        assert_eq!(
            GithubClient::new(&transport).search_people("octo"),
            expected
        );
    }
}

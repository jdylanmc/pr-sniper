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

use super::*;
use crate::azure_devops::token_source::AccessToken;
use std::collections::VecDeque;
use std::sync::Mutex;
use zeroize::Zeroizing;

const TENANT: &str = "00000000-0000-0000-0000-00000000000a";
const AVERY: &str = "00000000-0000-0000-0000-000000000001";
const BLAKE: &str = "00000000-0000-0000-0000-000000000002";
const CASEY: &str = "00000000-0000-0000-0000-000000000003";
const PROJECT: &str = "00000000-0000-0000-0000-0000000000b1";
const OTHER_PROJECT: &str = "00000000-0000-0000-0000-0000000000b2";
const REPO: &str = "00000000-0000-0000-0000-0000000000c1";
const OTHER_REPO: &str = "00000000-0000-0000-0000-0000000000c2";
const ORG: &str = "https://dev.azure.com/contoso";

/// Token per call: `token-1`, `token-2`, ...; records invalidations.
#[derive(Default)]
struct FakeTokens {
    issued: Mutex<u32>,
    invalidated: Mutex<Vec<String>>,
    fail: Option<TokenError>,
}

#[async_trait]
impl TokenSource for FakeTokens {
    async fn token(&self, tenant: &TenantId) -> Result<AccessToken, TokenError> {
        assert_eq!(tenant.as_str(), TENANT);
        if let Some(error) = self.fail {
            return Err(error);
        }
        let mut issued = self.issued.lock().unwrap();
        *issued += 1;
        Ok(AccessToken::new(
            Zeroizing::new(format!("token-{issued}")),
            i64::MAX,
        ))
    }

    fn invalidate(&self, _tenant: &TenantId, rejected: &AccessToken) {
        self.invalidated
            .lock()
            .unwrap()
            .push(rejected.secret().to_owned());
    }
}

type Reply = Result<Response, NetworkError>;

/// Replies by longest URL prefix, in order; records each request and token.
#[derive(Default)]
struct FakeWire {
    replies: Mutex<Vec<(String, VecDeque<Reply>)>>,
    sent: Mutex<Vec<(Request, String)>>,
}

impl FakeWire {
    fn on(&self, prefix: &str, reply: Reply) -> &Self {
        let mut replies = self.replies.lock().unwrap();
        match replies.iter_mut().find(|(known, _)| known == prefix) {
            Some((_, queue)) => queue.push_back(reply),
            None => replies.push((prefix.into(), VecDeque::from([reply]))),
        }
        self
    }

    fn json(&self, prefix: &str, body: Value) -> &Self {
        self.on(prefix, Ok(ok(body)))
    }

    fn urls(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(request, _)| request.url.clone())
            .collect()
    }
}

fn ok(body: Value) -> Response {
    Response {
        status: 200,
        body: body.to_string().into_bytes(),
        ..Response::default()
    }
}

fn status(status: u16, retry_after: Option<&str>) -> Response {
    Response {
        status,
        retry_after: retry_after.map(str::to_owned),
        ..Response::default()
    }
}

#[async_trait]
impl Transport for FakeWire {
    async fn send(&self, request: &Request, token: &AccessToken) -> Result<Response, NetworkError> {
        self.sent
            .lock()
            .unwrap()
            .push((request.clone(), token.secret().to_owned()));
        let mut replies = self.replies.lock().unwrap();
        let (_, queue) = replies
            .iter_mut()
            .filter(|(prefix, queue)| request.url.starts_with(prefix.as_str()) && !queue.is_empty())
            .max_by_key(|(prefix, _)| prefix.len())
            .unwrap_or_else(|| panic!("unexpected request: {}", request.url));
        queue.pop_front().unwrap()
    }
}

fn client(tokens: Arc<FakeTokens>, wire: Arc<FakeWire>) -> Client {
    Client::new(tokens, wire, TenantId::parse(TENANT).unwrap(), "Contoso")
}

fn fakes() -> (Arc<FakeTokens>, Arc<FakeWire>, Client) {
    let tokens = Arc::new(FakeTokens::default());
    let wire = Arc::new(FakeWire::default());
    let client = client(Arc::clone(&tokens), Arc::clone(&wire));
    (tokens, wire, client)
}

fn raw_repository(id: &str, name: &str, project: &str) -> Value {
    json!({"id": id, "name": name, "project": {"id": PROJECT, "name": project}, "isDisabled": false})
}

fn key() -> RepositoryKey {
    RepositoryKey {
        organization: "contoso".into(),
        project_id: TenantId::parse(PROJECT).unwrap(),
        repository_id: TenantId::parse(REPO).unwrap(),
    }
}

fn identity_body(id: &str, name: &str) -> Value {
    json!({"authenticatedUser": {"id": id, "providerDisplayName": name}})
}

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn parses_pasted_repository_urls() {
    let parsed = |input: &str| {
        parse_repository_url(input).map(|path| (path.organization, path.project, path.repository))
    };
    let expected = Some(("contoso".into(), "Web".into(), "app".into()));
    for input in [
        "https://dev.azure.com/contoso/Web/_git/app",
        "https://dev.azure.com/Contoso/Web/_git/app/",
        "https://avery@dev.azure.com/contoso/Web/_git/app",
        "https://dev.azure.com/contoso/Web/_git/app/pullrequest/7?_a=files",
        "https://contoso.visualstudio.com/Web/_git/app",
        "https://contoso.visualstudio.com/DefaultCollection/Web/_git/app",
        "contoso/Web/app",
    ] {
        assert_eq!(parsed(input), expected, "{input}");
    }
    assert_eq!(
        parsed("https://dev.azure.com/contoso/Fabrikam%20Web/_git/My%20App"),
        Some(("contoso".into(), "Fabrikam Web".into(), "My App".into()))
    );
    assert_eq!(
        parsed("https://dev.azure.com/contoso/_git/app"),
        Some(("contoso".into(), "app".into(), "app".into()))
    );
    for input in [
        "",
        "http://dev.azure.com/contoso/Web/_git/app",
        "https://user:secret@dev.azure.com/contoso/Web/_git/app",
        "https://dev.azure.com:8443/contoso/Web/_git/app",
        "https://dev.azure.com/contoso/Web/app",
        "https://dev.azure.com/contoso/Web/_git",
        "https://dev.azure.com/contoso/a/b/_git/app",
        "https://example.com/contoso/Web/_git/app",
        "https://visualstudio.com/Web/_git/app",
        "https://github.com/contoso/app",
        "https://dev.azure.com/-bad-/Web/_git/app",
        "https://dev.azure.com/contoso/Web/_git/a%2Fb",
        "https://dev.azure.com/contoso/Web/_git/%ZZ",
        "contoso/app",
    ] {
        assert_eq!(parsed(input), None, "{input}");
    }
}

#[test]
fn repository_keys_are_stable_lowercase_guids() {
    let id = key().id();
    assert_eq!(id, format!("contoso/{PROJECT}/{REPO}"));
    assert_eq!(RepositoryKey::parse(&id), Some(key()));
    for invalid in [
        format!("Contoso/{PROJECT}/{REPO}"),
        format!("contoso/{}/{REPO}", PROJECT.to_uppercase()),
        format!("contoso/web/{REPO}"),
        format!("contoso/{PROJECT}/{REPO}/x"),
        format!("contoso/{PROJECT}"),
    ] {
        assert_eq!(RepositoryKey::parse(&invalid), None, "{invalid}");
    }
}

#[test]
fn every_request_carries_the_tenant_token() {
    let (_, wire, client) = fakes();
    wire.json(
        &format!("{ORG}/_apis/projects"),
        json!({"value": [{"id": PROJECT, "name": "Web", "state": "wellFormed"}]}),
    );
    block(client.projects()).unwrap();
    let sent = wire.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "token-1");
    assert_eq!(sent[0].0.body, None);
}

#[test]
fn lists_well_formed_projects_across_continuation_pages() {
    let (_, wire, client) = fakes();
    wire.on(
        &format!("{ORG}/_apis/projects?api-version=7.1&%24top=500"),
        Ok(Response {
            continuation: Some("next".into()),
            ..ok(json!({"value": [
                {"id": OTHER_PROJECT, "name": "web", "state": "wellFormed"},
                {"id": PROJECT, "name": "Deleting", "state": "deleting"},
                {"id": "not-a-guid", "name": "Broken", "state": "wellFormed"}
            ]}))
        }),
    )
    .json(
        &format!("{ORG}/_apis/projects?api-version=7.1&%24top=500&continuationToken=next"),
        json!({"value": [{"id": PROJECT, "name": "Api", "state": "wellFormed"}]}),
    );
    let (projects, warnings) = block(client.projects()).unwrap();
    assert_eq!(
        projects
            .iter()
            .map(|project| (project.id.as_str(), project.name.as_str()))
            .collect::<Vec<_>>(),
        [(PROJECT, "Api"), (OTHER_PROJECT, "web")]
    );
    assert!(warnings.is_empty());
    assert_eq!(wire.urls().len(), 2);
}

#[test]
fn a_listing_cut_at_the_page_bound_is_incomplete() {
    let (_, wire, client) = fakes();
    for _ in 0..PROJECT_PAGES {
        wire.on(
            &format!("{ORG}/_apis/projects"),
            Ok(Response {
                continuation: Some("more".into()),
                ..ok(json!({"value": []}))
            }),
        );
    }
    let (_, warnings) = block(client.projects()).unwrap();
    assert_eq!(
        warnings,
        [RepositoryBrowseWarning {
            boundary: "pagination",
            page: PROJECT_PAGES,
            error: ConnectionError::IncompleteRead,
        }]
    );
}

#[test]
fn lists_enabled_repositories_of_one_project_with_stable_identities() {
    let (_, wire, client) = fakes();
    wire.json(
        &format!("{ORG}/Fabrikam%20Web/_apis/git/repositories?api-version=7.1"),
        json!({"value": [
            raw_repository(REPO, "zeta", "Fabrikam Web"),
            {"id": OTHER_REPO, "name": "old", "project": {"id": PROJECT, "name": "Fabrikam Web"}, "isDisabled": true},
            raw_repository(OTHER_REPO, "Alpha", "Fabrikam Web"),
        ]}),
    );
    let repositories = block(client.repositories("Fabrikam Web")).unwrap();
    assert_eq!(
        repositories
            .iter()
            .map(Repository::remote)
            .collect::<Vec<_>>(),
        [
            RemoteRepository {
                id: format!("contoso/{PROJECT}/{OTHER_REPO}"),
                name: "contoso/Fabrikam Web/Alpha".into()
            },
            RemoteRepository {
                id: format!("contoso/{PROJECT}/{REPO}"),
                name: "contoso/Fabrikam Web/zeta".into()
            },
        ]
    );
}

#[test]
fn resolves_urls_paths_and_stable_ids_within_the_organization() {
    let (_, wire, client) = fakes();
    for prefix in [
        format!("{ORG}/Web/_apis/git/repositories/app?api-version=7.1"),
        format!("{ORG}/Web/_apis/git/repositories/app?api-version=7.1"),
        format!("{ORG}/{PROJECT}/_apis/git/repositories/{REPO}?api-version=7.1"),
    ] {
        wire.json(&prefix, raw_repository(REPO, "app", "Web"));
    }
    for input in [
        "https://contoso.visualstudio.com/DefaultCollection/Web/_git/app".to_owned(),
        "contoso/Web/app".to_owned(),
        key().id(),
    ] {
        assert_eq!(block(client.resolve(&input)).unwrap().key, key(), "{input}");
    }
    assert_eq!(
        block(client.resolve("https://dev.azure.com/fabrikam/Web/_git/app")),
        Err(ConnectionError::RepositoryUnavailable)
    );
    assert_eq!(
        block(client.resolve("https://github.com/contoso/app")),
        Err(ConnectionError::InvalidRepository)
    );
    assert_eq!(wire.urls().len(), 3);
}

#[test]
fn a_stable_id_whose_repository_moved_is_changed() {
    let (_, wire, client) = fakes();
    wire.json(
        &format!("{ORG}/{PROJECT}/_apis/git/repositories/{REPO}"),
        json!({"id": REPO, "name": "app", "project": {"id": OTHER_PROJECT, "name": "Moved"}}),
    );
    assert_eq!(
        block(client.resolve(&key().id())),
        Err(ConnectionError::RepositoryChanged)
    );
}

#[test]
fn a_401_drops_the_token_and_retries_once() {
    let (tokens, wire, client) = fakes();
    let projects = format!("{ORG}/_apis/projects");
    wire.on(&projects, Ok(status(401, None)))
        .json(&projects, json!({"value": []}));
    block(client.projects()).unwrap();
    assert_eq!(*tokens.invalidated.lock().unwrap(), ["token-1"]);
    assert_eq!(
        wire.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(_, token)| token.as_str())
            .collect::<Vec<_>>(),
        ["token-1", "token-2"]
    );

    let (tokens, wire, client) = fakes();
    // A sign-in redirect or page is the same refusal as a 401.
    wire.on(&projects, Ok(status(302, None)))
        .on(&projects, Ok(status(203, None)));
    assert_eq!(block(client.projects()), Err(ConnectionError::SignedOut));
    assert_eq!(tokens.invalidated.lock().unwrap().len(), 1);
    assert_eq!(wire.urls().len(), 2);
}

#[test]
fn failures_are_typed_and_retry_after_reaches_the_retry_policy() {
    let projects = format!("{ORG}/_apis/projects");
    for (response, expected) in [
        (
            Ok(status(403, None)),
            ConnectionError::MissingReadPermission,
        ),
        (
            Ok(status(404, None)),
            ConnectionError::RepositoryUnavailable,
        ),
        (
            Ok(status(429, Some("17"))),
            ConnectionError::RateLimitedAfter(17),
        ),
        (
            Ok(status(429, Some("Wed, 21 Oct 2026 07:28:00 GMT"))),
            ConnectionError::RateLimited,
        ),
        (Ok(status(429, None)), ConnectionError::RateLimited),
        (
            Ok(status(429, Some("999999999"))),
            ConnectionError::RateLimitedAfter(crate::retry::PROVIDER_CAP_SECONDS),
        ),
        (
            Ok(status(503, Some("30"))),
            ConnectionError::ProviderFailureAfter(30),
        ),
        (Ok(status(500, None)), ConnectionError::ProviderFailure),
        (
            Ok(status(400, None)),
            ConnectionError::ProviderRejectedStatus(400),
        ),
        (
            Ok(ok(json!("not a list"))),
            ConnectionError::InvalidResponse,
        ),
        (Ok(status(200, None)), ConnectionError::InvalidResponse),
        (Err(NetworkError), ConnectionError::Network),
    ] {
        let (tokens, wire, client) = fakes();
        wire.on(&projects, response);
        assert_eq!(
            block(client.projects()).map(|_| ()),
            Err(expected),
            "{expected:?}"
        );
        assert!(tokens.invalidated.lock().unwrap().is_empty());
        assert_eq!(wire.urls().len(), 1, "never retried in-process");
    }
}

#[test]
fn token_failures_are_typed_without_a_request() {
    for (error, expected) in [
        (TokenError::AzMissing, ConnectionError::MissingCli),
        (TokenError::AzTooOld, ConnectionError::MissingCli),
        (TokenError::NotLoggedIn, ConnectionError::SignedOut),
        (
            TokenError::InteractionRequired { aadsts: 50076 },
            ConnectionError::SignedOut,
        ),
        (
            TokenError::TenantAccessBlocked,
            ConnectionError::OrganizationPolicyDenied,
        ),
        (TokenError::Network, ConnectionError::Network),
        (TokenError::Timeout, ConnectionError::Timeout),
        (TokenError::MalformedOutput, ConnectionError::BrokenCli),
    ] {
        let tokens = Arc::new(FakeTokens {
            fail: Some(error),
            ..FakeTokens::default()
        });
        let wire = Arc::new(FakeWire::default());
        let client = client(tokens, Arc::clone(&wire));
        assert_eq!(block(client.projects()).map(|_| ()), Err(expected));
        assert!(wire.urls().is_empty());
    }
}

fn picker() -> String {
    format!("{ORG}/_apis/IdentityPicker/Identities?api-version=7.1-preview.1")
}

fn recent() -> String {
    format!("{ORG}/{PROJECT}/_apis/git/repositories/{REPO}/pullrequests?api-version=7.1&searchCriteria.status=all&%24top=100")
}

#[test]
fn people_search_posts_the_identity_picker_query() {
    let (_, wire, client) = fakes();
    wire.json(
        &picker(),
        json!({"results": [{"queryToken": "av", "identities": [
            {"entityType": "User", "localId": AVERY, "displayName": "Avery Example", "mail": "avery@contoso.example"},
            {"entityType": "User", "localId": null, "displayName": "Not In Organization"},
            {"entityType": "Group", "localId": CASEY, "displayName": "Avid Group"},
            {"entityType": "User", "localId": BLAKE, "displayName": "Avery Two", "signInAddress": "avery2@contoso.example"},
            {"entityType": "User", "localId": AVERY, "displayName": "Avery Example"},
        ]}]}),
    );
    let people = block(client.people(" av ", Some(&key()))).unwrap();
    assert_eq!(people.source, PeopleSource::Directory);
    assert_eq!(
        people.people,
        [
            PersonMatch {
                id: AVERY.into(),
                login: "Avery Example".into(),
                mail: Some("avery@contoso.example".into())
            },
            PersonMatch {
                id: BLAKE.into(),
                login: "Avery Two".into(),
                mail: Some("avery2@contoso.example".into())
            },
        ]
    );
    let sent = wire.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].0.body,
        Some(json!({
            "query": "av",
            "identityTypes": ["user"],
            "operationScopes": ["ims", "source"],
            "properties": ["DisplayName", "Mail"],
            "options": {"MinResults": 5, "MaxResults": 20},
        }))
    );
}

#[test]
fn people_search_falls_back_to_recent_pull_request_people() {
    let (_, wire, client) = fakes();
    wire.on(&picker(), Ok(status(500, None))).json(
        &recent(),
        json!({"value": [
            {"createdBy": {"id": BLAKE, "displayName": "Blake Example", "uniqueName": "blake@contoso.example"},
             "reviewers": [
                {"id": AVERY, "displayName": "Avery Example", "uniqueName": "avery@contoso.example"},
                {"id": CASEY, "displayName": "[Web]\\Contributors", "isContainer": true}
             ]},
            {"createdBy": {"id": AVERY, "displayName": "Avery Example", "uniqueName": "avery@contoso.example"},
             "reviewers": [{"id": CASEY, "displayName": "Casey Brooks", "uniqueName": "casey@contoso.example"}]}
        ]}),
    );
    let people = block(client.people("ex", Some(&key()))).unwrap();
    assert_eq!(people.source, PeopleSource::Recent);
    assert_eq!(
        people
            .people
            .iter()
            .map(|person| person.id.as_str())
            .collect::<Vec<_>>(),
        [BLAKE, AVERY]
    );
    wire.on(&picker(), Ok(status(429, Some("5")))).json(
        &recent(),
        json!({"value": [{"createdBy": {"id": CASEY, "displayName": "Casey Brooks", "uniqueName": "casey@contoso.example"}}]}),
    );
    assert_eq!(
        block(client.people("CASEY@", Some(&key()))).unwrap().people[0].id,
        CASEY
    );
}

#[test]
fn people_search_without_a_fallback_reports_the_directory_failure() {
    let (_, wire, client) = fakes();
    wire.on(&picker(), Ok(status(500, None)));
    assert_eq!(
        block(client.people("av", None)),
        Err(ConnectionError::ProviderFailure)
    );
    // A refused token cannot read the fallback either.
    wire.on(&picker(), Ok(status(401, None)))
        .on(&picker(), Ok(status(401, None)));
    assert_eq!(
        block(client.people("av", Some(&key()))),
        Err(ConnectionError::SignedOut)
    );
    assert!(!wire.urls().iter().any(|url| url.contains("pullrequests")));
    // Empty queries send nothing; oversized ones are rejected.
    let before = wire.urls().len();
    assert!(block(client.people("  ", Some(&key())))
        .unwrap()
        .people
        .is_empty());
    assert!(block(client.people(&"a".repeat(101), None)).is_err());
    assert_eq!(wire.urls().len(), before);
}

fn reader(
    wire: &Arc<FakeWire>,
    runtime: &tokio::runtime::Runtime,
) -> Result<Reader, ConnectionError> {
    let tokens = Arc::new(FakeTokens::default());
    Reader::open(
        client(tokens, Arc::clone(wire)),
        runtime.handle().clone(),
        &Identity {
            id: AVERY.into(),
            display_name: "Avery Example".into(),
            unique_name: None,
        },
    )
}

#[test]
fn the_reader_is_bound_to_the_signed_in_identity() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let wire = Arc::new(FakeWire::default());
    let connection_data = format!("{ORG}/_apis/connectionData");
    wire.json(&connection_data, identity_body(BLAKE, "Blake Example"));
    assert_eq!(
        reader(&wire, &runtime).err(),
        Some(ConnectionError::WrongIdentity)
    );
    wire.json(&connection_data, identity_body(AVERY, "Avery Example"));
    let reader = reader(&wire, &runtime).unwrap();
    assert_eq!(
        reader.identity(),
        &Person {
            id: AVERY.into(),
            login: "Avery Example".into()
        }
    );
    assert_eq!(
        reader.connect("contoso/Web/app", Some(BLAKE)),
        Err(ConnectionError::WrongIdentity)
    );
}

#[test]
fn the_reader_implements_the_provider_read_interfaces() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let wire = Arc::new(FakeWire::default());
    let directory = json!({"results": [{"identities": [
        {"entityType": "User", "localId": BLAKE, "displayName": "Blake Example", "mail": "blake@contoso.example"}
    ]}]});
    wire.json(
        &format!("{ORG}/_apis/connectionData"),
        identity_body(AVERY, "Avery Example"),
    )
    .json(
        &format!("{ORG}/_apis/projects"),
        json!({"value": [{"id": PROJECT, "name": "Web", "state": "wellFormed"}]}),
    )
    .json(
        &format!("{ORG}/Web/_apis/git/repositories?"),
        json!({"value": [raw_repository(REPO, "app", "Web")]}),
    )
    .json(
        &format!("{ORG}/Web/_apis/git/repositories/app?"),
        raw_repository(REPO, "app", "Web"),
    )
    .json(&picker(), directory.clone())
    .json(&picker(), directory);
    let reader = reader(&wire, &runtime).unwrap();
    let identity = reader.identity().clone();
    let browser = reader.repository_browser(&identity).unwrap();
    assert_eq!(
        browser.owners,
        [RepositoryOwner {
            login: "Web".into(),
            kind: "project"
        }]
    );
    let repositories = reader.owner_repository_browser(&identity, "Web").unwrap();
    let remote = RemoteRepository {
        id: key().id(),
        name: "contoso/Web/app".into(),
    };
    assert_eq!(repositories.repositories, std::slice::from_ref(&remote));
    let target = reader
        .resolve_repository_target(
            "https://dev.azure.com/contoso/Web/_git/app/pullrequest/4",
            AVERY,
        )
        .unwrap();
    assert_eq!(target.connection.repository, remote);
    assert_eq!(target.connection.identity, identity);
    assert!(target.pull_request.is_none());
    assert_eq!(
        reader.resolve_person("blake@contoso.example").unwrap(),
        Person {
            id: BLAKE.into(),
            login: "Blake Example".into()
        }
    );
    assert_eq!(
        reader.search_people("bl", Some(&remote)).unwrap().people[0].id,
        BLAKE
    );
}

mod polling;
mod pulls;

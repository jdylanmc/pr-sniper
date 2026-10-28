//! Azure DevOps repository discovery and resolution (FR-30) and people search
//! (FR-35) behind the provider read interfaces. Requests suppress the FedAuth
//! sign-in redirect and never follow redirects; a 401 drops the token and
//! retries once; `Retry-After` reaches the shared retry policy (FR-36).

use super::connection::{self, Identity};
use super::tenant::{NetworkError, TenantId};
use super::token_source::{
    with_token, AccessToken, AuthorizedError, Rejection, TokenError, TokenSource,
};
use crate::provider::{
    AccountReader, Capabilities, CommentCapability, Connection, ConnectionError, People,
    PeopleSearch, PeopleSource, Person, PersonMatch, RemoteRepository, RepositoryBrowseWarning,
    RepositoryBrowser, RepositoryOwner, RepositoryReader, ResolvedTarget,
};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const API_VERSION: &str = "7.1";
const PICKER_API_VERSION: &str = "7.1-preview.1";
const BODY_LIMIT: usize = 8 * 1024 * 1024;
/// Project pages followed through `x-ms-continuationtoken`.
const PROJECT_PAGES: u64 = 20;
const PROJECT_PAGE_SIZE: &str = "500";
const RECENT_PULL_REQUESTS: &str = "100";
const MAX_PEOPLE: usize = 20;
const MAX_QUERY: usize = 100;
const MAX_TEXT: usize = 256;

mod diff;
mod pulls;
mod readiness;
mod session;
mod threads;
pub use pulls::Iteration;
pub use session::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

/// One authenticated request; POST and PATCH carry a JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub body: Option<Value>,
}

impl Request {
    pub fn get(url: String) -> Self {
        Self {
            method: Method::Get,
            url,
            body: None,
        }
    }

    pub fn post(url: String, body: Value) -> Self {
        Self {
            method: Method::Post,
            url,
            body: Some(body),
        }
    }

    pub fn patch(url: String, body: Value) -> Self {
        Self {
            method: Method::Patch,
            url,
            body: Some(body),
        }
    }

    pub fn delete(url: String) -> Self {
        Self {
            method: Method::Delete,
            url,
            body: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub retry_after: Option<String>,
    pub continuation: Option<String>,
    pub body: Vec<u8>,
    /// The body exceeded the read limit and was dropped.
    pub oversize: bool,
}

#[async_trait]
pub trait Transport: Send + Sync {
    /// Sends `X-TFS-FedAuthRedirect: Suppress` and never follows redirects.
    async fn send(&self, request: &Request, token: &AccessToken) -> Result<Response, NetworkError>;
}

pub struct HttpTransport(reqwest::Client);

impl HttpTransport {
    pub fn new() -> Result<Self, NetworkError> {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .map(Self)
            .map_err(|_| NetworkError)
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn send(&self, request: &Request, token: &AccessToken) -> Result<Response, NetworkError> {
        let builder = match request.method {
            Method::Get => self.0.get(&request.url),
            Method::Post => self.0.post(&request.url),
            Method::Patch => self.0.patch(&request.url),
            Method::Delete => self.0.delete(&request.url),
        };
        let builder = match &request.body {
            Some(body) => builder
                .header("Content-Type", "application/json")
                .body(serde_json::to_vec(body).map_err(|_| NetworkError)?),
            None => builder,
        };
        let response = builder
            .bearer_auth(token.secret())
            .header("Accept", "application/json")
            .header("X-TFS-FedAuthRedirect", "Suppress")
            .send()
            .await
            .map_err(|_| NetworkError)?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let status = response.status().as_u16();
        let retry_after = header("retry-after");
        let continuation = header("x-ms-continuationtoken");
        let body = response.bytes().await.map_err(|_| NetworkError)?;
        let oversize = body.len() > BODY_LIMIT;
        Ok(Response {
            status,
            retry_after,
            continuation,
            body: if oversize { Vec::new() } else { body.to_vec() },
            oversize,
        })
    }
}

/// Stable repository identity: organization, project ID and repository ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryKey {
    pub organization: String,
    pub project_id: TenantId,
    pub repository_id: TenantId,
}

impl RepositoryKey {
    /// `{organization}/{project ID}/{repository ID}`, lowercase.
    pub fn id(&self) -> String {
        format!(
            "{}/{}/{}",
            self.organization.to_ascii_lowercase(),
            self.project_id.as_str(),
            self.repository_id.as_str()
        )
    }

    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('/');
        let (organization, project, repository) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || !super::tenant::valid_organization(organization) {
            return None;
        }
        let key = Self {
            organization: organization.to_ascii_lowercase(),
            project_id: TenantId::parse(project)?,
            repository_id: TenantId::parse(repository)?,
        };
        (key.id() == value).then_some(key)
    }
}

/// A repository as Azure DevOps returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub key: RepositoryKey,
    pub project: String,
    pub name: String,
}

impl Repository {
    pub fn remote(&self) -> RemoteRepository {
        RemoteRepository {
            id: self.key.id(),
            name: format!("{}/{}/{}", self.key.organization, self.project, self.name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: TenantId,
    pub name: String,
}

/// Where a pasted repository lives: organization, project and repository
/// names (or IDs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryPath {
    pub organization: String,
    pub project: String,
    pub repository: String,
}

/// Parses a pasted `https://dev.azure.com/{org}/{project}/_git/{repo}` or
/// `https://{org}.visualstudio.com/[DefaultCollection/]{project}/_git/{repo}`
/// URL (any trailing path, such as a pull request, is ignored), or
/// `{org}/{project}/{repo}`.
pub fn parse_repository_url(input: &str) -> Option<RepositoryPath> {
    let input = input.trim();
    if input.is_empty() || input.len() > 2048 || input.chars().any(char::is_control) {
        return None;
    }
    if !input.contains("://") {
        let parts: Vec<_> = input.split('/').collect();
        return match parts.as_slice() {
            [organization, project, repository] => path(organization, project, repository),
            _ => None,
        };
    }
    let url = url::Url::parse(input).ok()?;
    if url.scheme() != "https" || url.password().is_some() || url.port().is_some() {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let mut segments: Vec<String> = url
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .map(percent_decode)
        .collect::<Option<_>>()?;
    let organization = if host == "dev.azure.com" {
        if segments.is_empty() {
            return None;
        }
        segments.remove(0)
    } else {
        let organization = host.strip_suffix(".visualstudio.com")?;
        if segments
            .first()
            .is_some_and(|segment| segment.eq_ignore_ascii_case("DefaultCollection"))
        {
            segments.remove(0);
        }
        organization.to_owned()
    };
    let git = segments.iter().position(|segment| segment == "_git")?;
    let repository = segments.get(git + 1)?;
    match git {
        // `{org}/_git/{repo}`: the project shares the repository's name.
        0 => path(&organization, repository, repository),
        1 => path(&organization, &segments[0], repository),
        _ => None,
    }
}

fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn path(organization: &str, project: &str, repository: &str) -> Option<RepositoryPath> {
    let name = |value: &str| {
        !value.is_empty()
            && value.len() <= MAX_TEXT
            && !value.starts_with('_')
            && !value
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
    };
    (super::tenant::valid_organization(organization) && name(project) && name(repository)).then(
        || RepositoryPath {
            organization: organization.to_ascii_lowercase(),
            project: project.to_owned(),
            repository: repository.to_owned(),
        },
    )
}

fn token_error(error: TokenError) -> ConnectionError {
    match error {
        TokenError::AzMissing | TokenError::AzTooOld => ConnectionError::MissingCli,
        TokenError::NotLoggedIn
        | TokenError::NoAccountInTenant
        | TokenError::TenantNotFound
        | TokenError::InteractionRequired { .. } => ConnectionError::SignedOut,
        // Signing in again cannot lift a cross-tenant access block.
        TokenError::TenantAccessBlocked => ConnectionError::OrganizationPolicyDenied,
        TokenError::Network => ConnectionError::Network,
        TokenError::Timeout => ConnectionError::Timeout,
        TokenError::MalformedOutput | TokenError::Other { .. } => ConnectionError::BrokenCli,
    }
}

/// A refused or failed response, with any stated wait for the shared retry
/// policy.
fn status_error(response: &Response) -> ConnectionError {
    let wait = response
        .retry_after
        .as_deref()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|seconds| *seconds >= 0)
        .map(|seconds| seconds.min(crate::retry::PROVIDER_CAP_SECONDS));
    match response.status {
        403 => ConnectionError::MissingReadPermission,
        404 => ConnectionError::RepositoryUnavailable,
        429 => wait.map_or(
            ConnectionError::RateLimited,
            ConnectionError::RateLimitedAfter,
        ),
        500..=599 => wait.map_or(
            ConnectionError::ProviderFailure,
            ConnectionError::ProviderFailureAfter,
        ),
        status => ConnectionError::ProviderRejectedStatus(status),
    }
}

fn bounded(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_owned()).filter(|text| {
        !text.is_empty() && text.len() <= MAX_TEXT && !text.chars().any(char::is_control)
    })
}

#[derive(Deserialize)]
struct List<T> {
    value: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProject {
    id: String,
    name: String,
    state: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRepository {
    id: String,
    name: String,
    project: RawProject,
    #[serde(default)]
    is_disabled: bool,
}

/// Azure DevOps REST reads for one organization with the user's Azure CLI
/// tokens for its tenant.
pub struct Client {
    source: Arc<dyn TokenSource>,
    transport: Arc<dyn Transport>,
    tenant: TenantId,
    organization: String,
    /// Checked before and after every request of a guarded session.
    guard: Option<crate::provider::ReadGuard>,
}

impl Client {
    pub fn new(
        source: Arc<dyn TokenSource>,
        transport: Arc<dyn Transport>,
        tenant: TenantId,
        organization: &str,
    ) -> Self {
        Self {
            source,
            transport,
            tenant,
            organization: organization.to_ascii_lowercase(),
            guard: None,
        }
    }

    pub fn organization(&self) -> &str {
        &self.organization
    }

    fn url(&self, segments: &[&str], query: &[(&str, &str)]) -> String {
        let mut url = url::Url::parse("https://dev.azure.com/").expect("static URL");
        url.path_segments_mut()
            .expect("base URL")
            .clear()
            .push(&self.organization)
            .extend(segments);
        url.query_pairs_mut().extend_pairs(query);
        url.into()
    }

    async fn call(&self, request: Request) -> Result<Response, ConnectionError> {
        if let Some(guard) = &self.guard {
            guard()?;
        }
        let response = self.send(request).await?;
        if let Some(guard) = &self.guard {
            guard()?;
        }
        Ok(response)
    }

    async fn send(&self, request: Request) -> Result<Response, ConnectionError> {
        let (request, transport) = (&request, &self.transport);
        with_token(
            self.source.as_ref(),
            &self.tenant,
            move |token| async move {
                let response = transport
                    .send(request, &token)
                    .await
                    .map_err(|_| Rejection::Failed(ConnectionError::Network))?;
                match response.status {
                    200 | 201 | 204 => Ok(response),
                    // A suppressed FedAuth redirect is a 401; a 203 sign-in page or
                    // a redirect is the same refusal.
                    401 | 203 | 300..=399 => Err(Rejection::Unauthorized),
                    _ => Err(Rejection::Failed(status_error(&response))),
                }
            },
        )
        .await
        .map_err(|error| match error {
            AuthorizedError::Token(error) => token_error(error),
            AuthorizedError::Unauthorized => ConnectionError::SignedOut,
            AuthorizedError::Failed(error) => error,
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        url: String,
    ) -> Result<(T, Response), ConnectionError> {
        let response = self.call(Request::get(url)).await?;
        let value =
            serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
        Ok((value, response))
    }

    /// The identity the tenant token belongs to.
    pub async fn identity(&self) -> Result<Identity, ConnectionError> {
        let url = self.url(&["_apis", "connectionData"], &[]);
        let response = self.call(Request::get(url)).await?;
        connection::parse_identity(&response.body)
            .map_err(|_| ConnectionError::InvalidResponse)?
            .ok_or(ConnectionError::SignedOut)
    }

    /// Well-formed projects, following continuation tokens. A listing cut off
    /// at the page bound carries a pagination warning.
    pub async fn projects(
        &self,
    ) -> Result<(Vec<Project>, Vec<RepositoryBrowseWarning>), ConnectionError> {
        let mut projects = Vec::new();
        let mut continuation: Option<String> = None;
        for page in 1..=PROJECT_PAGES {
            let mut query = vec![("api-version", API_VERSION), ("$top", PROJECT_PAGE_SIZE)];
            if let Some(token) = continuation.as_deref() {
                query.push(("continuationToken", token));
            }
            let (list, response): (List<RawProject>, _) =
                self.get(self.url(&["_apis", "projects"], &query)).await?;
            projects.extend(list.value.into_iter().filter_map(|raw| {
                let well_formed = raw
                    .state
                    .as_deref()
                    .is_none_or(|state| state.eq_ignore_ascii_case("wellFormed"));
                if !well_formed {
                    return None;
                }
                Some(Project {
                    id: TenantId::parse(&raw.id)?,
                    name: bounded(Some(raw.name))?,
                })
            }));
            continuation = response.continuation.filter(|token| !token.is_empty());
            if continuation.is_none() {
                projects.sort_by_key(|project| project.name.to_lowercase());
                return Ok((projects, Vec::new()));
            }
            if page == PROJECT_PAGES {
                projects.sort_by_key(|project| project.name.to_lowercase());
                return Ok((
                    projects,
                    vec![RepositoryBrowseWarning {
                        boundary: "pagination",
                        page,
                        error: ConnectionError::IncompleteRead,
                    }],
                ));
            }
        }
        unreachable!("the last page returns")
    }

    fn repository(&self, raw: RawRepository) -> Option<Repository> {
        if raw.is_disabled {
            return None;
        }
        Some(Repository {
            key: RepositoryKey {
                organization: self.organization.clone(),
                project_id: TenantId::parse(&raw.project.id)?,
                repository_id: TenantId::parse(&raw.id)?,
            },
            project: bounded(Some(raw.project.name))?,
            name: bounded(Some(raw.name))?,
        })
    }

    /// Enabled repositories of one project (name or ID).
    pub async fn repositories(&self, project: &str) -> Result<Vec<Repository>, ConnectionError> {
        let url = self.url(
            &[project, "_apis", "git", "repositories"],
            &[("api-version", API_VERSION)],
        );
        let (list, _): (List<RawRepository>, _) = self.get(url).await?;
        let mut repositories: Vec<_> = list
            .value
            .into_iter()
            .filter_map(|raw| self.repository(raw))
            .collect();
        repositories.sort_by_key(|repository| repository.name.to_lowercase());
        Ok(repositories)
    }

    /// One repository by project and repository (names or IDs).
    pub async fn repository_at(
        &self,
        project: &str,
        repository: &str,
    ) -> Result<Repository, ConnectionError> {
        let url = self.url(
            &[project, "_apis", "git", "repositories", repository],
            &[("api-version", API_VERSION)],
        );
        let (raw, _): (RawRepository, _) = self.get(url).await?;
        self.repository(raw)
            .ok_or(ConnectionError::RepositoryUnavailable)
    }

    /// Resolves a pasted URL, `{org}/{project}/{repo}` path or stable
    /// repository ID within this organization.
    pub async fn resolve(&self, input: &str) -> Result<Repository, ConnectionError> {
        let input = input.trim();
        if let Some(key) = RepositoryKey::parse(input) {
            if key.organization != self.organization {
                return Err(ConnectionError::RepositoryUnavailable);
            }
            let repository = self
                .repository_at(key.project_id.as_str(), key.repository_id.as_str())
                .await?;
            return if repository.key == key {
                Ok(repository)
            } else {
                Err(ConnectionError::RepositoryChanged)
            };
        }
        let path = parse_repository_url(input).ok_or(ConnectionError::InvalidRepository)?;
        if path.organization != self.organization {
            return Err(ConnectionError::RepositoryUnavailable);
        }
        self.repository_at(&path.project, &path.repository).await
    }

    /// IdentityPicker prefix search over users.
    pub async fn directory_people(&self, query: &str) -> Result<Vec<PersonMatch>, ConnectionError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawIdentity {
            entity_type: Option<String>,
            local_id: Option<String>,
            display_name: Option<String>,
            mail: Option<String>,
            sign_in_address: Option<String>,
        }
        #[derive(Deserialize)]
        struct RawResult {
            #[serde(default)]
            identities: Vec<RawIdentity>,
        }
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            results: Vec<RawResult>,
        }
        let url = self.url(
            &["_apis", "IdentityPicker", "Identities"],
            &[("api-version", PICKER_API_VERSION)],
        );
        let response = self
            .call(Request::post(
                url,
                json!({
                    "query": query,
                    "identityTypes": ["user"],
                    "operationScopes": ["ims", "source"],
                    "properties": ["DisplayName", "Mail"],
                    "options": {"MinResults": 5, "MaxResults": MAX_PEOPLE},
                }),
            ))
            .await?;
        let raw: Raw =
            serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
        let mut people = Vec::new();
        for identity in raw.results.into_iter().flat_map(|result| result.identities) {
            if !identity
                .entity_type
                .as_deref()
                .is_none_or(|kind| kind.eq_ignore_ascii_case("user"))
            {
                continue;
            }
            let (Some(id), Some(login)) = (
                identity.local_id.as_deref().and_then(TenantId::parse),
                bounded(identity.display_name),
            ) else {
                continue;
            };
            push_person(
                &mut people,
                PersonMatch {
                    id: id.as_str().to_owned(),
                    login,
                    mail: bounded(identity.mail).or(bounded(identity.sign_in_address)),
                },
            );
        }
        Ok(people)
    }

    /// Authors and reviewers of the repository's recent pull requests whose
    /// name or address starts with `query`, most recent first.
    pub async fn recent_people(
        &self,
        key: &RepositoryKey,
        query: &str,
    ) -> Result<Vec<PersonMatch>, ConnectionError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawIdentity {
            id: Option<String>,
            display_name: Option<String>,
            unique_name: Option<String>,
            #[serde(default)]
            is_container: bool,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawPullRequest {
            created_by: Option<RawIdentity>,
            #[serde(default)]
            reviewers: Vec<RawIdentity>,
        }
        if key.organization != self.organization {
            return Err(ConnectionError::RepositoryUnavailable);
        }
        let url = self.url(
            &[
                key.project_id.as_str(),
                "_apis",
                "git",
                "repositories",
                key.repository_id.as_str(),
                "pullrequests",
            ],
            &[
                ("api-version", API_VERSION),
                ("searchCriteria.status", "all"),
                ("$top", RECENT_PULL_REQUESTS),
            ],
        );
        let (list, _): (List<RawPullRequest>, _) = self.get(url).await?;
        let query = query.to_lowercase();
        let mut people = Vec::new();
        for identity in list
            .value
            .into_iter()
            .flat_map(|pull| pull.created_by.into_iter().chain(pull.reviewers))
        {
            if identity.is_container || people.len() == MAX_PEOPLE {
                continue;
            }
            let (Some(id), Some(login)) = (
                identity.id.as_deref().and_then(TenantId::parse),
                bounded(identity.display_name),
            ) else {
                continue;
            };
            let mail = bounded(identity.unique_name);
            let matches = std::iter::once(login.as_str())
                .chain(login.split_whitespace())
                .chain(mail.as_deref())
                .any(|text| text.to_lowercase().starts_with(&query));
            if matches {
                push_person(
                    &mut people,
                    PersonMatch {
                        id: id.as_str().to_owned(),
                        login,
                        mail,
                    },
                );
            }
        }
        Ok(people)
    }

    /// Directory search, falling back to recent pull request people of
    /// `repository` when the directory fails.
    pub async fn people(
        &self,
        query: &str,
        repository: Option<&RepositoryKey>,
    ) -> Result<People, ConnectionError> {
        let query = query.trim();
        if query.chars().count() > MAX_QUERY || query.chars().any(char::is_control) {
            return Err(ConnectionError::InvalidResponse);
        }
        if query.is_empty() {
            return Ok(People {
                source: PeopleSource::Directory,
                people: Vec::new(),
            });
        }
        match self.directory_people(query).await {
            Ok(people) => Ok(People {
                source: PeopleSource::Directory,
                people,
            }),
            // Without a working token the fallback cannot read either.
            Err(
                error @ (ConnectionError::SignedOut
                | ConnectionError::MissingCli
                | ConnectionError::BrokenCli),
            ) => Err(error),
            Err(error) => match repository {
                Some(key) => Ok(People {
                    source: PeopleSource::Recent,
                    people: self.recent_people(key, query).await?,
                }),
                None => Err(error),
            },
        }
    }
}

fn push_person(people: &mut Vec<PersonMatch>, person: PersonMatch) {
    if people.len() < MAX_PEOPLE && !people.iter().any(|known| known.id == person.id) {
        people.push(person);
    }
}

fn person(identity: &Identity) -> Person {
    Person {
        id: identity.id.clone(),
        login: identity.display_name.clone(),
    }
}

/// Synchronous provider read interfaces over [`Client`] for one bound
/// identity. Call only off the async runtime (blocking threads, tests).
pub struct Reader {
    client: Client,
    runtime: tokio::runtime::Handle,
    identity: Person,
}

impl Reader {
    /// Verifies that the signed-in Azure CLI identity is the bound one.
    pub fn open(
        client: Client,
        runtime: tokio::runtime::Handle,
        bound: &Identity,
    ) -> Result<Self, ConnectionError> {
        let identity = runtime.block_on(client.identity())?;
        if identity.id != bound.id {
            return Err(ConnectionError::WrongIdentity);
        }
        Ok(Self {
            client,
            runtime,
            identity: person(&identity),
        })
    }

    pub fn identity(&self) -> &Person {
        &self.identity
    }

    fn projects(
        &self,
    ) -> Result<(Vec<RepositoryOwner>, Vec<RepositoryBrowseWarning>), ConnectionError> {
        let (projects, warnings) = self.runtime.block_on(self.client.projects())?;
        Ok((
            projects
                .into_iter()
                .map(|project| RepositoryOwner {
                    login: project.name,
                    kind: "project",
                })
                .collect(),
            warnings,
        ))
    }

    fn connection(&self, repository: &Repository) -> Connection {
        Connection {
            identity: self.identity.clone(),
            repository: repository.remote(),
            capabilities: Capabilities {
                read: true,
                comment: CommentCapability::Unknown,
            },
        }
    }
}

impl AccountReader for Reader {
    fn current_identity(&self) -> Result<Person, ConnectionError> {
        Ok(person(&self.runtime.block_on(self.client.identity())?))
    }

    /// The one directory person whose display name or address is exactly
    /// `login`.
    fn resolve_person(&self, login: &str) -> Result<Person, ConnectionError> {
        let login = login.trim();
        let found = self
            .runtime
            .block_on(self.client.people(login, None))?
            .people
            .into_iter()
            .filter(|person| {
                person.login.eq_ignore_ascii_case(login)
                    || person
                        .mail
                        .as_deref()
                        .is_some_and(|mail| mail.eq_ignore_ascii_case(login))
            })
            .collect::<Vec<_>>();
        match found.as_slice() {
            [person] => Ok(Person {
                id: person.id.clone(),
                login: person.login.clone(),
            }),
            _ => Err(ConnectionError::ProviderRejectedStatus(404)),
        }
    }
}

impl RepositoryReader for Reader {
    fn accessible_repositories(&self) -> Result<Vec<RemoteRepository>, ConnectionError> {
        let (projects, warnings) = self.runtime.block_on(self.client.projects())?;
        if let Some(warning) = warnings.first() {
            return Err(warning.error);
        }
        let mut repositories = Vec::new();
        for project in projects {
            repositories.extend(
                self.runtime
                    .block_on(self.client.repositories(project.id.as_str()))?
                    .iter()
                    .map(Repository::remote),
            );
        }
        Ok(repositories)
    }

    /// Projects of the organization; repositories come per project.
    fn repository_browser(&self, _identity: &Person) -> Result<RepositoryBrowser, ConnectionError> {
        let (owners, warnings) = self.projects()?;
        Ok(RepositoryBrowser {
            owners,
            repositories: Vec::new(),
            warnings,
        })
    }

    /// Repositories of one project (`owner`, by name or ID).
    fn owner_repository_browser(
        &self,
        _identity: &Person,
        owner: &str,
    ) -> Result<RepositoryBrowser, ConnectionError> {
        let repositories = self.runtime.block_on(self.client.repositories(owner))?;
        Ok(RepositoryBrowser {
            owners: Vec::new(),
            repositories: repositories.iter().map(Repository::remote).collect(),
            warnings: Vec::new(),
        })
    }

    fn connect(
        &self,
        repository: &str,
        expected_account_id: Option<&str>,
    ) -> Result<Connection, ConnectionError> {
        if expected_account_id.is_some_and(|id| id != self.identity.id) {
            return Err(ConnectionError::WrongIdentity);
        }
        let repository = self.runtime.block_on(self.client.resolve(repository))?;
        Ok(self.connection(&repository))
    }

    /// A pull request URL resolves its repository only.
    fn resolve_repository_target(
        &self,
        input: &str,
        account_id: &str,
    ) -> Result<ResolvedTarget, ConnectionError> {
        Ok(ResolvedTarget {
            connection: self.connect(input, Some(account_id))?,
            pull_request: None,
        })
    }
}

impl PeopleSearch for Reader {
    fn search_people(
        &self,
        query: &str,
        repository: Option<&RemoteRepository>,
    ) -> Result<People, ConnectionError> {
        let key = repository
            .map(|repository| {
                RepositoryKey::parse(&repository.id).ok_or(ConnectionError::InvalidRepository)
            })
            .transpose()?;
        self.runtime
            .block_on(self.client.people(query, key.as_ref()))
    }
}

#[cfg(test)]
mod tests;

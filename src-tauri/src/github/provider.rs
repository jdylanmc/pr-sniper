use super::{verify_identity, ConnectionError, Identity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub trait Transport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError>;
}

pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connection {
    pub identity: Identity,
    pub repository: RemoteRepository,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteRepository {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryOwner {
    pub login: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryBrowseWarning {
    pub boundary: &'static str,
    pub page: u64,
    pub error: ConnectionError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryBrowser {
    pub owners: Vec<RepositoryOwner>,
    pub repositories: Vec<RemoteRepository>,
    pub warnings: Vec<RepositoryBrowseWarning>,
}

#[derive(Deserialize)]
struct RepositoryMetadata {
    id: u64,
    full_name: String,
    #[serde(rename = "private")]
    _private: bool,
    owner: Option<OwnerMetadata>,
}

#[derive(Deserialize)]
struct OwnerMetadata {
    login: String,
    #[serde(rename = "type")]
    kind: OwnerKind,
}

#[derive(Deserialize)]
enum OwnerKind {
    User,
    Organization,
}

type RepositoryCatalog = (
    Vec<(RemoteRepository, Option<RepositoryOwner>)>,
    Vec<RepositoryBrowseWarning>,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub read: bool,
    pub comment: CommentCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentCapability {
    Available,
    Unavailable,
    Unknown,
}

pub struct GithubClient<T: Transport> {
    pub(super) transport: T,
}

impl<T: Transport> GithubClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn resolve_person(&self, login: &str) -> Result<Identity, ConnectionError> {
        if login.is_empty()
            || login.len() > 39
            || login.starts_with('-')
            || login.ends_with('-')
            || !login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(ConnectionError::InvalidResponse);
        }

        let (user, _) = self.read(&format!("/users/{login}"))?;
        verify_identity(&user, None)
    }

    pub fn current_identity(&self) -> Result<Identity, ConnectionError> {
        let (user, _) = self.read("/user")?;
        verify_identity(&user, None)
    }

    pub fn accessible_repositories(&self) -> Result<Vec<RemoteRepository>, ConnectionError> {
        let (entries, warnings) = self.repository_catalog(false)?;
        if let Some(warning) = warnings.first() {
            return Err(warning.error);
        }
        Ok(entries.into_iter().map(|(repo, _)| repo).collect())
    }

    pub fn repository_owners(
        &self,
        identity: &Identity,
    ) -> Result<Vec<RepositoryOwner>, ConnectionError> {
        let browser = self.repository_browser(identity)?;
        if let Some(warning) = browser.warnings.first() {
            return Err(warning.error);
        }
        Ok(browser.owners)
    }

    pub fn repository_browser(
        &self,
        identity: &Identity,
    ) -> Result<RepositoryBrowser, ConnectionError> {
        let (entries, warnings) = self.repository_catalog(true)?;
        let mut owners = BTreeMap::from([(identity.login.to_ascii_lowercase(), "personal")]);
        let mut repositories = Vec::new();
        for (repository, owner) in entries {
            if let Some(owner) = owner {
                if owner.kind == "organization" {
                    owners.insert(owner.login, "organization");
                }
            }
            repositories.push(repository);
        }
        Ok(RepositoryBrowser {
            owners: owners
                .into_iter()
                .map(|(login, kind)| RepositoryOwner { login, kind })
                .collect(),
            repositories,
            warnings,
        })
    }

    pub fn owner_repositories(
        &self,
        owner: &str,
    ) -> Result<Vec<RemoteRepository>, ConnectionError> {
        if owner.is_empty()
            || owner.len() > 39
            || !owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(ConnectionError::InvalidRepository);
        }
        Ok(self
            .accessible_repositories()?
            .into_iter()
            .filter(|r| {
                r.name
                    .split('/')
                    .next()
                    .is_some_and(|login| login.eq_ignore_ascii_case(owner))
            })
            .collect())
    }

    pub fn owner_repository_browser(
        &self,
        identity: &Identity,
        owner: &str,
    ) -> Result<RepositoryBrowser, ConnectionError> {
        crate::storage::canonical_repository(&format!("{owner}/repository"))
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let mut browser = self.repository_browser(identity)?;
        browser.repositories.retain(|repository| {
            repository
                .name
                .split('/')
                .next()
                .is_some_and(|login| login.eq_ignore_ascii_case(owner))
        });
        Ok(browser)
    }

    fn repository_catalog(
        &self,
        require_owner: bool,
    ) -> Result<RepositoryCatalog, ConnectionError> {
        let mut result = Vec::new();
        let mut warnings = Vec::new();
        let mut repository_ids = std::collections::HashSet::new();
        let mut advertised_last = None;
        let first = "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1";
        let mut path = first.to_string();
        let mut explicit_next = false;
        for page in 1..=10_000 {
            let page_read = self.transport.get(&path).and_then(parse_response);
            let (value, response) = match page_read {
                Ok(read) => read,
                Err(error)
                    if result.is_empty()
                        || matches!(
                            error,
                            ConnectionError::SignedOut
                                | ConnectionError::WrongIdentity
                                | ConnectionError::MissingScope
                        ) =>
                {
                    return Err(error)
                }
                Err(error) => {
                    warnings.push(RepositoryBrowseWarning {
                        boundary: "repository_page",
                        page,
                        error,
                    });
                    return Ok((result, warnings));
                }
            };
            if let Err(error) = require_repo_scope(&response) {
                if result.is_empty() || error == ConnectionError::MissingScope {
                    return Err(error);
                }
                warnings.push(RepositoryBrowseWarning {
                    boundary: "repository_page",
                    page,
                    error,
                });
                return Ok((result, warnings));
            }
            if response
                .headers
                .get("x-github-sso")
                .is_some_and(|value| value.starts_with("partial-results;"))
            {
                warnings.push(RepositoryBrowseWarning {
                    boundary: "organization_access",
                    page,
                    error: ConnectionError::OrganizationPolicyDenied,
                });
            }
            let repositories = match value.as_array().filter(|array| array.len() <= 100) {
                Some(repositories) => repositories,
                None => {
                    if result.is_empty() {
                        return Err(ConnectionError::InvalidResponse);
                    }
                    warnings.push(RepositoryBrowseWarning {
                        boundary: "repository_page",
                        page,
                        error: ConnectionError::InvalidResponse,
                    });
                    return Ok((result, warnings));
                }
            };
            for value in repositories {
                match Self::repository_metadata(value, require_owner) {
                    Ok((repository, owner)) => {
                        if repository_ids.insert(repository.id.clone()) {
                            result.push((repository, owner));
                        } else {
                            warnings.push(RepositoryBrowseWarning {
                                boundary: "pagination",
                                page,
                                error: ConnectionError::IncompleteRead,
                            });
                            return Ok((result, warnings));
                        }
                    }
                    Err(error) => {
                        warnings.push(RepositoryBrowseWarning {
                            boundary: "repository_metadata",
                            page,
                            error,
                        });
                    }
                }
            }
            let next = match super::metadata::next_page(&path, &response, &mut advertised_last) {
                Ok(next) => next,
                Err(error) => {
                    warnings.push(RepositoryBrowseWarning {
                        boundary: "pagination",
                        page,
                        error,
                    });
                    return Ok((result, warnings));
                }
            };
            if repositories.is_empty() && (explicit_next || next.is_some()) {
                warnings.push(RepositoryBrowseWarning {
                    boundary: "pagination",
                    page,
                    error: ConnectionError::IncompleteRead,
                });
                return Ok((result, warnings));
            }
            explicit_next = next.is_some();
            if let Some(next) = next {
                path = next;
            } else if repositories.len() == 100
                && !response.headers.contains_key("link")
                && advertised_last.is_none()
            {
                path = format!("{}{}", first.trim_end_matches('1'), page + 1);
            } else {
                return Ok((result, warnings));
            }
        }
        warnings.push(RepositoryBrowseWarning {
            boundary: "pagination",
            page: 10_000,
            error: ConnectionError::IncompleteRead,
        });
        Ok((result, warnings))
    }

    fn repository_metadata(
        value: &Value,
        require_owner: bool,
    ) -> Result<(RemoteRepository, Option<RepositoryOwner>), ConnectionError> {
        let metadata: RepositoryMetadata =
            serde_json::from_value(value.clone()).map_err(|_| ConnectionError::InvalidResponse)?;
        if metadata.id == 0 {
            return Err(ConnectionError::InvalidResponse);
        }
        let name = crate::storage::canonical_repository(&metadata.full_name)
            .map_err(|_| ConnectionError::InvalidResponse)?;
        let owner = metadata.owner.map(|owner| RepositoryOwner {
            login: owner.login.to_ascii_lowercase(),
            kind: match owner.kind {
                OwnerKind::User => "personal",
                OwnerKind::Organization => "organization",
            },
        });
        if require_owner && owner.is_none()
            || owner
                .as_ref()
                .is_some_and(|owner| name.split('/').next() != Some(owner.login.as_str()))
        {
            return Err(ConnectionError::InvalidResponse);
        }
        Ok((
            RemoteRepository {
                id: metadata.id.to_string(),
                name,
            },
            owner,
        ))
    }

    pub fn connect(
        &self,
        repository: &str,
        expected_account_id: Option<&str>,
    ) -> Result<Connection, ConnectionError> {
        let name = super::intake::repository_target(repository)?.name;
        let (user, _) = self.read("/user")?;
        let identity = verify_identity(&user, expected_account_id)?;
        let (repo, response) = self.read_repository(&format!("/repos/{name}"))?;
        let id = decimal_id(&repo["id"])?;
        let remote_name = repo["full_name"]
            .as_str()
            .ok_or(ConnectionError::InvalidResponse)?;
        if remote_name.to_ascii_lowercase() != name {
            return Err(ConnectionError::RepositoryChanged);
        }
        let private = boolean(&repo, "private")?;
        require_repo_scope(&response)?;
        let archived = boolean(&repo, "archived")?;
        let disabled = boolean(&repo, "disabled")?;
        if repo["permissions"]["pull"].as_bool() == Some(false) || disabled {
            return Err(ConnectionError::MissingReadPermission);
        }
        let (pulls, _) =
            self.read_repository(&format!("/repos/{name}/pulls?state=open&per_page=1"))?;
        if !pulls.is_array() {
            return Err(ConnectionError::InvalidResponse);
        }
        let comment = if archived {
            CommentCapability::Unavailable
        } else if let Some(scopes) = response.headers.get("x-oauth-scopes") {
            if scopes
                .split(',')
                .map(str::trim)
                .any(|scope| scope == "repo" || (!private && scope == "public_repo"))
            {
                CommentCapability::Available
            } else {
                CommentCapability::Unavailable
            }
        } else {
            CommentCapability::Unknown
        };
        Ok(Connection {
            identity,
            repository: RemoteRepository { id, name },
            capabilities: Capabilities {
                read: true,
                comment,
            },
        })
    }

    pub(super) fn read(&self, path: &str) -> Result<(Value, Response), ConnectionError> {
        parse_response(self.transport.get(path)?)
    }

    fn read_repository(&self, path: &str) -> Result<(Value, Response), ConnectionError> {
        let response = self.transport.get(path)?;
        // GitHub hides unauthorized private resources behind the same 404 as
        // absent repositories. Neither absence nor a definite denial is proven.
        if response.status == 404 {
            return Err(ConnectionError::RepositoryUnavailable);
        }
        let missing_scope =
            response.headers.contains_key("x-oauth-scopes") && !has_scope(&response, "repo");
        let organization_policy = oauth_app_restricted(&response);
        match parse_response(response) {
            Err(ConnectionError::RateLimitedWithContext {
                retry_after_seconds,
                reset_at,
                organization_access_incomplete,
                ..
            }) if missing_scope => Err(ConnectionError::RateLimitedWithContext {
                retry_after_seconds,
                reset_at,
                organization_access_incomplete,
                missing_repo_scope: true,
            }),
            Err(ConnectionError::OrganizationPolicyDenied) if missing_scope => {
                Err(ConnectionError::OrganizationPolicyDeniedWithMissingScope)
            }
            Err(ConnectionError::MissingReadPermission) if organization_policy => {
                Err(if missing_scope {
                    ConnectionError::OrganizationPolicyDeniedWithMissingScope
                } else {
                    ConnectionError::OrganizationPolicyDenied
                })
            }
            Err(ConnectionError::MissingReadPermission) if missing_scope => {
                Err(ConnectionError::MissingScope)
            }
            result => result,
        }
    }
}

pub(super) fn parse_response(response: Response) -> Result<(Value, Response), ConnectionError> {
    let retry_after_seconds = response
        .headers
        .get("retry-after")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 0);
    let rate_limit_message = response.status == 403
        && serde_json::from_slice::<Value>(&response.body)
            .ok()
            .and_then(|body| body["message"].as_str().map(str::to_ascii_lowercase))
            .is_some_and(|message| {
                message.contains("secondary rate limit")
                    || message.contains("api rate limit exceeded")
            });
    match response.status {
        200 => (),
        401 => return Err(ConnectionError::SignedOut),
        429 => return Err(rate_limited(&response, retry_after_seconds)),
        403 if response
            .headers
            .get("x-ratelimit-remaining")
            .is_some_and(|v| v == "0")
            || retry_after_seconds.is_some()
            || rate_limit_message =>
        {
            return Err(rate_limited(&response, retry_after_seconds))
        }
        403 if sso_marker(&response) == Some("required") => {
            return Err(ConnectionError::OrganizationPolicyDenied)
        }

        403 | 404 => return Err(ConnectionError::MissingReadPermission),
        500..=599 => {
            return Err(retry_after_seconds.map_or(
                ConnectionError::ProviderFailure,
                ConnectionError::ProviderFailureAfter,
            ))
        }
        status => return Err(ConnectionError::ProviderRejectedStatus(status)),
    }
    let value =
        serde_json::from_slice(&response.body).map_err(|_| ConnectionError::InvalidResponse)?;
    Ok((value, response))
}

fn rate_limited(response: &Response, retry_after_seconds: Option<i64>) -> ConnectionError {
    let reset_at = response
        .headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 0);
    let organization_access_incomplete =
        matches!(sso_marker(response), Some("partial-results" | "required"))
            || oauth_app_restricted(response);
    if reset_at.is_some() || organization_access_incomplete {
        return ConnectionError::RateLimitedWithContext {
            retry_after_seconds,
            reset_at,
            organization_access_incomplete,
            missing_repo_scope: false,
        };
    }
    retry_after_seconds.map_or(
        ConnectionError::RateLimited,
        ConnectionError::RateLimitedAfter,
    )
}

fn sso_marker(response: &Response) -> Option<&str> {
    response
        .headers
        .get("x-github-sso")?
        .split(';')
        .next()
        .map(str::trim)
}

fn oauth_app_restricted(response: &Response) -> bool {
    response.status == 403
        && serde_json::from_slice::<Value>(&response.body)
            .ok()
            .and_then(|body| body["message"].as_str().map(str::to_owned))
            .is_some_and(|message| {
                message.starts_with("Although you appear to have the correct authorization credentials, the ")
                    && message.contains(" organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited.")
            })
}

fn require_repo_scope(response: &Response) -> Result<(), ConnectionError> {
    if !response.headers.contains_key("x-oauth-scopes") {
        return Err(ConnectionError::ScopeUnverified);
    }
    if !has_scope(response, "repo") {
        return Err(ConnectionError::MissingScope);
    }
    Ok(())
}

fn has_scope(response: &Response, expected: &str) -> bool {
    response
        .headers
        .get("x-oauth-scopes")
        .is_some_and(|scopes| {
            scopes
                .split(',')
                .map(str::trim)
                .any(|scope| scope == expected)
        })
}

pub(super) fn decimal_id(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_u64()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .ok_or(ConnectionError::InvalidResponse)
}

pub(super) fn boolean(value: &Value, key: &str) -> Result<bool, ConnectionError> {
    value[key].as_bool().ok_or(ConnectionError::InvalidResponse)
}

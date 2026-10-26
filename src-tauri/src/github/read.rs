//! GitHub implementation of the provider read interfaces.

use super::{
    intake::repository_target,
    metadata::{array, polling_pull_request, sha, text},
    provider::{boolean, decimal_id, require_repo_scope, GithubClient, Transport},
    verify_identity,
};
use crate::provider::{
    AccountReader, Capabilities, ChangedFile, CommentCapability, Connection, ConnectionError,
    Lifecycle, Person, PullRequest, PullRequestReader, RemoteRepository, RepositoryBrowser,
    RepositoryOwner, RepositoryReader, RequestedTeam, ResolvedTarget, ReviewContext, ReviewFile,
    TreeEntry,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

impl<T: Transport> AccountReader for GithubClient<T> {
    fn current_identity(&self) -> Result<Person, ConnectionError> {
        let (user, _) = self.read("/user")?;
        verify_identity(&user, None)
    }

    fn resolve_person(&self, login: &str) -> Result<Person, ConnectionError> {
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
}

impl<T: Transport> RepositoryReader for GithubClient<T> {
    fn accessible_repositories(&self) -> Result<Vec<RemoteRepository>, ConnectionError> {
        let (entries, warnings) = self.repository_catalog(false)?;
        if let Some(warning) = warnings.first() {
            return Err(warning.error);
        }
        Ok(entries.into_iter().map(|(repo, _)| repo).collect())
    }

    fn repository_browser(&self, identity: &Person) -> Result<RepositoryBrowser, ConnectionError> {
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

    fn owner_repository_browser(
        &self,
        identity: &Person,
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

    fn connect(
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

    fn resolve_repository_target(
        &self,
        input: &str,
        account_id: &str,
    ) -> Result<ResolvedTarget, ConnectionError> {
        let target = repository_target(input)?;
        let connection = self.connect(&target.name, Some(account_id))?;
        let pull_request = target
            .pull_request_number
            .map(|number| self.pull_request(&connection.repository, number))
            .transpose()?;
        Ok(ResolvedTarget {
            connection,
            pull_request,
        })
    }
}

impl<T: Transport> PullRequestReader for GithubClient<T> {
    fn latest_pull_request_number(
        &self,
        repository: &RemoteRepository,
    ) -> Result<u64, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let (value, _) = self.read(&format!(
            "/repos/{name}/pulls?state=all&sort=created&direction=desc&per_page=1"
        ))?;
        let pulls = array(&value)?;
        if pulls.len() > 1 {
            return Err(ConnectionError::InvalidResponse);
        }
        pulls
            .first()
            .map(|pull| {
                if decimal_id(&pull["base"]["repo"]["id"])? != repository.id {
                    return Err(ConnectionError::RepositoryChanged);
                }
                pull["number"]
                    .as_u64()
                    .filter(|number| *number > 0)
                    .ok_or(ConnectionError::InvalidResponse)
            })
            .transpose()
            .map(|number| number.unwrap_or(0))
    }

    fn pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let list = self.pages(&format!(
            "/repos/{name}/pulls?state=all&sort=created&direction=asc&per_page=100&page=1"
        ))?;
        let mut ids = HashSet::new();
        let mut numbers = HashSet::new();
        let mut result = Vec::new();
        for summary in list {
            let id = decimal_id(&summary["id"])?;
            let number = summary["number"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or(ConnectionError::InvalidResponse)?;
            if !ids.insert(id.clone()) || !numbers.insert(number) {
                return Err(ConnectionError::IncompleteRead);
            }
            let path = format!("/repos/{name}/pulls/{number}");
            let (detail, _) = self.read(&path)?;
            if decimal_id(&detail["id"])? != id
                || detail["number"].as_u64() != Some(number)
                || detail["head"]["sha"] != summary["head"]["sha"]
            {
                return Err(ConnectionError::RevisionChanged);
            }
            let count = detail["changed_files"]
                .as_u64()
                .ok_or(ConnectionError::InvalidResponse)?;
            // GitHub's files endpoint cannot return more than 3,000 files.
            if count > 3_000 {
                return Err(ConnectionError::LimitExceeded);
            }
            let files = self.changed_files(&path, count)?;
            let (reviewers, response) = self.read(&format!("{path}/requested_reviewers"))?;
            if response.headers.contains_key("link") {
                return Err(ConnectionError::IncompleteRead);
            }
            let mut reviewer_ids = HashSet::new();
            let requested_reviewers = array(&reviewers["users"])?
                .iter()
                .map(|value| {
                    let identity = verify_identity(value, None)?;
                    if !reviewer_ids.insert(identity.id.clone()) {
                        return Err(ConnectionError::IncompleteRead);
                    }
                    Ok(identity)
                })
                .collect::<Result<_, _>>()?;
            let mut team_ids = HashSet::new();
            let requested_teams = array(&reviewers["teams"])?
                .iter()
                .map(|value| {
                    let id = decimal_id(&value["id"])?;
                    if !team_ids.insert(id.clone()) {
                        return Err(ConnectionError::IncompleteRead);
                    }
                    Ok(RequestedTeam {
                        id,
                        slug: text(&value["slug"])?,
                    })
                })
                .collect::<Result<_, _>>()?;
            let base_repository_id = decimal_id(&detail["base"]["repo"]["id"])?;
            if base_repository_id != repository.id {
                return Err(ConnectionError::RepositoryChanged);
            }
            let state = match (detail["state"].as_str(), detail["merged"].as_bool()) {
                (Some("open"), Some(false)) => Lifecycle::Open,
                (Some("closed"), Some(false)) => Lifecycle::Closed,
                (Some("closed"), Some(true)) => Lifecycle::Merged,
                _ => return Err(ConnectionError::InvalidResponse),
            };
            let author = match detail.get("user") {
                Some(Value::Null) => None,
                Some(user) => Some(verify_identity(user, None)?),
                None => return Err(ConnectionError::InvalidResponse),
            };
            let head_repository_id = match detail["head"].get("repo") {
                Some(Value::Null) => None,
                Some(repo) => Some(decimal_id(&repo["id"])?),
                None => return Err(ConnectionError::InvalidResponse),
            };
            let (current, _) = self.read(&path)?;
            for key in [
                "id",
                "number",
                "head",
                "base",
                "updated_at",
                "state",
                "draft",
                "merged",
                "changed_files",
            ] {
                if current.get(key) != detail.get(key) {
                    return Err(ConnectionError::RevisionChanged);
                }
            }
            result.push(PullRequest {
                mentioned: false,
                id,
                number,
                title: text(&detail["title"])?,
                author,
                requested_reviewers,
                requested_teams,
                state,
                draft: boolean(&detail, "draft")?,
                head_sha: sha(&detail["head"]["sha"])?,
                base_sha: sha(&detail["base"]["sha"])?,
                head_repository_id,
                base_repository_id,
                updated_at: text(&detail["updated_at"])?,
                files,
            });
        }
        Ok(result)
    }

    fn poll_pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let list = self.pages(&format!(
            "/repos/{name}/pulls?state=open&sort=created&direction=asc&per_page=100&page=1"
        ))?;
        let mut ids = HashSet::new();
        let mut numbers = HashSet::new();
        list.iter()
            .map(|value| {
                let pull = polling_pull_request(value)?;
                if !ids.insert(pull.id.clone()) || !numbers.insert(pull.number) {
                    return Err(ConnectionError::IncompleteRead);
                }
                if pull.base_repository_id != repository.id {
                    return Err(ConnectionError::RepositoryChanged);
                }
                Ok(pull)
            })
            .collect()
    }

    /// An open listing cannot establish closure. Verify every previously admitted
    /// active PR missing from that complete listing. Previously verified terminal
    /// PRs can reappear in the open listing without polling all historical details.
    fn poll_tracked_pull_requests(
        &self,
        repository: &RemoteRepository,
        tracked: &[(String, u64)],
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        let mut pulls = self.poll_pull_requests(repository)?;
        for (id, number) in tracked {
            if pulls.iter().any(|pull| &pull.id == id) {
                continue;
            }
            let pull = self.pull_request(repository, *number)?;
            if &pull.id != id {
                return Err(ConnectionError::RepositoryChanged);
            }
            pulls.push(pull);
        }
        Ok(pulls)
    }

    fn pull_request(
        &self,
        repo: &RemoteRepository,
        number: u64,
    ) -> Result<PullRequest, ConnectionError> {
        let name = crate::storage::canonical_repository(&repo.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let (value, _) = self.read(&format!("/repos/{name}/pulls/{number}"))?;
        let pull = polling_pull_request(&value)?;
        if pull.number != number || pull.base_repository_id != repo.id {
            return Err(ConnectionError::RepositoryChanged);
        }
        Ok(pull)
    }

    /// Changed files of one pull request; the listing endpoint omits them.
    fn pull_request_files(
        &self,
        repository: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<ChangedFile>, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let path = format!("/repos/{name}/pulls/{number}");
        let (detail, _) = self.read(&path)?;
        let count = detail["changed_files"]
            .as_u64()
            .ok_or(ConnectionError::InvalidResponse)?;
        self.changed_files(&path, count)
    }

    fn review_context(
        &self,
        repo: &RemoteRepository,
        number: u64,
        head: &str,
    ) -> Result<ReviewContext, ConnectionError> {
        let name = crate::storage::canonical_repository(&repo.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let path = format!("/repos/{name}/pulls/{number}");
        let (detail, _) = self.read(&path)?;
        let pull = polling_pull_request(&detail)?;
        if pull.number != number || pull.base_repository_id != repo.id || pull.head_sha != head {
            return Err(ConnectionError::RevisionChanged);
        }
        let count = detail["changed_files"]
            .as_u64()
            .ok_or(ConnectionError::InvalidResponse)?;
        if count > 3000 {
            return Err(ConnectionError::LimitExceeded);
        }
        let raw = self.pages(&format!("{path}/files?per_page=100&page=1"))?;
        if raw.len() as u64 != count {
            return Err(ConnectionError::IncompleteRead);
        }
        let mut files = Vec::new();
        let mut paths = BTreeSet::new();
        for file in raw {
            let path = file["filename"]
                .as_str()
                .ok_or(ConnectionError::InvalidResponse)?
                .to_string();
            if path.is_empty() || !paths.insert(path.clone()) {
                return Err(ConnectionError::IncompleteRead);
            }
            let status = file["status"]
                .as_str()
                .ok_or(ConnectionError::InvalidResponse)?
                .to_string();
            if !matches!(
                status.as_str(),
                "added" | "removed" | "modified" | "renamed" | "copied" | "changed" | "unchanged"
            ) {
                return Err(ConnectionError::InvalidResponse);
            }
            let previous_path = file
                .get("previous_filename")
                .map(|v| {
                    v.as_str()
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .ok_or(ConnectionError::InvalidResponse)
                })
                .transpose()?;
            if status == "renamed" && previous_path.is_none() {
                return Err(ConnectionError::IncompleteRead);
            }
            let patch = file
                .get("patch")
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .ok_or(ConnectionError::InvalidResponse)
                })
                .transpose()?;
            files.push(ReviewFile {
                path,
                previous_path,
                status,
                patch,
            });
        }
        // GitHub PR diffs compare against the merge base, not the current target tip.
        let (comparison, _) = self.read(&format!(
            "/repos/{name}/compare/{}...{}",
            pull.base_sha, pull.head_sha
        ))?;
        let base_revision = comparison["merge_base_commit"]["sha"]
            .as_str()
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or(ConnectionError::InvalidResponse)?
            .to_string();
        let base = self.review_tree(&name, &base_revision)?;
        let head_tree = self.review_tree(&name, &pull.head_sha)?;
        for file in &files {
            if (file.status != "removed" && !head_tree.contains_key(&file.path))
                || (file.status != "added"
                    && !(file.status == "copied" && file.previous_path.is_none())
                    && !base.contains_key(file.previous_path.as_ref().unwrap_or(&file.path)))
            {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        let current = self.pull_request(repo, number)?;
        if current != pull {
            return Err(ConnectionError::RevisionChanged);
        }
        Ok(ReviewContext {
            pull,
            base_revision,
            files,
            head: head_tree,
            base,
        })
    }

    fn file_content(&self, name: &str, entry: &TreeEntry) -> Result<Value, ConnectionError> {
        let name = crate::storage::canonical_repository(name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        if entry.kind != "blob" {
            return Ok(json!({"kind": entry.kind, "sha": entry.sha, "mode": entry.mode}));
        }
        if entry.sha.len() != 40 || !entry.sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ConnectionError::InvalidResponse);
        }
        let (value, _) = self.read(&format!("/repos/{name}/git/blobs/{}", entry.sha))?;
        if value["sha"].as_str() != Some(&entry.sha) || value["encoding"].as_str() != Some("base64")
        {
            return Err(ConnectionError::InvalidResponse);
        }
        let content = value["content"]
            .as_str()
            .ok_or(ConnectionError::InvalidResponse)?;
        let bytes = STANDARD
            .decode(content.split_whitespace().collect::<String>())
            .map_err(|_| ConnectionError::InvalidResponse)?;
        if value["size"].as_u64() != Some(bytes.len() as u64) {
            return Err(ConnectionError::IncompleteRead);
        }
        let size = bytes.len();
        match String::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => {
                Ok(json!({"kind":"text","text":text,"size":size,"mode":entry.mode,"sha":entry.sha}))
            }
            _ => Ok(json!({"kind":"binary","size":size,"mode":entry.mode,"sha":entry.sha})),
        }
    }
}

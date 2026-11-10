//! GitHub implementation of the provider read interfaces.

use super::{
    intake::repository_target,
    metadata::{array, polling_pull_request, sha, text},
    provider::{boolean, decimal_id, require_repo_scope, GithubClient, Transport},
    verify_identity,
};
use crate::provider::{
    AccountReader, Capabilities, ChangedFile, CommentCapability, Connection, ConnectionError,
    Lifecycle, Listing, Person, PullRequest, PullRequestReader, RemoteRepository,
    RepositoryBrowser, RepositoryOwner, RepositoryReader, RequestedTeam, ResolvedTarget,
    ReviewContext, ReviewFile, Since, TreeDiff, TreeEntry,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Tree reads one scoped walk may spend before it counts as cut off.
const MAX_SCOPED_TREE_READS: usize = 5_000;

/// A level-by-level tree diff under scope prefixes (`""` = everything).
struct ScopedWalk<'a, T: Transport> {
    client: &'a GithubClient<T>,
    name: &'a str,
    scope: &'a [String],
    reads: usize,
    files: Vec<ChangedFile>,
}

impl<T: Transport> ScopedWalk<'_, T> {
    fn directory_in_scope(&self, directory: &str) -> bool {
        self.scope
            .iter()
            .any(|p| p.starts_with(directory) || directory.starts_with(p.as_str()))
    }

    /// One tree level by object or commit; `None` when cut off.
    fn level(
        &mut self,
        sha: Option<&str>,
    ) -> Result<Option<BTreeMap<String, TreeEntry>>, ConnectionError> {
        let Some(sha) = sha else {
            return Ok(Some(BTreeMap::new()));
        };
        self.reads += 1;
        if self.reads > MAX_SCOPED_TREE_READS {
            return Ok(None);
        }
        self.client.tree_level(self.name, sha)
    }

    /// False when a read was cut off.
    fn walk(
        &mut self,
        old: Option<&str>,
        new: Option<&str>,
        directory: &str,
    ) -> Result<bool, ConnectionError> {
        if old == new {
            return Ok(true);
        }
        let (Some(before), Some(after)) = (self.level(old)?, self.level(new)?) else {
            return Ok(false);
        };
        let names: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
        for name in names {
            let (b, a) = (before.get(&name), after.get(&name));
            if b.zip(a)
                .is_some_and(|(b, a)| b.sha == a.sha && b.mode == a.mode)
            {
                continue;
            }
            let path = format!("{directory}{name}");
            let file = |e: Option<&TreeEntry>| e.filter(|e| e.kind != "tree").cloned();
            let (fb, fa) = (file(b), file(a));
            if (fb.is_some() || fa.is_some())
                && self.scope.iter().any(|p| path.starts_with(p.as_str()))
                && fb
                    .as_ref()
                    .zip(fa.as_ref())
                    .is_none_or(|(b, a)| b.sha != a.sha || b.mode != a.mode)
            {
                self.files.push(ChangedFile {
                    status: match (&fb, &fa) {
                        (Some(_), Some(_)) => "modified",
                        (None, _) => "added",
                        _ => "removed",
                    }
                    .into(),
                    sha: fa.or(fb).map(|e| e.sha).unwrap_or_default(),
                    path: path.clone(),
                    previous_path: None,
                    additions: 0,
                    deletions: 0,
                    changes: 0,
                });
            }
            let tree =
                |e: Option<&TreeEntry>| e.filter(|e| e.kind == "tree").map(|e| e.sha.clone());
            let (tb, ta) = (tree(b), tree(a));
            let child = format!("{path}/");
            if (tb.is_some() || ta.is_some())
                && tb != ta
                && self.directory_in_scope(&child)
                && !self.walk(tb.as_deref(), ta.as_deref(), &child)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl<T: Transport> GithubClient<T> {
    /// One tree level by object or commit; `None` when GitHub truncated it.
    fn tree_level(
        &self,
        name: &str,
        sha: &str,
    ) -> Result<Option<BTreeMap<String, TreeEntry>>, ConnectionError> {
        let (value, _) = self.read(&format!("/repos/{name}/git/trees/{sha}"))?;
        match value["truncated"].as_bool() {
            Some(true) => return Ok(None),
            Some(false) => {}
            None => return Err(ConnectionError::IncompleteRead),
        }
        let entries: Vec<TreeEntry> = serde_json::from_value(value["tree"].clone())
            .map_err(|_| ConnectionError::InvalidResponse)?;
        let mut level = BTreeMap::new();
        for entry in entries {
            if entry.path.is_empty()
                || entry.path.contains('/')
                || entry.sha.len() != 40
                || !entry.sha.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(ConnectionError::InvalidResponse);
            }
            if level.insert(entry.path.clone(), entry).is_some() {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        Ok(Some(level))
    }

    /// The tree entries of `paths` at `revision`, read level by level along
    /// those paths only (ADR 0028): a review of a repository whose whole tree
    /// GitHub truncates. Absent paths are absent.
    fn entries_at(
        &self,
        name: &str,
        revision: &str,
        paths: &BTreeSet<String>,
    ) -> Result<BTreeMap<String, TreeEntry>, ConnectionError> {
        let mut levels: BTreeMap<String, Option<BTreeMap<String, TreeEntry>>> = BTreeMap::new();
        let mut result = BTreeMap::new();
        'paths: for path in paths {
            let mut tree = revision.to_owned();
            let mut parts = path.split('/').peekable();
            while let Some(part) = parts.next() {
                if !levels.contains_key(&tree) {
                    let level = self.tree_level(name, &tree)?;
                    levels.insert(tree.clone(), level);
                }
                let Some(entry) = levels[&tree].as_ref().and_then(|level| level.get(part)) else {
                    if levels[&tree].is_none() {
                        return Err(ConnectionError::LimitExceeded);
                    }
                    continue 'paths;
                };
                if parts.peek().is_none() {
                    result.insert(
                        path.clone(),
                        TreeEntry {
                            path: path.clone(),
                            ..entry.clone()
                        },
                    );
                } else if entry.kind == "tree" {
                    tree = entry.sha.clone();
                } else {
                    continue 'paths;
                }
            }
        }
        Ok(result)
    }
}

/// A whole-tree read a very large repository cannot serve: truncated or past
/// the body cap, or not downloaded within the request timeout.
fn tree_too_large(error: ConnectionError) -> bool {
    matches!(
        error,
        ConnectionError::LimitExceeded | ConnectionError::Network | ConnectionError::Timeout
    )
}

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
        let (mut by_number, mut by_id) = (BTreeMap::new(), BTreeMap::new());
        let mut result = Vec::new();
        for summary in list {
            let id = decimal_id(&summary["id"])?;
            let number = summary["number"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or(ConnectionError::InvalidResponse)?;
            // A listing that moved while paged repeats an entry: read once.
            // An ID under another number is no repeat.
            match (by_number.get(&number), by_id.get(&id)) {
                (Some(known), Some(_)) if known == &id => continue,
                (None, None) => {
                    by_number.insert(number, id.clone());
                    by_id.insert(id.clone(), number);
                }
                _ => return Err(ConnectionError::IncompleteRead),
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
            // GitHub's files endpoint lists at most 3,000 files: past that,
            // the files it lists.
            let cap = super::metadata::FILES_LISTING_CAP;
            let files = if count > cap {
                super::metadata::changed_file_entries(
                    self.pages_until(&format!("{path}/files?per_page=100&page=1"), cap as usize)?,
                )?
            } else {
                self.changed_files(&path, count)?
            };
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
        // A listing that moved while paged repeats entries: de-duplicated.
        let mut pulls = BTreeMap::new();
        for value in &list {
            let pull = polling_pull_request(value)?;
            if pull.base_repository_id != repository.id {
                return Err(ConnectionError::RepositoryChanged);
            }
            pulls.entry(pull.number).or_insert(pull);
        }
        Ok(pulls.into_values().collect())
    }

    fn poll_batch(
        &self,
        repository: &RemoteRepository,
        tracked: &[(String, u64)],
        closed: &[(String, u64)],
        scope: &crate::provider::ListingScope,
        cursor: Option<&crate::provider::ListingCursor>,
        now: i64,
    ) -> Result<crate::provider::PolledPullRequests, ConnectionError> {
        self.listing_batch(repository, tracked, closed, scope, cursor, now)
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

    /// The PR's `changed_files` against what `/files` lists, paged to the end.
    fn changed_file_listing(
        &self,
        repository: &RemoteRepository,
        pull: &PullRequest,
        list: bool,
    ) -> Result<Listing, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let path = format!("/repos/{name}/pulls/{}", pull.number);
        let count = |client: &Self| -> Result<u64, ConnectionError> {
            let (detail, _) = client.read(&path)?;
            if detail["head"]["sha"].as_str() != Some(pull.head_sha.as_str()) {
                return Err(ConnectionError::RevisionChanged);
            }
            detail["changed_files"]
                .as_u64()
                .ok_or(ConnectionError::InvalidResponse)
        };
        let reported = count(self)?;
        // Listing past GitHub's cap cannot complete: no pointless paging.
        if reported > super::metadata::FILES_LISTING_CAP {
            return Ok(Listing::Incomplete(reported));
        }
        if !list {
            return Ok(Listing::Visible);
        }
        let files = self.listed_files(&path)?;
        let listed = files.len() as u64;
        if listed == reported {
            return Ok(Listing::Complete(files));
        }
        // Short of the count on the same head is a listing that cannot finish;
        // anything else is a read that raced a push.
        if listed < reported && count(self)? == reported {
            return Ok(Listing::Incomplete(reported));
        }
        Err(ConnectionError::IncompleteRead)
    }

    /// The PR's merge base, from `compare/{base}...{head}`.
    fn merge_base(
        &self,
        repository: &RemoteRepository,
        pull: &PullRequest,
    ) -> Result<String, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let (value, _) = self.read(&format!(
            "/repos/{name}/compare/{}...{}?per_page=1",
            pull.base_sha, pull.head_sha
        ))?;
        sha(&value["merge_base_commit"]["sha"])
    }

    /// Recursive trees of both commits; with a scope, a level-by-level walk
    /// that only enters differing subtrees under the scope prefixes.
    fn tree_diff(
        &self,
        repository: &RemoteRepository,
        from: &str,
        to: &str,
        scope: &[String],
    ) -> Result<TreeDiff, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        if scope.is_empty() {
            let tree = |revision: &str| match self.review_tree(&name, revision) {
                Err(ConnectionError::LimitExceeded) => Ok(None),
                other => other.map(Some),
            };
            return Ok(match (tree(from)?, tree(to)?) {
                (Some(old), Some(new)) => {
                    TreeDiff::Complete(crate::provider::tree_changes(&old, &new))
                }
                _ => TreeDiff::Truncated,
            });
        }
        let mut walk = ScopedWalk {
            client: self,
            name: &name,
            scope,
            reads: 0,
            files: Vec::new(),
        };
        Ok(if walk.walk(Some(from), Some(to), "")? {
            TreeDiff::Complete(walk.files)
        } else {
            TreeDiff::Truncated
        })
    }

    /// `compare/{old}...{head}`: only an `ahead` comparison is incremental.
    fn changed_files_since(
        &self,
        repository: &RemoteRepository,
        pull: &PullRequest,
        old_head: &str,
    ) -> Result<Since, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let commit = |sha: &str| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit());
        if !commit(old_head) || !commit(&pull.head_sha) {
            return Ok(Since::WholePullRequest);
        }
        let response = match self.transport.get(&format!(
            "/repos/{name}/compare/{old_head}...{}",
            pull.head_sha
        )) {
            Err(ConnectionError::LimitExceeded) => return Ok(Since::WholePullRequest),
            other => other?,
        };
        // An old head no longer in the repository (force-push, gc).
        if matches!(response.status, 404 | 422) {
            return Ok(Since::WholePullRequest);
        }
        let (value, _) = super::provider::parse_response(response)?;
        match value["status"].as_str() {
            Some("identical") => Ok(Since::Files(Vec::new())),
            Some("ahead") => {
                let raw = value["files"]
                    .as_array()
                    .ok_or(ConnectionError::InvalidResponse)?;
                if raw.len() >= super::metadata::COMPARE_FILES_CAP {
                    return Ok(Since::Capped);
                }
                Ok(Since::Files(super::metadata::changed_file_entries(
                    raw.clone(),
                )?))
            }
            // Behind or diverged: the old head is not an ancestor (rebase).
            Some(_) => Ok(Since::WholePullRequest),
            None => Err(ConnectionError::InvalidResponse),
        }
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
        // Past GitHub's listing cap the listed files keep their patches and
        // the merge-base tree diff (the path facts' ladder) lists the rest.
        let cap = super::metadata::FILES_LISTING_CAP;
        let raw = self.pages_until(&format!("{path}/files?per_page=100&page=1"), cap as usize)?;
        if raw.len() as u64 != count.min(cap) {
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
        let whole = (|| {
            Ok::<_, ConnectionError>((
                self.review_tree(&name, &base_revision)?,
                self.review_tree(&name, &pull.head_sha)?,
            ))
        })();
        let (base, head_tree) = match whole {
            Ok(trees) => trees,
            // Trees of the changed paths only: what the PR changed, listed or
            // found by a bounded walk of the differing subtrees.
            Err(error) if tree_too_large(error) => {
                let mut changed: BTreeSet<String> = BTreeSet::new();
                if count > cap {
                    let root = [String::new()];
                    if let TreeDiff::Complete(walked) =
                        self.tree_diff(repo, &base_revision, &pull.head_sha, &root)?
                    {
                        changed.extend(walked.into_iter().map(|file| file.path));
                    }
                }
                let mut before = changed.clone();
                let mut after = changed;
                for file in &files {
                    if file.status != "removed" {
                        after.insert(file.path.clone());
                    }
                    if file.status != "added" {
                        before.insert(file.previous_path.clone().unwrap_or(file.path.clone()));
                    }
                }
                (
                    self.entries_at(&name, &base_revision, &before)?,
                    self.entries_at(&name, &pull.head_sha, &after)?,
                )
            }
            Err(error) => return Err(error),
        };
        for file in &files {
            if (file.status != "removed" && !head_tree.contains_key(&file.path))
                || (file.status != "added"
                    && !(file.status == "copied" && file.previous_path.is_none())
                    && !base.contains_key(file.previous_path.as_ref().unwrap_or(&file.path)))
            {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        if count > cap {
            let changes = crate::provider::tree_changes(&base, &head_tree);
            let changed: BTreeSet<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            if files
                .iter()
                .any(|f| f.status != "unchanged" && !changed.contains(f.path.as_str()))
            {
                return Err(ConnectionError::IncompleteRead);
            }
            // A listed rename covers its source only when it left head; a copy
            // source or a reused old path is its own change.
            let listed: BTreeSet<String> = files
                .iter()
                .map(|f| f.path.clone())
                .chain(
                    files
                        .iter()
                        .filter(|f| f.status == "renamed")
                        .filter_map(|f| f.previous_path.clone())
                        .filter(|p| !head_tree.contains_key(p)),
                )
                .collect();
            // Unlisted files have no provider patch: no addressable line.
            files.extend(
                changes
                    .into_iter()
                    .filter(|c| !listed.contains(&c.path))
                    .map(|c| ReviewFile {
                        path: c.path,
                        previous_path: None,
                        status: c.status,
                        patch: None,
                    }),
            );
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

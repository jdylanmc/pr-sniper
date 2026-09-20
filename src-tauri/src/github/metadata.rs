use super::provider::{boolean, decimal_id, GithubClient, RemoteRepository, Response, Transport};
use super::{verify_identity, ConnectionError, Identity};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PullRequest {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub author: Option<Identity>,
    pub requested_reviewers: Vec<Identity>,
    pub requested_teams: Vec<RequestedTeam>,
    pub state: Lifecycle,
    pub draft: bool,
    pub head_sha: String,
    pub base_sha: String,
    pub head_repository_id: Option<String>,
    pub base_repository_id: String,
    pub updated_at: String,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestedTeam {
    pub id: String,
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub changes: u64,
    pub sha: String,
}

impl<T: Transport> GithubClient<T> {
    pub fn poll_pull_requests_since(
        &self,
        repository: &RemoteRepository,
        _updated_after: Option<&str>,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        self.poll_pull_requests(repository)
    }

    pub fn poll_pull_requests(
        &self,
        repository: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let list = self.pages(&format!(
            "/repos/{name}/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1"
        ))?;
        let mut ids = HashSet::new();
        let mut numbers = HashSet::new();
        let mut result = Vec::new();
        for detail in list {
            let id = decimal_id(&detail["id"])?;
            let number = unsigned(&detail["number"])?;
            if number == 0 || !ids.insert(id.clone()) || !numbers.insert(number) {
                return Err(ConnectionError::IncompleteRead);
            }
            let base_repository_id = decimal_id(&detail["base"]["repo"]["id"])?;
            if base_repository_id != repository.id {
                return Err(ConnectionError::RepositoryChanged);
            }
            let state = match detail["state"].as_str() {
                Some("open") => Lifecycle::Open,
                Some("closed") => Lifecycle::Closed,
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
            let mut reviewer_ids = HashSet::new();
            let requested_reviewers = array(&detail["requested_reviewers"])?
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
            let requested_teams = array(&detail["requested_teams"])?
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
            result.push(PullRequest {
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
                files: Vec::new(),
            });
        }
        Ok(result)
    }

    pub fn pull_requests(
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
                return Err(ConnectionError::IncompleteRead);
            }
            let raw_files = self.pages(&format!("{path}/files?per_page=100&page=1"))?;
            if raw_files.len() as u64 != count {
                return Err(ConnectionError::IncompleteRead);
            }
            let mut paths = HashSet::new();
            let mut files = Vec::new();
            for file in raw_files {
                let filename = text(&file["filename"])?;
                if !paths.insert(filename.clone()) {
                    return Err(ConnectionError::IncompleteRead);
                }
                let status = text(&file["status"])?;
                if ![
                    "added",
                    "removed",
                    "modified",
                    "renamed",
                    "copied",
                    "changed",
                    "unchanged",
                ]
                .contains(&status.as_str())
                {
                    return Err(ConnectionError::InvalidResponse);
                }
                let previous_path = optional_text(file.get("previous_filename"))?;
                if status == "renamed" && previous_path.is_none() {
                    return Err(ConnectionError::InvalidResponse);
                }
                files.push(ChangedFile {
                    path: filename,
                    previous_path,
                    status,
                    additions: unsigned(&file["additions"])?,
                    deletions: unsigned(&file["deletions"])?,
                    changes: unsigned(&file["changes"])?,
                    sha: sha(&file["sha"])?,
                });
            }
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

    fn pages(&self, first: &str) -> Result<Vec<Value>, ConnectionError> {
        let mut path = first.to_string();
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        let mut advertised_last = None;
        loop {
            if !seen.insert(path.clone()) || seen.len() > 10_000 {
                return Err(ConnectionError::IncompleteRead);
            }
            let (value, response) = self.read(&path)?;
            let page = array(&value)?;
            if page.len() > 100 {
                return Err(ConnectionError::InvalidResponse);
            }
            let next = next_page(&path, &response, &mut advertised_last)?;
            if page.is_empty() && (next.is_some() || seen.len() > 1) {
                return Err(ConnectionError::IncompleteRead);
            }
            result.extend(page.iter().cloned());
            match next {
                Some(next) => path = next,
                None => return Ok(result),
            }
        }
    }
}

fn array(value: &Value) -> Result<&Vec<Value>, ConnectionError> {
    value.as_array().ok_or(ConnectionError::InvalidResponse)
}

fn text(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or(ConnectionError::InvalidResponse)
}

fn unsigned(value: &Value) -> Result<u64, ConnectionError> {
    value.as_u64().ok_or(ConnectionError::InvalidResponse)
}

fn sha(value: &Value) -> Result<String, ConnectionError> {
    let value = text(value)?;
    if ![40, 64].contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConnectionError::InvalidResponse);
    }
    Ok(value)
}

fn optional_text(value: Option<&Value>) -> Result<Option<String>, ConnectionError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => text(value).map(Some),
    }
}

fn next_page(
    path: &str,
    response: &Response,
    advertised_last: &mut Option<u64>,
) -> Result<Option<String>, ConnectionError> {
    let current = reqwest::Url::parse(&format!("https://api.github.com{path}"))
        .map_err(|_| ConnectionError::IncompleteRead)?;
    let query: BTreeMap<_, _> = current
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let page = query
        .get("page")
        .and_then(|page| page.parse::<u64>().ok())
        .ok_or(ConnectionError::IncompleteRead)?;
    let Some(link) = response.headers.get("link") else {
        return if advertised_last.is_some_and(|last| last > page) {
            Err(ConnectionError::IncompleteRead)
        } else {
            Ok(None)
        };
    };
    let mut next = None;
    let mut last = None;
    let mut relations = HashSet::new();
    for item in link.split(',') {
        let (target, relation) = item
            .trim()
            .split_once(';')
            .ok_or(ConnectionError::IncompleteRead)?;
        let target = target
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .ok_or(ConnectionError::IncompleteRead)?;
        let url = reqwest::Url::parse(target).map_err(|_| ConnectionError::IncompleteRead)?;
        if url.origin() != current.origin()
            || url.path() != current.path()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ConnectionError::IncompleteRead);
        }
        let pairs: Vec<_> = url
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let unique: BTreeMap<_, _> = pairs.iter().cloned().collect();
        let mut expected = query.clone();
        let number = pairs
            .iter()
            .find(|(key, _)| key == "page")
            .and_then(|(_, value)| value.parse::<u64>().ok())
            .ok_or(ConnectionError::IncompleteRead)?;
        expected.insert("page".into(), number.to_string());
        if pairs.len() != unique.len() || unique != expected {
            return Err(ConnectionError::IncompleteRead);
        }
        if !relations.insert(relation.trim()) {
            return Err(ConnectionError::IncompleteRead);
        }
        match relation.trim() {
            "rel=\"next\"" => {
                if number != page + 1 || next.is_some() {
                    return Err(ConnectionError::IncompleteRead);
                }
                next = Some(format!(
                    "{}?{}",
                    url.path(),
                    url.query().ok_or(ConnectionError::IncompleteRead)?
                ));
            }
            "rel=\"last\"" => last = Some(number),
            "rel=\"prev\"" | "rel=\"first\"" => (),
            _ => return Err(ConnectionError::IncompleteRead),
        }
    }
    if let Some(last) = last {
        if last < page || advertised_last.is_some_and(|previous| last < previous) {
            return Err(ConnectionError::IncompleteRead);
        }
        *advertised_last = Some(last);
    }
    if next.is_none() && advertised_last.is_some_and(|last| last > page) {
        return Err(ConnectionError::IncompleteRead);
    }
    Ok(next)
}

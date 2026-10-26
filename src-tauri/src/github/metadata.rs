use super::provider::{boolean, decimal_id, GithubClient, Response, Transport};
use super::{verify_identity, ConnectionError};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub use crate::provider::{ChangedFile, Lifecycle, PullRequest, RequestedTeam};

impl<T: Transport> GithubClient<T> {
    pub(super) fn changed_files(
        &self,
        path: &str,
        count: u64,
    ) -> Result<Vec<ChangedFile>, ConnectionError> {
        // GitHub's files endpoint cannot return more than 3,000 files.
        if count > 3_000 {
            return Err(ConnectionError::LimitExceeded);
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
        Ok(files)
    }

    pub(super) fn pages(&self, first: &str) -> Result<Vec<Value>, ConnectionError> {
        self.pages_limited(first, 1_000_000)
    }

    pub(super) fn pages_limited(
        &self,
        first: &str,
        limit: usize,
    ) -> Result<Vec<Value>, ConnectionError> {
        let mut path = first.to_string();
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        let mut advertised_last = None;
        loop {
            if !seen.insert(path.clone()) {
                return Err(ConnectionError::IncompleteRead);
            }
            if seen.len() > 10_000 {
                return Err(ConnectionError::LimitExceeded);
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
            if result.len() + page.len() > limit {
                return Err(ConnectionError::LimitExceeded);
            }
            result.extend(page.iter().cloned());
            match next {
                Some(next) => path = next,
                None => return Ok(result),
            }
        }
    }
}

pub(super) fn polling_pull_request(value: &Value) -> Result<PullRequest, ConnectionError> {
    let id = decimal_id(&value["id"])?;
    let number = unsigned(&value["number"])?;
    if number == 0 {
        return Err(ConnectionError::InvalidResponse);
    }
    let state = match (value["state"].as_str(), value["merged"].as_bool()) {
        (Some("open"), None | Some(false)) => Lifecycle::Open,
        (Some("closed"), Some(false)) => Lifecycle::Closed,
        (Some("closed"), Some(true)) => Lifecycle::Merged,
        (Some("closed"), None) if value.get("merged_at") == Some(&Value::Null) => Lifecycle::Closed,
        (Some("closed"), None)
            if value["merged_at"]
                .as_str()
                .is_some_and(|time| chrono::DateTime::parse_from_rfc3339(time).is_ok()) =>
        {
            Lifecycle::Merged
        }
        _ => return Err(ConnectionError::InvalidResponse),
    };
    let author = match value.get("user") {
        Some(Value::Null) => None,
        Some(user) => Some(verify_identity(user, None)?),
        None => return Err(ConnectionError::InvalidResponse),
    };
    let requested_reviewers = array(&value["requested_reviewers"])?
        .iter()
        .map(|reviewer| verify_identity(reviewer, None))
        .collect::<Result<Vec<_>, _>>()?;
    let requested_teams = array(&value["requested_teams"])?
        .iter()
        .map(|team| {
            Ok(RequestedTeam {
                id: decimal_id(&team["id"])?,
                slug: text(&team["slug"])?,
            })
        })
        .collect::<Result<Vec<_>, ConnectionError>>()?;
    let base_repository_id = decimal_id(&value["base"]["repo"]["id"])?;
    let head_repository_id = match value["head"].get("repo") {
        Some(Value::Null) => None,
        Some(repository) => Some(decimal_id(&repository["id"])?),
        None => return Err(ConnectionError::InvalidResponse),
    };
    Ok(PullRequest {
        mentioned: false,
        id,
        number,
        title: text(&value["title"])?,
        author,
        requested_reviewers,
        requested_teams,
        state,
        draft: boolean(value, "draft")?,
        head_sha: sha(&value["head"]["sha"])?,
        base_sha: sha(&value["base"]["sha"])?,
        head_repository_id,
        base_repository_id,
        updated_at: text(&value["updated_at"])?,
        files: Vec::new(),
    })
}

pub(super) fn array(value: &Value) -> Result<&Vec<Value>, ConnectionError> {
    value.as_array().ok_or(ConnectionError::InvalidResponse)
}

pub(super) fn text(value: &Value) -> Result<String, ConnectionError> {
    value
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or(ConnectionError::InvalidResponse)
}

fn unsigned(value: &Value) -> Result<u64, ConnectionError> {
    value.as_u64().ok_or(ConnectionError::InvalidResponse)
}

pub(super) fn sha(value: &Value) -> Result<String, ConnectionError> {
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

pub(super) fn next_page(
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
    if advertised_last.is_some_and(|last| page > last) {
        return Err(ConnectionError::IncompleteRead);
    }
    let Some(link) = response.headers.get("link") else {
        return if advertised_last.is_some_and(|last| last > page) {
            Err(ConnectionError::IncompleteRead)
        } else {
            Ok(None)
        };
    };
    let mut next = None;
    let mut next_number = None;
    let mut last = None;
    let mut relations = HashSet::new();
    // Repository affiliation parameters contain commas inside the target URL.
    let mut start = 0;
    let mut in_target = false;
    let mut items = Vec::new();
    for (index, byte) in link.bytes().enumerate() {
        match byte {
            b'<' => in_target = true,
            b'>' => in_target = false,
            b',' if !in_target => {
                items.push(&link[start..index]);
                start = index + 1;
            }
            _ => (),
        }
    }
    items.push(&link[start..]);
    for item in items {
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
                next_number = Some(number);
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
    }
    let effective_last = last.or(*advertised_last);
    if effective_last.is_some_and(|last| next_number.is_some_and(|number| number > last))
        || (next.is_none() && effective_last.is_some_and(|last| last > page))
    {
        return Err(ConnectionError::IncompleteRead);
    }
    *advertised_last = effective_last;
    Ok(next)
}

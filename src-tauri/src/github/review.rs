use super::{
    metadata::{polling_pull_request, PullRequest},
    provider::{GithubClient, RemoteRepository, Transport},
    ConnectionError,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub struct GuardedTransport<T> {
    transport: T,
    guard: Arc<dyn Fn() -> Result<(), ConnectionError> + Send + Sync>,
}

impl<T: Transport> Transport for GuardedTransport<T> {
    fn get(&self, path: &str) -> Result<super::provider::Response, ConnectionError> {
        (self.guard)()?;
        let result = self.transport.get(path)?;
        (self.guard)()?;
        Ok(result)
    }
}

impl<T: super::threads::QueryTransport> super::threads::QueryTransport for GuardedTransport<T> {
    fn query(
        &self,
        query: &str,
        variables: Value,
    ) -> Result<super::provider::Response, ConnectionError> {
        (self.guard)()?;
        let result = self.transport.query(query, variables)?;
        (self.guard)()?;
        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: String,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TreeEntry {
    pub path: String,
    pub mode: String,
    pub sha: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct ReviewContext {
    pub pull: PullRequest,
    pub base_revision: String,
    pub files: Vec<ReviewFile>,
    pub head: BTreeMap<String, TreeEntry>,
    pub base: BTreeMap<String, TreeEntry>,
}

impl<T: Transport> GithubClient<T> {
    pub fn guarded(
        self,
        guard: Arc<dyn Fn() -> Result<(), ConnectionError> + Send + Sync>,
    ) -> GithubClient<GuardedTransport<T>> {
        GithubClient::new(GuardedTransport {
            transport: self.transport,
            guard,
        })
    }

    pub fn review_pull(
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

    pub fn review_context(
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
            return Err(ConnectionError::IncompleteRead);
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
        let current = self.review_pull(repo, number)?;
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

    fn review_tree(
        &self,
        name: &str,
        revision: &str,
    ) -> Result<BTreeMap<String, TreeEntry>, ConnectionError> {
        let (value, _) = self.read(&format!("/repos/{name}/git/trees/{revision}?recursive=1"))?;
        if value["truncated"].as_bool() != Some(false) {
            return Err(ConnectionError::IncompleteRead);
        }
        let entries: Vec<TreeEntry> = serde_json::from_value(value["tree"].clone())
            .map_err(|_| ConnectionError::InvalidResponse)?;
        let mut result = BTreeMap::new();
        for entry in entries {
            if entry.path.is_empty()
                || entry.sha.len() != 40
                || !entry.sha.bytes().all(|b| b.is_ascii_hexdigit())
                || !matches!(entry.kind.as_str(), "blob" | "tree" | "commit")
            {
                return Err(ConnectionError::InvalidResponse);
            }
            if result.insert(entry.path.clone(), entry).is_some() {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        Ok(result)
    }

    pub fn review_source(&self, name: &str, entry: &TreeEntry) -> Result<Value, ConnectionError> {
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

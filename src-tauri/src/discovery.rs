use crate::storage::canonical_repository;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct DiscoveredRepository {
    pub path: String,
    pub name: Option<String>,
    pub unavailable: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Discovery {
    pub root: String,
    pub repositories: Vec<DiscoveredRepository>,
    pub warnings: Vec<String>,
}

// Read only bounded local metadata, never git, hooks, includes, or repository code.
pub fn discover(root: &Path) -> Result<Discovery, String> {
    if !root.is_absolute() {
        return Err("Choose an absolute local root folder.".into());
    }
    let root = root
        .canonicalize()
        .map_err(|_| "Cannot open this folder. Check access and choose it again.")?;
    if !root.is_dir() {
        return Err("Choose a folder, not a file.".into());
    }
    let mut result = Discovery {
        root: root.to_string_lossy().into(),
        repositories: Vec::new(),
        warnings: Vec::new(),
    };
    let mut pending = vec![(root.clone(), 0)];
    let mut visited = 0;
    while let Some((path, depth)) = pending.pop() {
        visited += 1;
        if visited > 10_000 {
            result.warnings.push("Folder scan reached its local resource limit. Choose a smaller root to see remaining repositories.".into());
            break;
        }
        let git = path.join(".git");
        if let Ok(metadata) = fs::symlink_metadata(&git) {
            let remote = if metadata.is_dir() && !metadata.file_type().is_symlink() {
                read_remote(&git.join("config"))
            } else {
                Err(
                    "Linked worktree metadata is not followed. Add its GitHub repository manually."
                        .into(),
                )
            };
            result.repositories.push(match remote {
                Ok(name) => DiscoveredRepository {
                    path: path.to_string_lossy().into(),
                    name: Some(name),
                    unavailable: None,
                },
                Err(reason) => DiscoveredRepository {
                    path: path.to_string_lossy().into(),
                    name: None,
                    unavailable: Some(reason),
                },
            });
            continue;
        }
        let entries = fs::read_dir(&path).map_err(|_| "Cannot read a folder inside the chosen root. Check permissions or choose a smaller root.")?;
        for entry in entries {
            let entry = entry.map_err(|_| "Cannot read an entry in the chosen folder.")?;
            let kind = entry
                .file_type()
                .map_err(|_| "Cannot inspect an entry in the chosen folder.")?;
            if !kind.is_dir()
                || kind.is_symlink()
                || entry.file_name().to_string_lossy().starts_with('.')
            {
                continue;
            }
            if depth >= 12 {
                if !result.warnings.iter().any(|s| s.contains("depth")) {
                    result.warnings.push("Folder scan reached its depth limit. Choose a deeper root to see remaining repositories.".into());
                }
            } else {
                pending.push((entry.path(), depth + 1));
            }
        }
    }
    result.repositories.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn read_remote(config: &PathBuf) -> Result<String, String> {
    let metadata = fs::symlink_metadata(config)
        .map_err(|_| "Cannot read Git metadata. Check folder permissions.")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 128 * 1024 {
        return Err(
            "Git metadata is linked or too large to read safely. Add the repository manually."
                .into(),
        );
    }
    let text = fs::read_to_string(config)
        .map_err(|_| "Cannot read Git metadata. Check folder permissions.")?;
    let mut remotes = BTreeMap::new();
    let mut remote = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            remote = line
                .strip_prefix("[remote \"")
                .and_then(|s| s.strip_suffix("\"]"))
                .map(str::to_string);
        } else if let Some(name) = &remote {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case("url") {
                    let value = value.trim().trim_matches('"');
                    let github = value
                        .strip_prefix("git@github.com:")
                        .or_else(|| value.strip_prefix("ssh://git@github.com/"))
                        .or_else(|| value.strip_prefix("https://github.com/"));
                    if let Some(path) = github {
                        if let Ok(canonical) = canonical_repository(path) {
                            remotes.insert(name.clone(), canonical);
                        }
                    }
                }
            }
        }
    }
    if let Some(origin) = remotes.remove("origin") {
        return Ok(origin);
    }
    let names: std::collections::BTreeSet<_> = remotes.into_values().collect();
    if names.len() == 1 {
        return Ok(names.into_iter().next().unwrap());
    }
    Err(if names.is_empty() {
        "No supported GitHub remote. Add an HTTPS or SSH github.com remote, then scan again."
    } else {
        "Multiple GitHub remotes without an origin. Add the intended repository manually."
    }
    .into())
}

//! Path globs (ADR 0027): the repository gate decides what is queued, a
//! secondary assignment's globs decide whether it reviews a revision.
//!
//! Syntax: include patterns plus `!pattern` excludes, gitignore-style. Per
//! file the last matching pattern wins; a list of only excludes starts from
//! "every file". Pattern syntax is `admission::glob_matches`.

use crate::admission::{glob_matches, valid_pattern, MAX_PATTERN_LENGTH, MAX_VALUES_PER_RULE};
use crate::provider::{
    ChangedFile, ConnectionError, Listing, PullRequest, PullRequestReader, RemoteRepository, Since,
    TreeDiff,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Patterns one path list may hold: both legacy path rules' worth, so every
/// migrated include + exclude list fits.
pub const MAX_PATTERNS: usize = 2 * MAX_VALUES_PER_RULE;

/// Validates one path list; `owner` names it in errors.
pub fn validate(patterns: &[String], owner: &str) -> Result<(), String> {
    if patterns.len() > MAX_PATTERNS {
        return Err(format!(
            "{owner} paths can list at most {MAX_PATTERNS} patterns."
        ));
    }
    let mut seen = HashSet::new();
    for pattern in patterns {
        let glob = pattern.strip_prefix('!').unwrap_or(pattern);
        if !valid_pattern(glob) {
            return Err(format!(
                "{owner} path patterns must be 1-{MAX_PATTERN_LENGTH} characters, repository-relative, without '..' or control characters; a leading '!' excludes."
            ));
        }
        if !seen.insert(pattern) {
            return Err(format!("{owner} paths list {pattern} twice."));
        }
    }
    Ok(())
}

fn selected(patterns: &[String], name: &str) -> bool {
    let mut selected = patterns.iter().all(|p| p.starts_with('!'));
    for pattern in patterns {
        match pattern.strip_prefix('!') {
            Some(glob) if glob_matches(glob, name) => selected = false,
            None if glob_matches(pattern, name) => selected = true,
            _ => {}
        }
    }
    selected
}

/// Whether any changed file (either name of a rename) is selected. Empty
/// patterns match everything, even no files.
pub fn matches(patterns: &[String], files: &[ChangedFile]) -> bool {
    patterns.is_empty()
        || files.iter().any(|file| {
            std::iter::once(file.path.as_str())
                .chain(file.previous_path.as_deref())
                .any(|name| selected(patterns, name))
        })
}

/// Directory prefixes a scoped tree diff must walk so `patterns` decide the
/// same as on the full list: each include's fixed leading directories. An
/// exclude-only list starts from every file, so it walks the root (`""`).
pub fn scope(patterns: &[String]) -> Vec<String> {
    let includes: Vec<_> = patterns.iter().filter(|p| !p.starts_with('!')).collect();
    if includes.is_empty() && !patterns.is_empty() {
        return vec![String::new()];
    }
    minimal(
        includes
            .iter()
            .map(|pattern| {
                let segments: Vec<_> = pattern.split('/').collect();
                segments[..segments.len() - 1]
                    .iter()
                    .take_while(|s| !s.contains(['*', '?']))
                    .fold(String::new(), |prefix, s| format!("{prefix}{s}/"))
            })
            .collect(),
    )
}

/// Sorted prefixes without one inside another.
fn minimal(mut prefixes: Vec<String>) -> Vec<String> {
    prefixes.sort();
    prefixes.dedup();
    let all = prefixes.clone();
    prefixes.retain(|p| !all.iter().any(|q| q != p && p.starts_with(q.as_str())));
    prefixes
}

/// A way of getting the changed-file list, in ladder order (ADR 0027).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Listing,
    TreeDiff,
    ScopedTreeDiff,
}

/// What the path globs decided for one pull request head. Stored per PR in
/// the monitoring state; re-read when the head or an input changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathFacts {
    pub head_sha: String,
    /// Base the diff was read against: a retarget changes the files. Facts
    /// saved without one are stale.
    #[serde(default)]
    pub base_sha: String,
    /// The provider reports this many changed files but cannot list them
    /// all: no review runs on this head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete: Option<u64>,
    /// Strategies tried before parking, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tried: Vec<Strategy>,
    /// The repository gate the verdict was taken for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gate: Vec<String>,
    pub admitted: bool,
    /// Secondary assignments with globs, by assignment id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secondaries: BTreeMap<String, SecondaryFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecondaryFacts {
    pub patterns: Vec<String>,
    /// Head of the assignment's last completed review it was compared from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    pub matched: bool,
}

/// The inputs of one PR's facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub gate: Vec<String>,
    pub secondaries: Vec<SecondaryPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondaryPlan {
    pub assignment_id: String,
    pub patterns: Vec<String>,
    /// Head of its last completed review of this PR; `None`: whole PR.
    pub since: Option<String>,
}

impl PathFacts {
    fn incomplete(pull: &PullRequest, count: u64, tried: Vec<Strategy>) -> Self {
        Self {
            head_sha: pull.head_sha.clone(),
            base_sha: pull.base_sha.clone(),
            incomplete: Some(count),
            tried,
            gate: Vec::new(),
            admitted: true,
            secondaries: BTreeMap::new(),
        }
    }

    /// Read for this revision: head and base.
    pub fn at(&self, pull: &PullRequest) -> bool {
        self.head_sha == pull.head_sha && self.base_sha == pull.base_sha
    }

    /// Taken for this revision and these inputs.
    pub fn current(&self, pull: &PullRequest, plan: &Plan) -> bool {
        self.at(pull)
            && (self.incomplete.is_some()
                || self.gate == plan.gate
                    && plan.secondaries.iter().all(|s| {
                        self.secondaries
                            .get(&s.assignment_id)
                            .is_some_and(|f| f.patterns == s.patterns && f.since == s.since)
                    }))
    }

    /// The gate keeps this revision out of the queue.
    pub fn gated_out(&self, pull: &PullRequest) -> bool {
        self.at(pull) && self.incomplete.is_none() && !self.admitted
    }

    /// Changed files the provider cannot list in full at this revision.
    pub fn incomplete_for(&self, pull: &PullRequest) -> Option<u64> {
        self.incomplete.filter(|_| self.at(pull))
    }

    /// The parked count at `head`, for display.
    pub fn incomplete_at(&self, head: &str) -> Option<u64> {
        self.incomplete.filter(|_| self.head_sha == head)
    }

    /// A secondary whose facts for this revision, its current `patterns` and
    /// `since` say no file matched. Facts for other inputs never skip.
    pub fn skips(
        &self,
        pull: &PullRequest,
        assignment_id: &str,
        since: Option<&str>,
        patterns: &[String],
    ) -> bool {
        self.at(pull)
            && self.incomplete.is_none()
            && self.secondaries.get(assignment_id).is_some_and(|f| {
                !f.matched && f.since.as_deref() == since && f.patterns == patterns
            })
    }
}

/// A list the ladder could not complete: the reported count and what it tried.
type Parked = (u64, Vec<Strategy>);

/// The whole-PR list, read at most once, down the strategy ladder.
struct Whole<'a, R: ?Sized> {
    reader: &'a R,
    repository: &'a RemoteRepository,
    pull: &'a PullRequest,
    /// Union scope of every glob in the plan; empty without globs.
    scope: Vec<String>,
    files: Option<Result<Vec<ChangedFile>, Parked>>,
    /// The list came from a scoped walk: it decides globs, but a review needs
    /// every file, so an admitted PR still parks with this.
    partial: Option<Parked>,
}

impl<R: PullRequestReader + ?Sized> Whole<'_, R> {
    fn get(&mut self) -> Result<Result<&[ChangedFile], Parked>, ConnectionError> {
        if self.files.is_none() {
            let files = match self
                .reader
                .changed_file_listing(self.repository, self.pull, true)?
            {
                Listing::Complete(files) => Ok(files),
                Listing::Incomplete(count) => self.climb(count, false)?,
                Listing::Visible => return Err(ConnectionError::InvalidResponse),
            };
            self.files = Some(files);
        }
        Ok(match &self.files {
            Some(Ok(files)) => Ok(files),
            Some(Err(parked)) => Err(parked.clone()),
            None => Ok(&[]),
        })
    }

    /// Steps 2-3 after an incomplete listing of `count` files. `visible_only`:
    /// no glob reads the list, so only a complete diff counts.
    fn climb(
        &mut self,
        count: u64,
        visible_only: bool,
    ) -> Result<Result<Vec<ChangedFile>, Parked>, ConnectionError> {
        let mut tried = vec![Strategy::Listing];
        let base = self.reader.merge_base(self.repository, self.pull)?;
        let head = self.pull.head_sha.as_str();
        if let TreeDiff::Complete(files) =
            self.reader.tree_diff(self.repository, &base, head, &[])?
        {
            return Ok(Ok(files));
        }
        tried.push(Strategy::TreeDiff);
        if !visible_only && !self.scope.is_empty() {
            if let TreeDiff::Complete(files) =
                self.reader
                    .tree_diff(self.repository, &base, head, &self.scope)?
            {
                tried.push(Strategy::ScopedTreeDiff);
                self.partial = Some((count, tried));
                return Ok(Ok(files));
            }
            tried.push(Strategy::ScopedTreeDiff);
        }
        Ok(Err((count, tried)))
    }
}

/// Reads what `plan` needs for `pull`'s head, reusing `previous` facts of
/// the same head. Every new head checks that all changed files are visible
/// (cheap count first); the full list is read only when a glob needs it.
/// An incomplete listing climbs the ladder (tree diff, scoped tree diff)
/// before parking. A scoped list can keep a PR out, but an admitted PR needs
/// the complete list its review reads, so it parks. Provider errors (rate limits, network, raced reads)
/// propagate: they never decide, skip or park.
pub fn read_facts<R: PullRequestReader + ?Sized>(
    reader: &R,
    repository: &RemoteRepository,
    pull: &PullRequest,
    plan: &Plan,
    previous: Option<&PathFacts>,
) -> Result<PathFacts, ConnectionError> {
    let head = pull.head_sha.as_str();
    // Only an admitted verdict proves the list was complete: a gate-out may
    // rest on a scoped walk, so it never skips the completeness read.
    let previous = previous.filter(|p| p.at(pull) && p.admitted);
    if let Some(previous) = previous.filter(|p| p.incomplete.is_some()) {
        return Ok(previous.clone());
    }
    // One scoped list serves every glob list: the union of their scopes.
    let scope = minimal(
        std::iter::once(&plan.gate)
            .chain(plan.secondaries.iter().map(|s| &s.patterns))
            .flat_map(|patterns| self::scope(patterns))
            .collect(),
    );
    let mut whole = Whole {
        reader,
        repository,
        pull,
        scope,
        files: None,
        partial: None,
    };
    let parked = |(count, tried): Parked| PathFacts::incomplete(pull, count, tried);
    if previous.is_none() {
        let globbed = !plan.gate.is_empty() || !plan.secondaries.is_empty();
        match reader.changed_file_listing(repository, pull, globbed)? {
            Listing::Complete(files) => whole.files = Some(Ok(files)),
            Listing::Visible => {}
            Listing::Incomplete(count) => match whole.climb(count, !globbed)? {
                Ok(files) => whole.files = Some(Ok(files)),
                Err(stuck) => return Ok(parked(stuck)),
            },
        }
    }
    let admitted = if plan.gate.is_empty() {
        true
    } else if let Some(previous) = previous.filter(|p| p.gate == plan.gate) {
        previous.admitted
    } else {
        match whole.get()? {
            Ok(files) => matches(&plan.gate, files),
            Err(stuck) => return Ok(parked(stuck)),
        }
    };
    let mut secondaries = BTreeMap::new();
    for secondary in &plan.secondaries {
        let reused = previous
            .and_then(|p| p.secondaries.get(&secondary.assignment_id))
            .filter(|f| f.patterns == secondary.patterns && f.since == secondary.since);
        let incremental = match (reused, secondary.since.as_deref()) {
            (Some(_), _) | (None, None) => None,
            // Reviewed this very head: nothing changed since.
            (None, Some(old)) if old == head => Some(Vec::new()),
            (None, Some(old)) => match reader.changed_files_since(repository, pull, old)? {
                Since::Files(files) => Some(files),
                Since::WholePullRequest => None,
                Since::Capped => {
                    match reader.tree_diff(repository, old, head, &[])? {
                        TreeDiff::Complete(files) => Some(files),
                        TreeDiff::Truncated => match reader.tree_diff(
                            repository,
                            old,
                            head,
                            &self::scope(&secondary.patterns),
                        )? {
                            TreeDiff::Complete(files) => Some(files),
                            // The whole PR still decides, conservatively.
                            TreeDiff::Truncated => None,
                        },
                    }
                }
            },
        };
        let matched = match (reused, incremental) {
            (Some(facts), _) => facts.matched,
            (None, Some(files)) => matches(&secondary.patterns, &files),
            (None, None) => match whole.get()? {
                Ok(files) => matches(&secondary.patterns, files),
                Err(stuck) => return Ok(parked(stuck)),
            },
        };
        secondaries.insert(
            secondary.assignment_id.clone(),
            SecondaryFacts {
                patterns: secondary.patterns.clone(),
                since: secondary.since.clone(),
                matched,
            },
        );
    }
    // Admitted on a list a review cannot see in full: no review starts.
    if let Some(stuck) = whole.partial.filter(|_| admitted) {
        return Ok(parked(stuck));
    }
    Ok(PathFacts {
        head_sha: head.into(),
        base_sha: pull.base_sha.clone(),
        incomplete: None,
        tried: Vec::new(),
        gate: plan.gate.clone(),
        admitted,
        secondaries,
    })
}

#[cfg(test)]
mod tests;

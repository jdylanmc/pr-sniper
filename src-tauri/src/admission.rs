//! Ordered, per-repository pull request admission rules.
//!
//! When a repository has no `admission`, the legacy watched-author,
//! reviewer-assignment and mention behavior applies unchanged.

use crate::policy::WatchedIdentity;
use crate::provider::{ChangedFile, PullRequest};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_VALUES_PER_RULE: usize = 100;
pub const MAX_PATTERN_LENGTH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionMode {
    All,
    Filtered,
    /// Only native explicit intake may admit new PRs. Already admitted PRs
    /// retain heartbeat/iteration monitoring.
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionRuleKind {
    Assigned,
    Mentioned,
    IncludeUsers,
    ExcludeUsers,
    IncludePaths,
    ExcludePaths,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRule {
    pub kind: AdmissionRuleKind,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub users: Option<Vec<WatchedIdentity>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionConfig {
    pub mode: AdmissionMode,
    pub rules: Vec<AdmissionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// `None`: mode "all" admits without a deciding rule.
    Admit(Option<AdmissionRuleKind>),
    Reject(Option<AdmissionRuleKind>),
    /// A reachable, enabled path rule needs the changed-file list first.
    NeedFiles,
}

impl Decision {
    pub fn admitted(&self) -> bool {
        matches!(self, Self::Admit(_))
    }
}

impl AdmissionRule {
    fn new(kind: AdmissionRuleKind, enabled: bool) -> Self {
        let users = matches!(
            kind,
            AdmissionRuleKind::IncludeUsers | AdmissionRuleKind::ExcludeUsers
        )
        .then(Vec::new);
        let paths = matches!(
            kind,
            AdmissionRuleKind::IncludePaths | AdmissionRuleKind::ExcludePaths
        )
        .then(Vec::new);
        Self {
            kind,
            enabled,
            users,
            paths,
        }
    }

    #[cfg(test)]
    pub(crate) fn default_for_test(kind: AdmissionRuleKind) -> Self {
        Self::new(kind, true)
    }

    fn users(&self) -> &[WatchedIdentity] {
        self.users.as_deref().unwrap_or(&[])
    }

    fn paths(&self) -> &[String] {
        self.paths.as_deref().unwrap_or(&[])
    }
}

impl Default for AdmissionConfig {
    fn default() -> Self {
        Self {
            mode: AdmissionMode::All,
            rules: vec![
                AdmissionRule::new(AdmissionRuleKind::Assigned, true),
                AdmissionRule::new(AdmissionRuleKind::Mentioned, true),
                AdmissionRule::new(AdmissionRuleKind::IncludeUsers, false),
                AdmissionRule::new(AdmissionRuleKind::ExcludeUsers, false),
                AdmissionRule::new(AdmissionRuleKind::IncludePaths, false),
                AdmissionRule::new(AdmissionRuleKind::ExcludePaths, false),
            ],
        }
    }
}

impl AdmissionConfig {
    /// Materializes legacy values. Used by both the one-time migration and
    /// by seeding new repositories.
    pub fn from_legacy(watched_authors: &[WatchedIdentity], reviewer_assignment: bool) -> Self {
        if watched_authors.is_empty() {
            return Self::default();
        }
        let mut include = AdmissionRule::new(AdmissionRuleKind::IncludeUsers, true);
        include.users = Some(watched_authors.to_vec());
        Self {
            mode: AdmissionMode::Filtered,
            rules: vec![
                include,
                AdmissionRule::new(AdmissionRuleKind::Assigned, reviewer_assignment),
                AdmissionRule::new(AdmissionRuleKind::Mentioned, true),
                AdmissionRule::new(AdmissionRuleKind::ExcludeUsers, false),
                AdmissionRule::new(AdmissionRuleKind::IncludePaths, false),
                AdmissionRule::new(AdmissionRuleKind::ExcludePaths, false),
            ],
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut kinds = HashSet::new();
        for rule in &self.rules {
            if !kinds.insert(rule.kind) {
                return Err("Each admission rule kind can appear at most once.".into());
            }
            let takes_users = matches!(
                rule.kind,
                AdmissionRuleKind::IncludeUsers | AdmissionRuleKind::ExcludeUsers
            );
            let takes_paths = matches!(
                rule.kind,
                AdmissionRuleKind::IncludePaths | AdmissionRuleKind::ExcludePaths
            );
            if rule.users.is_some() && !takes_users || rule.paths.is_some() && !takes_paths {
                return Err("Admission rule values do not belong to this rule kind.".into());
            }
            validate_users(rule.users())?;
            validate_paths(rule.paths())?;
        }
        Ok(())
    }

    /// The enabled include-users rule, for display and legacy-shaped evidence.
    pub fn watched_authors(&self) -> Vec<WatchedIdentity> {
        let mut authors: Vec<_> = self
            .rules
            .iter()
            .filter(|rule| rule.enabled && rule.kind == AdmissionRuleKind::IncludeUsers)
            .flat_map(|rule| rule.users().iter().cloned())
            .collect();
        authors.sort_by(|left, right| left.id.cmp(&right.id));
        authors.dedup_by(|left, right| left.id == right.id);
        authors
    }

    /// Whether an enabled assigned rule matches (used for baseline bypass).
    pub fn assigned_matches(&self, account_id: &str, pull: &PullRequest) -> bool {
        self.mode != AdmissionMode::Explicit
            && self.rules.iter().any(|rule| {
                rule.enabled
                    && rule.kind == AdmissionRuleKind::Assigned
                    && is_requested_reviewer(account_id, pull)
            })
    }

    pub fn include_users_matches(&self, pull: &PullRequest) -> bool {
        self.rules.iter().any(|rule| {
            rule.enabled && rule.kind == AdmissionRuleKind::IncludeUsers && author_in(rule, pull)
        })
    }

    pub fn mentioned_enabled(&self) -> bool {
        self.mode != AdmissionMode::Explicit
            && self
                .rules
                .iter()
                .any(|rule| rule.enabled && rule.kind == AdmissionRuleKind::Mentioned)
    }

    /// `files` is `None` when the changed-file list has not been loaded.
    pub fn decide(
        &self,
        account_id: &str,
        pull: &PullRequest,
        files: Option<&[ChangedFile]>,
    ) -> Decision {
        if self.mode == AdmissionMode::Explicit {
            return Decision::Reject(None);
        }
        if self.mode == AdmissionMode::All {
            return Decision::Admit(None);
        }
        for rule in self.rules.iter().filter(|rule| rule.enabled) {
            match rule.kind {
                AdmissionRuleKind::Assigned if is_requested_reviewer(account_id, pull) => {
                    return Decision::Admit(Some(rule.kind))
                }
                AdmissionRuleKind::Mentioned if pull.mentioned => {
                    return Decision::Admit(Some(rule.kind))
                }
                AdmissionRuleKind::IncludeUsers if author_in(rule, pull) => {
                    return Decision::Admit(Some(rule.kind))
                }
                AdmissionRuleKind::ExcludeUsers if author_in(rule, pull) => {
                    return Decision::Reject(Some(rule.kind))
                }
                AdmissionRuleKind::IncludePaths | AdmissionRuleKind::ExcludePaths => {
                    if rule.paths().is_empty() {
                        continue;
                    }
                    let Some(files) = files else {
                        return Decision::NeedFiles;
                    };
                    if rule.kind == AdmissionRuleKind::IncludePaths {
                        if files
                            .iter()
                            .any(|file| file_names(file).any(|name| matches_any(rule, name)))
                        {
                            return Decision::Admit(Some(rule.kind));
                        }
                    } else if !files.is_empty()
                        && files
                            .iter()
                            .all(|file| file_names(file).all(|name| matches_any(rule, name)))
                    {
                        return Decision::Reject(Some(rule.kind));
                    }
                }
                _ => {}
            }
        }
        Decision::Reject(None)
    }

    /// Decision using the pull request's own file list.
    pub fn admits(&self, account_id: &str, pull: &PullRequest) -> bool {
        self.decide(account_id, pull, Some(&pull.files)).admitted()
    }

    /// True when the changed-file list is needed (and not yet loaded) before
    /// this pull request can be decided.
    pub fn needs_changed_files(&self, account_id: &str, pull: &PullRequest) -> bool {
        pull.files.is_empty() && self.decide(account_id, pull, None) == Decision::NeedFiles
    }
}

fn is_requested_reviewer(account_id: &str, pull: &PullRequest) -> bool {
    pull.requested_reviewers
        .iter()
        .any(|reviewer| reviewer.id == account_id)
}

fn author_in(rule: &AdmissionRule, pull: &PullRequest) -> bool {
    pull.author
        .as_ref()
        .is_some_and(|author| rule.users().iter().any(|user| user.id == author.id))
}

fn file_names(file: &ChangedFile) -> impl Iterator<Item = &str> {
    std::iter::once(file.path.as_str()).chain(file.previous_path.as_deref())
}

fn matches_any(rule: &AdmissionRule, name: &str) -> bool {
    rule.paths()
        .iter()
        .any(|pattern| glob_matches(pattern, name))
}

fn validate_users(users: &[WatchedIdentity]) -> Result<(), String> {
    if users.len() > MAX_VALUES_PER_RULE {
        return Err(format!(
            "An admission rule can list at most {MAX_VALUES_PER_RULE} users."
        ));
    }
    let mut ids = HashSet::new();
    for identity in users {
        let valid_id = identity
            .id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0 && id.to_string() == identity.id);
        if valid_id.is_none() || !ids.insert(&identity.id) {
            return Err(
                "Watched GitHub account IDs must be unique positive decimal numbers.".into(),
            );
        }
        if identity.login.trim().is_empty() || identity.login.chars().any(char::is_whitespace) {
            return Err("Each watched GitHub identity needs a nonempty login display label without whitespace.".into());
        }
        if crate::policy::validate_configuration_text(&identity.login).is_err() {
            return Err("Credentials and tokens must not be stored in configuration.".into());
        }
    }
    Ok(())
}

fn validate_paths(paths: &[String]) -> Result<(), String> {
    if paths.len() > MAX_VALUES_PER_RULE {
        return Err(format!(
            "An admission rule can list at most {MAX_VALUES_PER_RULE} path patterns."
        ));
    }
    for pattern in paths {
        if pattern.is_empty()
            || pattern.chars().count() > MAX_PATTERN_LENGTH
            || pattern.starts_with('/')
            || pattern.chars().any(char::is_control)
            || pattern.split('/').any(|segment| segment == "..")
        {
            return Err(format!(
                "Path patterns must be 1-{MAX_PATTERN_LENGTH} characters, repository-relative, without '..' or control characters."
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Literal(char),
    Question,
    Star,
    /// `**`: any characters, including '/'.
    Deep,
    /// `**/`: zero or more whole directories.
    DeepSlash,
}

fn tokenize(pattern: &str) -> Vec<Token> {
    // A trailing '/' is a directory prefix.
    let pattern = pattern
        .strip_suffix('/')
        .map(|directory| format!("{directory}/**"))
        .unwrap_or_else(|| pattern.to_string());
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' if chars.get(index + 1) == Some(&'*') => {
                let at_segment_start = index == 0 || chars[index - 1] == '/';
                if at_segment_start && chars.get(index + 2) == Some(&'/') {
                    tokens.push(Token::DeepSlash);
                    index += 3;
                } else {
                    tokens.push(Token::Deep);
                    index += 2;
                }
            }
            '*' => {
                tokens.push(Token::Star);
                index += 1;
            }
            '?' => {
                tokens.push(Token::Question);
                index += 1;
            }
            character => {
                tokens.push(Token::Literal(character));
                index += 1;
            }
        }
    }
    tokens
}

/// Case-sensitive match of a repository-relative path. A pattern ending in
/// '/' is a directory prefix; otherwise `*`, `**` and `?` are the only syntax.
pub fn glob_matches(pattern: &str, path: &str) -> bool {
    let tokens = tokenize(pattern);
    let text: Vec<char> = path.chars().collect();
    let width = text.len() + 1;
    // matches[token * width + position]: does tokens[token..] match text[position..]?
    let mut matches = vec![false; (tokens.len() + 1) * width];
    matches[tokens.len() * width + text.len()] = true;
    for token in (0..tokens.len()).rev() {
        for position in (0..=text.len()).rev() {
            let next = |t: usize, p: usize| matches[t * width + p];
            let result = match tokens[token] {
                Token::Literal(c) => {
                    position < text.len() && text[position] == c && next(token + 1, position + 1)
                }
                Token::Question => {
                    position < text.len() && text[position] != '/' && next(token + 1, position + 1)
                }
                Token::Star => {
                    next(token + 1, position)
                        || position < text.len()
                            && text[position] != '/'
                            && next(token, position + 1)
                }
                Token::Deep => {
                    next(token + 1, position) || position < text.len() && next(token, position + 1)
                }
                Token::DeepSlash => {
                    next(token + 1, position)
                        || text[position..]
                            .iter()
                            .position(|c| *c == '/')
                            .is_some_and(|offset| next(token, position + offset + 1))
                }
            };
            matches[token * width + position] = result;
        }
    }
    matches[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{metadata::Lifecycle, Identity};

    fn identity(id: &str) -> WatchedIdentity {
        WatchedIdentity {
            id: id.into(),
            login: format!("user{id}"),
        }
    }

    fn file(path: &str, previous: Option<&str>) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            previous_path: previous.map(str::to_string),
            status: if previous.is_some() {
                "renamed"
            } else {
                "modified"
            }
            .into(),
            additions: 1,
            deletions: 0,
            changes: 1,
            sha: "a".repeat(40),
        }
    }

    fn pull(
        author: &str,
        reviewers: &[&str],
        mentioned: bool,
        files: Vec<ChangedFile>,
    ) -> PullRequest {
        PullRequest {
            mentioned,
            id: "1".into(),
            number: 1,
            title: "t".into(),
            author: Some(Identity {
                id: author.into(),
                login: format!("user{author}"),
            }),
            requested_reviewers: reviewers
                .iter()
                .map(|id| Identity {
                    id: (*id).into(),
                    login: format!("user{id}"),
                })
                .collect(),
            requested_teams: Vec::new(),
            state: Lifecycle::Open,
            draft: false,
            head_sha: "b".repeat(40),
            base_sha: "c".repeat(40),
            head_repository_id: None,
            base_repository_id: "100".into(),
            updated_at: "2026-10-01T00:00:00Z".into(),
            files,
        }
    }

    fn rule(kind: AdmissionRuleKind, enabled: bool) -> AdmissionRule {
        AdmissionRule::new(kind, enabled)
    }

    fn users(kind: AdmissionRuleKind, ids: &[&str]) -> AdmissionRule {
        let mut rule = rule(kind, true);
        rule.users = Some(ids.iter().map(|id| identity(id)).collect());
        rule
    }

    fn paths(kind: AdmissionRuleKind, patterns: &[&str]) -> AdmissionRule {
        let mut rule = rule(kind, true);
        rule.paths = Some(patterns.iter().map(|p| p.to_string()).collect());
        rule
    }

    fn filtered(rules: Vec<AdmissionRule>) -> AdmissionConfig {
        AdmissionConfig {
            mode: AdmissionMode::Filtered,
            rules,
        }
    }

    const ME: &str = "22";

    #[test]
    fn glob_syntax() {
        for (pattern, path, expected) in [
            ("src/", "src/a.rs", true),
            ("src/", "src/a/b.rs", true),
            ("src/", "srcx/a.rs", false),
            ("src/", "lib/src/a.rs", false),
            ("src/a/", "src/a/b/c", true),
            ("*.md", "README.md", true),
            ("*.md", "docs/README.md", false),
            ("**/*.md", "README.md", true),
            ("**/*.md", "docs/deep/README.md", true),
            ("**/*.md", "README.txt", false),
            ("docs/**", "docs/a/b.md", true),
            ("docs/**", "docs/", true),
            ("docs/**", "doc/a", false),
            ("src/**/test.rs", "src/test.rs", true),
            ("src/**/test.rs", "src/a/b/test.rs", true),
            ("src/**/test.rs", "src/a/b/atest.rs", false),
            ("src/*/mod.rs", "src/a/mod.rs", true),
            ("src/*/mod.rs", "src/a/b/mod.rs", false),
            ("a?c", "abc", true),
            ("a?c", "a/c", false),
            ("a?c", "ac", false),
            ("Makefile", "Makefile", true),
            ("Makefile", "makefile", false),
            ("Makefile", "sub/Makefile", false),
            ("**/Makefile", "sub/Makefile", true),
            ("a.b", "axb", false),
            ("a+(b)", "a+(b)", true),
            ("**", "anything/at/all", true),
            ("*", "a/b", false),
        ] {
            assert_eq!(glob_matches(pattern, path), expected, "{pattern} vs {path}");
        }
    }

    #[test]
    fn each_rule_kind_decides() {
        let p = pull("11", &[ME], false, vec![file("src/a.rs", None)]);
        let assigned = filtered(vec![rule(AdmissionRuleKind::Assigned, true)]);
        assert_eq!(
            assigned.decide(ME, &p, Some(&p.files)),
            Decision::Admit(Some(AdmissionRuleKind::Assigned))
        );
        assert!(!assigned.admits("99", &p));

        let mentioned = filtered(vec![rule(AdmissionRuleKind::Mentioned, true)]);
        assert!(!mentioned.admits(ME, &p));
        assert!(mentioned.admits(ME, &pull("11", &[], true, vec![])));

        let include = filtered(vec![users(AdmissionRuleKind::IncludeUsers, &["11"])]);
        assert!(include.admits(ME, &p));
        assert!(!include.admits(ME, &pull("12", &[], false, vec![])));

        let exclude = filtered(vec![
            users(AdmissionRuleKind::ExcludeUsers, &["11"]),
            rule(AdmissionRuleKind::Assigned, true),
        ]);
        assert_eq!(
            exclude.decide(ME, &p, Some(&p.files)),
            Decision::Reject(Some(AdmissionRuleKind::ExcludeUsers))
        );

        let include_paths = filtered(vec![paths(AdmissionRuleKind::IncludePaths, &["src/"])]);
        assert!(include_paths.admits(ME, &p));
        assert!(!include_paths.admits(ME, &pull("11", &[], false, vec![file("docs/a.md", None)])));

        let exclude_paths = filtered(vec![
            paths(AdmissionRuleKind::ExcludePaths, &["docs/"]),
            rule(AdmissionRuleKind::Assigned, true),
        ]);
        let docs = pull("11", &[ME], false, vec![file("docs/a.md", None)]);
        assert_eq!(
            exclude_paths.decide(ME, &docs, Some(&docs.files)),
            Decision::Reject(Some(AdmissionRuleKind::ExcludePaths))
        );
    }

    #[test]
    fn no_matching_rule_rejects_and_empty_rules_reject() {
        let p = pull("11", &[], false, vec![file("a", None)]);
        assert_eq!(
            filtered(vec![]).decide(ME, &p, Some(&p.files)),
            Decision::Reject(None)
        );
        assert!(!filtered(vec![rule(AdmissionRuleKind::Assigned, true)]).admits(ME, &p));
    }

    #[test]
    fn mode_all_admits_everything_and_preserves_rules() {
        let mut config = filtered(vec![users(AdmissionRuleKind::ExcludeUsers, &["11"])]);
        let p = pull("11", &[], false, vec![]);
        assert!(!config.admits(ME, &p));
        config.mode = AdmissionMode::All;
        assert!(config.admits(ME, &p));
        assert_eq!(config.rules.len(), 1);
        assert!(!config.needs_changed_files(ME, &p));
        let mut anonymous = p.clone();
        anonymous.author = None;
        assert!(config.admits(ME, &anonymous));
    }

    #[test]
    fn disabled_rules_are_skipped() {
        let mut exclude = users(AdmissionRuleKind::ExcludeUsers, &["11"]);
        exclude.enabled = false;
        let config = filtered(vec![exclude, rule(AdmissionRuleKind::Mentioned, true)]);
        assert!(config.admits(ME, &pull("11", &[], true, vec![])));
        let mut include = users(AdmissionRuleKind::IncludeUsers, &["11"]);
        include.enabled = false;
        assert!(!filtered(vec![include]).admits(ME, &pull("11", &[], false, vec![])));
        let mut path = paths(AdmissionRuleKind::ExcludePaths, &["**"]);
        path.enabled = false;
        let p = pull("11", &[], false, vec![file("a", None)]);
        assert!(!filtered(vec![path]).needs_changed_files(ME, &pull("11", &[], false, vec![])));
        assert!(!filtered(vec![rule(AdmissionRuleKind::Mentioned, true)]).admits(ME, &p));
    }

    #[test]
    fn first_match_wins_across_orderings() {
        let author = "11";
        let p = pull(author, &[ME], true, vec![file("docs/a.md", None)]);
        let make = |kinds: &[AdmissionRuleKind]| {
            filtered(
                kinds
                    .iter()
                    .map(|kind| match kind {
                        AdmissionRuleKind::ExcludeUsers | AdmissionRuleKind::IncludeUsers => {
                            users(*kind, &[author])
                        }
                        AdmissionRuleKind::ExcludePaths | AdmissionRuleKind::IncludePaths => {
                            paths(*kind, &["docs/"])
                        }
                        _ => rule(*kind, true),
                    })
                    .collect(),
            )
        };
        use AdmissionRuleKind::*;
        // Every exclude/admit pair: whichever comes first decides.
        for (admit, exclude) in [
            (Assigned, ExcludeUsers),
            (Mentioned, ExcludeUsers),
            (IncludeUsers, ExcludeUsers),
            (IncludePaths, ExcludeUsers),
            (Assigned, ExcludePaths),
            (Mentioned, ExcludePaths),
            (IncludeUsers, ExcludePaths),
            (IncludePaths, ExcludePaths),
        ] {
            assert_eq!(
                make(&[admit, exclude]).decide(ME, &p, Some(&p.files)),
                Decision::Admit(Some(admit)),
                "{admit:?} before {exclude:?}"
            );
            assert_eq!(
                make(&[exclude, admit]).decide(ME, &p, Some(&p.files)),
                Decision::Reject(Some(exclude)),
                "{exclude:?} before {admit:?}"
            );
        }
        // A rejecting rule that does not match does not stop later admits.
        let other = pull("12", &[], true, vec![file("src/a", None)]);
        assert!(make(&[ExcludeUsers, ExcludePaths, Mentioned]).admits(ME, &other));
    }

    #[test]
    fn exclude_paths_requires_every_file_and_at_least_one() {
        let config = filtered(vec![
            paths(AdmissionRuleKind::ExcludePaths, &["docs/", "*.md"]),
            rule(AdmissionRuleKind::Mentioned, true),
        ]);
        let all_docs = pull(
            "11",
            &[],
            true,
            vec![file("docs/a.txt", None), file("README.md", None)],
        );
        assert_eq!(
            config.decide(ME, &all_docs, Some(&all_docs.files)),
            Decision::Reject(Some(AdmissionRuleKind::ExcludePaths))
        );
        let mixed = pull(
            "11",
            &[],
            true,
            vec![file("docs/a.txt", None), file("src/a.rs", None)],
        );
        assert!(config.admits(ME, &mixed));
        // Vacuous truth must not exclude PRs without files.
        let empty = pull("11", &[], true, vec![]);
        assert!(config.admits(ME, &empty));
    }

    #[test]
    fn renames_match_on_either_path() {
        let rename = pull(
            "11",
            &[],
            false,
            vec![file("docs/new.md", Some("src/old.rs"))],
        );
        let include = filtered(vec![paths(AdmissionRuleKind::IncludePaths, &["src/"])]);
        assert!(include.admits(ME, &rename));
        let include_new = filtered(vec![paths(AdmissionRuleKind::IncludePaths, &["docs/"])]);
        assert!(include_new.admits(ME, &rename));

        let exclude = filtered(vec![
            paths(AdmissionRuleKind::ExcludePaths, &["docs/"]),
            rule(AdmissionRuleKind::Mentioned, true),
        ]);
        let moved_out_of_docs = pull(
            "11",
            &[],
            true,
            vec![file("docs/new.md", Some("src/old.rs"))],
        );
        // The previous path is not excluded, so the rename is not all-excluded.
        assert!(exclude.admits(ME, &moved_out_of_docs));
        let within_docs = pull(
            "11",
            &[],
            true,
            vec![file("docs/new.md", Some("docs/old.md"))],
        );
        assert!(!exclude.admits(ME, &within_docs));
    }

    #[test]
    fn empty_file_list_matches_no_path_rule() {
        let p = pull("11", &[], true, vec![]);
        let config = filtered(vec![
            paths(AdmissionRuleKind::IncludePaths, &["**"]),
            paths(AdmissionRuleKind::ExcludePaths, &["**"]),
        ]);
        assert_eq!(
            config.decide(ME, &p, Some(&p.files)),
            Decision::Reject(None)
        );
    }

    #[test]
    fn changed_files_are_needed_only_when_a_path_rule_is_reached() {
        use AdmissionRuleKind::*;
        let p = pull("11", &[ME], false, vec![]);
        let reached = filtered(vec![rule(Mentioned, true), paths(IncludePaths, &["src/"])]);
        assert!(reached.needs_changed_files(ME, &p));
        // An earlier decisive rule means no fetch is needed.
        let decided = filtered(vec![rule(Assigned, true), paths(IncludePaths, &["src/"])]);
        assert!(!decided.needs_changed_files(ME, &p));
        let excluded = filtered(vec![
            users(ExcludeUsers, &["11"]),
            paths(IncludePaths, &["src/"]),
        ]);
        assert!(!excluded.needs_changed_files(ME, &p));
        // Loaded files are never refetched; empty path lists never need files.
        let loaded = pull("11", &[], false, vec![file("src/a", None)]);
        assert!(!reached.needs_changed_files(ME, &loaded));
        assert!(!filtered(vec![paths(ExcludePaths, &[])]).needs_changed_files(ME, &p));
        assert!(!AdmissionConfig::default().needs_changed_files(ME, &p));
    }

    #[test]
    fn legacy_materialization() {
        let all = AdmissionConfig::from_legacy(&[], false);
        assert_eq!(all, AdmissionConfig::default());
        let kinds: Vec<_> = all.rules.iter().map(|r| (r.kind, r.enabled)).collect();
        use AdmissionRuleKind::*;
        assert_eq!(
            kinds,
            [
                (Assigned, true),
                (Mentioned, true),
                (IncludeUsers, false),
                (ExcludeUsers, false),
                (IncludePaths, false),
                (ExcludePaths, false)
            ]
        );
        let watched = AdmissionConfig::from_legacy(&[identity("11")], false);
        assert_eq!(watched.mode, AdmissionMode::Filtered);
        let kinds: Vec<_> = watched.rules.iter().map(|r| (r.kind, r.enabled)).collect();
        assert_eq!(
            kinds,
            [
                (IncludeUsers, true),
                (Assigned, false),
                (Mentioned, true),
                (ExcludeUsers, false),
                (IncludePaths, false),
                (ExcludePaths, false)
            ]
        );
        assert_eq!(watched.rules[0].users, Some(vec![identity("11")]));
        assert!(AdmissionConfig::from_legacy(&[identity("11")], true).rules[1].enabled);
        watched.validate().unwrap();
        all.validate().unwrap();
    }

    #[test]
    fn serde_shape_and_unknown_fields() {
        let config: AdmissionConfig = serde_json::from_value(serde_json::json!({
            "mode": "filtered",
            "rules": [
                {"kind": "include_users", "enabled": true, "users": [{"id": "5", "login": "a"}]},
                {"kind": "exclude_paths", "enabled": false, "paths": ["docs/"]},
                {"kind": "assigned", "enabled": true}
            ]
        }))
        .unwrap();
        config.validate().unwrap();
        let round: AdmissionConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(round, config);
        let json = serde_json::to_value(&config).unwrap();
        assert!(json["rules"][2].get("users").is_none());
        for bad in [
            serde_json::json!({"mode": "all", "rules": [], "extra": 1}),
            serde_json::json!({"mode": "some", "rules": []}),
            serde_json::json!({"mode": "all", "rules": [{"kind": "other", "enabled": true}]}),
            serde_json::json!({"mode": "all", "rules": [{"kind": "assigned", "enabled": true, "x": 1}]}),
            serde_json::json!({"mode": "all", "rules": [{"kind": "assigned"}]}),
            serde_json::json!({"mode": "all"}),
        ] {
            assert!(serde_json::from_value::<AdmissionConfig>(bad).is_err());
        }
    }

    #[test]
    fn validation_rejects_bad_input() {
        use AdmissionRuleKind::*;
        let bad_paths = [
            "",
            "/abs",
            "a/../b",
            "..",
            "../x",
            "a\u{0}b",
            "tab\there",
            &"x".repeat(257),
        ];
        for pattern in bad_paths {
            assert!(
                filtered(vec![paths(IncludePaths, &[pattern])])
                    .validate()
                    .is_err(),
                "{pattern:?}"
            );
        }
        filtered(vec![paths(
            IncludePaths,
            &[&"x".repeat(256), "..x", "a..b/"],
        )])
        .validate()
        .unwrap();
        let many: Vec<_> = (0..101).map(|i| format!("p{i}/")).collect();
        let many: Vec<_> = many.iter().map(String::as_str).collect();
        assert!(filtered(vec![paths(ExcludePaths, &many)])
            .validate()
            .is_err());
        assert!(filtered(vec![paths(ExcludePaths, &many[..100])])
            .validate()
            .is_ok());
        let ids: Vec<_> = (1..=101).map(|i| i.to_string()).collect();
        let ids: Vec<_> = ids.iter().map(String::as_str).collect();
        assert!(filtered(vec![users(IncludeUsers, &ids)])
            .validate()
            .is_err());
        assert!(filtered(vec![users(IncludeUsers, &ids[..100])])
            .validate()
            .is_ok());
        for id in ["0", "01", "-1", "x", ""] {
            assert!(filtered(vec![users(IncludeUsers, &[id])])
                .validate()
                .is_err());
        }
        assert!(filtered(vec![users(IncludeUsers, &["5", "5"])])
            .validate()
            .is_err());
        let mut blank = users(IncludeUsers, &["5"]);
        blank.users.as_mut().unwrap()[0].login = "a b".into();
        assert!(filtered(vec![blank]).validate().is_err());
        assert!(filtered(vec![rule(Assigned, true), rule(Assigned, false)])
            .validate()
            .is_err());
        let mut misplaced = rule(Assigned, true);
        misplaced.users = Some(vec![]);
        assert!(filtered(vec![misplaced]).validate().is_err());
        let mut misplaced = rule(IncludeUsers, true);
        misplaced.paths = Some(vec![]);
        assert!(filtered(vec![misplaced]).validate().is_err());
    }

    #[test]
    fn glob_matching_is_linear_on_adversarial_patterns() {
        let pattern = "*a*a*a*a*a*a*a*a*a*a*a*a*b";
        let path = "a".repeat(200);
        assert!(!glob_matches(pattern, &path));
    }
}

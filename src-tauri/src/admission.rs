//! Ordered, per-repository pull request admission rules.
//!
//! When a repository has no `admission`, the legacy watched-author,
//! reviewer-assignment and mention behavior applies unchanged. Path scoping
//! is the separate repository gate (`crate::paths`, ADR 0027).

use crate::policy::WatchedIdentity;
use crate::provider::PullRequest;
use crate::storage::ProviderId;
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
    /// Legacy: read only so the load-time migration can move it into the
    /// repository paths gate. Saves reject it.
    IncludePaths,
    /// Legacy, like `IncludePaths`.
    ExcludePaths,
}

impl AdmissionRuleKind {
    /// A legacy path rule, migrated into `Repository::paths`.
    pub fn legacy_path(self) -> bool {
        matches!(self, Self::IncludePaths | Self::ExcludePaths)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRule {
    pub kind: AdmissionRuleKind,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub users: Option<Vec<WatchedIdentity>>,
    /// Legacy path rule values; see `AdmissionRuleKind::IncludePaths`.
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
        Self {
            kind,
            enabled,
            users,
            paths: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn default_for_test(kind: AdmissionRuleKind) -> Self {
        Self::new(kind, true)
    }

    fn users(&self) -> &[WatchedIdentity] {
        self.users.as_deref().unwrap_or(&[])
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
            ],
        }
    }

    /// Removes the legacy path rules: enabled include patterns, then enabled
    /// excludes as `!pattern`, in rule order. A Filtered config whose only
    /// enabled rules were path rules, at least one an include, becomes All:
    /// the gate admits what that include admitted. Disabled path rules
    /// migrate to nothing. `None` when there was no path rule.
    pub fn take_legacy_paths(&mut self) -> Option<Vec<String>> {
        if !self.rules.iter().any(|rule| rule.kind.legacy_path()) {
            return None;
        }
        let enabled = |kind| {
            self.rules
                .iter()
                .filter(move |rule| rule.enabled && rule.kind == kind)
                .flat_map(|rule| rule.paths.iter().flatten().cloned())
        };
        let mut patterns: Vec<String> = enabled(AdmissionRuleKind::IncludePaths)
            .chain(enabled(AdmissionRuleKind::ExcludePaths).map(|p| format!("!{p}")))
            .collect();
        let mut seen = HashSet::new();
        patterns.retain(|pattern| seen.insert(pattern.clone()));
        let included = patterns.iter().any(|p| !p.starts_with('!'));
        self.rules.retain(|rule| !rule.kind.legacy_path());
        if included
            && self.mode == AdmissionMode::Filtered
            && !self.rules.iter().any(|rule| rule.enabled)
        {
            self.mode = AdmissionMode::All;
        }
        Some(patterns)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_for(&ProviderId::Github)
    }

    /// Validates rules whose people are identities of `provider`.
    pub fn validate_for(&self, provider: &ProviderId) -> Result<(), String> {
        let mut kinds = HashSet::new();
        for rule in &self.rules {
            if !kinds.insert(rule.kind) {
                return Err("Each admission rule kind can appear at most once.".into());
            }
            if rule.kind.legacy_path() {
                return Err(
                    "Path admission rules moved to repository paths. Use `paths` on the repository."
                        .into(),
                );
            }
            let takes_users = matches!(
                rule.kind,
                AdmissionRuleKind::IncludeUsers | AdmissionRuleKind::ExcludeUsers
            );
            if rule.users.is_some() && !takes_users || rule.paths.is_some() {
                return Err("Admission rule values do not belong to this rule kind.".into());
            }
            validate_users(provider, rule.users())?;
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

    pub fn decide(&self, account_id: &str, pull: &PullRequest) -> Decision {
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
                _ => {}
            }
        }
        Decision::Reject(None)
    }

    pub fn admits(&self, account_id: &str, pull: &PullRequest) -> bool {
        self.decide(account_id, pull).admitted()
    }

    /// Pull requests a poll must list so that `decide` sees every one it can
    /// admit (ADR 0028). `decide` admits only through an enabled Assigned
    /// (the account reviews), Mentioned (a mention) or Include users (the
    /// author is listed) rule; Exclude users only rejects. Explicit admits
    /// nothing, All everything.
    pub fn listing_scope(&self, account_id: &str) -> crate::provider::ListingScope {
        use crate::provider::ListingScope;
        match self.mode {
            AdmissionMode::All => ListingScope::All,
            AdmissionMode::Explicit => ListingScope::Narrowed {
                authors: Vec::new(),
                reviewer: None,
                mentions: false,
            },
            AdmissionMode::Filtered => {
                let enabled = |kind| {
                    self.rules
                        .iter()
                        .filter(move |rule| rule.enabled && rule.kind == kind)
                };
                ListingScope::Narrowed {
                    authors: enabled(AdmissionRuleKind::IncludeUsers)
                        .flat_map(|rule| rule.users().iter().map(|user| user.id.clone()))
                        .collect(),
                    reviewer: enabled(AdmissionRuleKind::Assigned)
                        .next()
                        .map(|_| account_id.to_owned()),
                    mentions: enabled(AdmissionRuleKind::Mentioned).next().is_some(),
                }
            }
        }
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

fn validate_users(provider: &ProviderId, users: &[WatchedIdentity]) -> Result<(), String> {
    if users.len() > MAX_VALUES_PER_RULE {
        return Err(format!(
            "An admission rule can list at most {MAX_VALUES_PER_RULE} users."
        ));
    }
    validate_identities(provider, users)
}

/// People of one provider: unique stable IDs (GitHub: positive decimal;
/// Azure DevOps: lowercase identity GUID) with a display label (GitHub: the
/// login, no whitespace; Azure DevOps: the display name).
pub fn validate_identities(provider: &ProviderId, users: &[WatchedIdentity]) -> Result<(), String> {
    let mut ids = HashSet::new();
    for identity in users {
        match provider {
            ProviderId::Github => {
                let valid_id = identity
                    .id
                    .parse::<u64>()
                    .ok()
                    .filter(|id| *id > 0 && id.to_string() == identity.id);
                if valid_id.is_none() || !ids.insert(&identity.id) {
                    return Err(
                        "Watched GitHub account IDs must be unique positive decimal numbers."
                            .into(),
                    );
                }
                if identity.login.trim().is_empty()
                    || identity.login.chars().any(char::is_whitespace)
                {
                    return Err("Each watched GitHub identity needs a nonempty login display label without whitespace.".into());
                }
            }
            ProviderId::AzureDevops => {
                let valid_id = crate::azure_devops::tenant::TenantId::parse(&identity.id)
                    .is_some_and(|id| id.as_str() == identity.id);
                if !valid_id || !ids.insert(&identity.id) {
                    return Err("Azure DevOps people need unique identity IDs.".into());
                }
                if identity.login.trim() != identity.login
                    || identity.login.is_empty()
                    || identity.login.len() > 256
                    || identity.login.chars().any(char::is_control)
                {
                    return Err("Each Azure DevOps person needs a display name.".into());
                }
            }
        }
        if crate::policy::validate_configuration_text(&identity.login).is_err() {
            return Err("Credentials and tokens must not be stored in configuration.".into());
        }
    }
    Ok(())
}

/// One repository-relative glob (without a leading `!`).
pub(crate) fn valid_pattern(pattern: &str) -> bool {
    !(pattern.is_empty()
        || pattern.chars().count() > MAX_PATTERN_LENGTH
        || pattern.starts_with('/')
        || pattern.chars().any(char::is_control)
        || pattern.split('/').any(|segment| segment == ".."))
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
    use crate::provider::ChangedFile;

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
            assigned.decide(ME, &p),
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
            exclude.decide(ME, &p),
            Decision::Reject(Some(AdmissionRuleKind::ExcludeUsers))
        );
    }

    #[test]
    fn no_matching_rule_rejects_and_empty_rules_reject() {
        let p = pull("11", &[], false, vec![file("a", None)]);
        assert_eq!(filtered(vec![]).decide(ME, &p), Decision::Reject(None));
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
        let p = pull("11", &[], false, vec![file("a", None)]);
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
        ] {
            assert_eq!(
                make(&[admit, exclude]).decide(ME, &p),
                Decision::Admit(Some(admit)),
                "{admit:?} before {exclude:?}"
            );
            assert_eq!(
                make(&[exclude, admit]).decide(ME, &p),
                Decision::Reject(Some(exclude)),
                "{exclude:?} before {admit:?}"
            );
        }
        // A rejecting rule that does not match does not stop later admits.
        let other = pull("12", &[], true, vec![file("src/a", None)]);
        assert!(make(&[ExcludeUsers, Mentioned]).admits(ME, &other));
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
                (ExcludeUsers, false)
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
                (ExcludeUsers, false)
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
                {"kind": "exclude_users", "enabled": false, "users": []},
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
        // Path rules are legacy: only the load-time migration reads them.
        for kind in [IncludePaths, ExcludePaths] {
            let error = filtered(vec![paths(kind, &["src/"])])
                .validate()
                .unwrap_err();
            assert!(error.contains("repository paths"), "{error}");
        }
        let mut stray = users(IncludeUsers, &["5"]);
        stray.paths = Some(vec!["src/".into()]);
        assert!(filtered(vec![stray]).validate().is_err());
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
    fn azure_devops_people_are_guid_identities_with_display_names() {
        use crate::storage::ProviderId::AzureDevops;
        let person = |id: &str, login: &str| WatchedIdentity {
            id: id.into(),
            login: login.into(),
        };
        let avery = "00000000-0000-0000-0000-000000000001";
        let config = |people: Vec<WatchedIdentity>| {
            let mut rule = AdmissionRule::new(AdmissionRuleKind::IncludeUsers, true);
            rule.users = Some(people);
            filtered(vec![rule])
        };
        assert!(config(vec![person(avery, "Avery Example")])
            .validate_for(&AzureDevops)
            .is_ok());
        // GitHub rules still require decimal IDs and whitespace-free logins.
        assert!(config(vec![person(avery, "Avery Example")])
            .validate()
            .is_err());
        for invalid in [
            vec![person("11", "octo")],
            vec![person("0000000A-0000-0000-0000-000000000001", "Avery")],
            vec![person(avery, "")],
            vec![person(avery, " Avery")],
            vec![person(avery, "Avery\nExample")],
            vec![person(avery, "Avery"), person(avery, "Avery Two")],
        ] {
            assert!(
                config(invalid.clone()).validate_for(&AzureDevops).is_err(),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn legacy_path_rules_move_out_losslessly() {
        use AdmissionRuleKind::*;
        let mut disabled = paths(IncludePaths, &["vendor/"]);
        disabled.enabled = false;
        let mut config = filtered(vec![
            users(IncludeUsers, &["11"]),
            paths(ExcludePaths, &["src/gen/"]),
            disabled.clone(),
        ]);
        config
            .rules
            .insert(1, paths(IncludePaths, &["src/", "docs/"]));
        assert_eq!(
            config.take_legacy_paths(),
            Some(vec!["src/".into(), "docs/".into(), "!src/gen/".into()])
        );
        assert_eq!(config.mode, AdmissionMode::Filtered);
        assert_eq!(config.rules, vec![users(IncludeUsers, &["11"])]);
        config.validate().unwrap();
        // Idempotent: nothing left to move.
        assert_eq!(config.take_legacy_paths(), None);

        // Only path rules on: everything else was the gate, so All.
        let mut only = filtered(vec![rule(Assigned, false), paths(IncludePaths, &["src/"])]);
        assert_eq!(only.take_legacy_paths(), Some(vec!["src/".into()]));
        assert_eq!(only.mode, AdmissionMode::All);
        // Exclude-only admitted nothing before and still admits nothing.
        let mut excluded = filtered(vec![paths(ExcludePaths, &["docs/"])]);
        assert_eq!(excluded.take_legacy_paths(), Some(vec!["!docs/".into()]));
        assert_eq!(excluded.mode, AdmissionMode::Filtered);
        // Disabled path rules migrate to nothing and change no mode.
        let mut off = filtered(vec![disabled]);
        assert_eq!(off.take_legacy_paths(), Some(vec![]));
        assert_eq!(off.mode, AdmissionMode::Filtered);
        assert!(off.rules.is_empty());
    }

    #[test]
    fn glob_matching_is_linear_on_adversarial_patterns() {
        let pattern = "*a*a*a*a*a*a*a*a*a*a*a*a*b";
        let path = "a".repeat(200);
        assert!(!glob_matches(pattern, &path));
    }

    #[test]
    fn listing_scope_covers_every_rule_that_can_admit() {
        use crate::provider::ListingScope;
        use AdmissionRuleKind::*;
        let narrowed = |authors: &[&str], reviewer: bool, mentions: bool| ListingScope::Narrowed {
            authors: authors.iter().map(|id| id.to_string()).collect(),
            reviewer: reviewer.then(|| ME.to_string()),
            mentions,
        };
        let config = filtered(vec![
            users(ExcludeUsers, &["13"]),
            users(IncludeUsers, &["11", "12"]),
            rule(Assigned, true),
            rule(Mentioned, false),
        ]);
        assert_eq!(
            config.listing_scope(ME),
            narrowed(&["11", "12"], true, false)
        );
        let config = filtered(vec![
            rule(Assigned, false),
            rule(Mentioned, true),
            users(ExcludeUsers, &["13"]),
        ]);
        assert_eq!(config.listing_scope(ME), narrowed(&[], false, true));
        let mut explicit = config.clone();
        explicit.mode = AdmissionMode::Explicit;
        assert_eq!(explicit.listing_scope(ME), narrowed(&[], false, false));
        assert_eq!(
            AdmissionConfig::default().listing_scope(ME),
            ListingScope::All
        );
    }
}

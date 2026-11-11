use super::*;
use crate::provider::{Lifecycle, ReviewContext, TreeEntry};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::VecDeque;

const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MERGE_BASE: &str = "9999999999999999999999999999999999999999";

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

fn file(path: &str) -> ChangedFile {
    renamed(path, None)
}

fn renamed(path: &str, previous: Option<&str>) -> ChangedFile {
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
        sha: "c".repeat(40),
    }
}

fn files(paths: &[&str]) -> Vec<ChangedFile> {
    paths.iter().map(|p| file(p)).collect()
}

fn pull() -> PullRequest {
    PullRequest {
        mentioned: false,
        id: "7".into(),
        number: 7,
        title: "t".into(),
        author: None,
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: HEAD.into(),
        base_sha: BASE.into(),
        head_repository_id: None,
        base_repository_id: "1".into(),
        updated_at: "2026-11-01T00:00:00Z".into(),
        files: vec![],
    }
}

const BASE: &str = "dddddddddddddddddddddddddddddddddddddddd";

/// The pull request at another head or base.
fn at(head: &str, base: &str) -> PullRequest {
    PullRequest {
        head_sha: head.into(),
        base_sha: base.into(),
        ..pull()
    }
}

fn repository() -> RemoteRepository {
    RemoteRepository {
        id: "1".into(),
        name: "octo-org/app".into(),
    }
}

/// Provider stub: one whole-PR answer, one incremental answer, and a call log.
struct Stub {
    whole: Result<Vec<ChangedFile>, ConnectionError>,
    /// Reported changed files the provider cannot list.
    incomplete: Option<u64>,
    since: Result<Since, ConnectionError>,
    /// Tree diff answers, in call order; empty: truncated.
    trees: RefCell<VecDeque<Result<TreeDiff, ConnectionError>>>,
    calls: RefCell<Vec<String>>,
}

impl Stub {
    fn new(whole: Vec<ChangedFile>) -> Self {
        Self {
            whole: Ok(whole),
            incomplete: None,
            since: Ok(Since::WholePullRequest),
            trees: RefCell::new(VecDeque::new()),
            calls: RefCell::new(vec![]),
        }
    }
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
    fn tree(self, answer: Result<TreeDiff, ConnectionError>) -> Self {
        self.trees.borrow_mut().push_back(answer);
        self
    }
    fn incomplete(mut self, count: u64) -> Self {
        self.incomplete = Some(count);
        self
    }
}

impl PullRequestReader for Stub {
    fn latest_pull_request_number(&self, _: &RemoteRepository) -> Result<u64, ConnectionError> {
        unimplemented!()
    }
    fn pull_requests(&self, _: &RemoteRepository) -> Result<Vec<PullRequest>, ConnectionError> {
        unimplemented!()
    }
    fn poll_pull_requests(
        &self,
        _: &RemoteRepository,
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        unimplemented!()
    }
    fn poll_tracked_pull_requests(
        &self,
        _: &RemoteRepository,
        _: &[(String, u64)],
    ) -> Result<Vec<PullRequest>, ConnectionError> {
        unimplemented!()
    }
    fn pull_request(&self, _: &RemoteRepository, _: u64) -> Result<PullRequest, ConnectionError> {
        unimplemented!()
    }
    fn pull_request_files(
        &self,
        _: &RemoteRepository,
        _: u64,
    ) -> Result<Vec<ChangedFile>, ConnectionError> {
        unimplemented!()
    }
    fn changed_file_listing(
        &self,
        _: &RemoteRepository,
        _: &PullRequest,
        list: bool,
    ) -> Result<Listing, ConnectionError> {
        self.calls.borrow_mut().push(format!("listing:{list}"));
        if let Some(count) = self.incomplete {
            return Ok(Listing::Incomplete(count));
        }
        let files = self.whole.clone()?;
        Ok(if list {
            Listing::Complete(files)
        } else {
            Listing::Visible
        })
    }
    fn changed_files_since(
        &self,
        _: &RemoteRepository,
        _: &PullRequest,
        old_head: &str,
    ) -> Result<Since, ConnectionError> {
        self.calls.borrow_mut().push(format!("since:{old_head}"));
        self.since.clone()
    }
    fn merge_base(&self, _: &RemoteRepository, _: &PullRequest) -> Result<String, ConnectionError> {
        self.calls.borrow_mut().push("merge_base".into());
        Ok(MERGE_BASE.into())
    }
    fn tree_diff(
        &self,
        _: &RemoteRepository,
        from: &str,
        to: &str,
        scope: &[String],
    ) -> Result<TreeDiff, ConnectionError> {
        self.calls
            .borrow_mut()
            .push(format!("tree:{}..{}:{scope:?}", &from[..1], &to[..1]));
        self.trees
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(TreeDiff::Truncated))
    }
    fn review_context(
        &self,
        _: &RemoteRepository,
        _: u64,
        _: &str,
    ) -> Result<ReviewContext, ConnectionError> {
        unimplemented!()
    }
    fn file_content(&self, _: &str, _: &TreeEntry) -> Result<Value, ConnectionError> {
        unimplemented!()
    }
}

type SecondaryInput<'a> = (&'a str, &'a [&'a str], Option<&'a str>);

fn plan(gate: &[&str], secondaries: &[SecondaryInput]) -> Plan {
    Plan {
        gate: strings(gate),
        whole: false,
        secondaries: secondaries
            .iter()
            .map(|(id, patterns, since)| SecondaryPlan {
                assignment_id: id.to_string(),
                patterns: strings(patterns),
                since: since.map(str::to_string),
            })
            .collect(),
    }
}

fn read(
    stub: &Stub,
    plan: &Plan,
    previous: Option<&PathFacts>,
) -> Result<PathFacts, ConnectionError> {
    read_facts(stub, &repository(), &pull(), plan, previous)
}

#[test]
fn gate_matrix_with_negation_and_last_match_wins() {
    let gate = strings(&["src/", "!src/gen/", "src/gen/keep.rs"]);
    for (changed, expected) in [
        (&["src/a.rs"][..], true),
        (&["docs/a.md"][..], false),
        (&["src/gen/x.rs"][..], false),
        (&["src/gen/keep.rs"][..], true),
        (&["docs/a.md", "src/gen/x.rs"][..], false),
        (&["docs/a.md", "src/b.rs"][..], true),
        (&[][..], false),
    ] {
        assert_eq!(matches(&gate, &files(changed)), expected, "{changed:?}");
    }
    // Only excludes: everything else matches.
    let excludes = strings(&["!docs/", "!*.md"]);
    assert!(matches(&excludes, &files(&["src/a.rs"])));
    assert!(!matches(&excludes, &files(&["docs/a.txt", "README.md"])));
    // A later include re-admits what an exclude dropped.
    assert!(matches(
        &strings(&["!docs/", "docs/api/"]),
        &files(&["docs/api/a.md"])
    ));
    // Renames match on either name.
    let moved = vec![renamed("docs/new.md", Some("src/old.rs"))];
    assert!(matches(&strings(&["src/"]), &moved));
    assert!(matches(&strings(&["docs/"]), &moved));
    assert!(!matches(&strings(&["lib/"]), &moved));
    // Empty globs: no gate, even with no files.
    assert!(matches(&[], &[]));
    assert!(matches(&[], &files(&["anything"])));
}

#[test]
fn validation_reuses_admission_limits() {
    validate(&strings(&["src/", "!src/gen/", "**/*.ts"]), "Repository").unwrap();
    for bad in ["", "!", "/abs", "!/abs", "a/../b", "!..", "a\u{0}b"] {
        let error = validate(&strings(&[bad]), "Repository").unwrap_err();
        assert!(
            error.starts_with("Repository path patterns"),
            "{bad:?}: {error}"
        );
    }
    let long = "x".repeat(MAX_PATTERN_LENGTH);
    validate(&[long.clone(), format!("!{long}")], "Assignment").unwrap();
    assert!(validate(&[format!("{long}x")], "Assignment").is_err());
    // Room for both legacy path rules (100 each) after migration.
    assert_eq!(MAX_PATTERNS, 2 * MAX_VALUES_PER_RULE);
    let many: Vec<_> = (0..=MAX_PATTERNS).map(|i| format!("p{i}/")).collect();
    assert!(validate(&many, "Repository")
        .unwrap_err()
        .contains("at most 200"));
    validate(&many[..MAX_PATTERNS], "Repository").unwrap();
    assert!(validate(&strings(&["src/", "src/"]), "Repository")
        .unwrap_err()
        .contains("twice"));
}

#[test]
fn empty_globs_only_check_visibility() {
    let stub = Stub::new(files(&["src/a.rs"]));
    let facts = read(&stub, &Plan::default(), None).unwrap();
    assert_eq!(stub.calls(), ["listing:false"]);
    assert!(facts.admitted && facts.incomplete.is_none() && facts.secondaries.is_empty());
    assert!(!facts.gated_out(&pull()));
    assert!(facts.current(&pull(), &Plan::default()));
    // Same head and inputs: nothing to read again.
    let again = read(&stub, &Plan::default(), Some(&facts)).unwrap();
    assert_eq!(again, facts);
    assert_eq!(stub.calls().len(), 1);
}

#[test]
fn scope_is_each_include_fixed_prefix() {
    for (patterns, expected) in [
        (&["src/**/*.ts"][..], &["src/"][..]),
        (&["src/", "src/gen/", "!src/gen/x"][..], &["src/"][..]),
        (&["docs/a.md", "lib/*/mod.rs"][..], &["docs/", "lib/"][..]),
        (&["*.md", "src/"][..], &[""][..]),
        (&["**/Makefile"][..], &[""][..]),
        (&["!docs/"][..], &[""][..]),
        (&[][..], &[][..]),
    ] {
        assert_eq!(scope(&strings(patterns)), strings(expected), "{patterns:?}");
    }
}

#[test]
fn incomplete_listing_climbs_to_a_tree_diff_before_parking() {
    let gate = plan(&["src/"], &[]);
    // Step 2: the whole tree diff completes the list.
    let stub = Stub::new(vec![])
        .incomplete(3_500)
        .tree(Ok(TreeDiff::Complete(files(&["src/a.rs"]))));
    let facts = read(&stub, &gate, None).unwrap();
    assert!(facts.incomplete.is_none() && !facts.gated_out(&pull()));
    assert_eq!(stub.calls(), ["listing:true", "merge_base", "tree:9..b:[]"]);
    // Step 3: a truncated tree falls back to the scoped walk.
    let stub = Stub::new(vec![])
        .incomplete(3_500)
        .tree(Ok(TreeDiff::Truncated))
        .tree(Ok(TreeDiff::Complete(files(&["docs/a.md"]))));
    let facts = read(&stub, &gate, None).unwrap();
    assert!(facts.gated_out(&pull()), "decided on the scoped list");
    assert_eq!(stub.calls()[3], "tree:9..b:[\"src/\"]");
    // A scoped list that admits parks: the review needs every file.
    let stub = Stub::new(vec![])
        .incomplete(3_500)
        .tree(Ok(TreeDiff::Truncated))
        .tree(Ok(TreeDiff::Complete(files(&["src/a.rs"]))));
    let facts = read(&stub, &gate, None).unwrap();
    assert_eq!(facts.incomplete_at(HEAD), Some(3_500));
    assert_eq!(
        facts.tried,
        [
            Strategy::Listing,
            Strategy::TreeDiff,
            Strategy::ScopedTreeDiff
        ]
    );
    // Secondaries alone: the gate admits everything, so it parks too.
    let stub = Stub::new(vec![])
        .incomplete(3_500)
        .tree(Ok(TreeDiff::Truncated))
        .tree(Ok(TreeDiff::Complete(files(&["docs/a.md"]))));
    let facts = read(&stub, &plan(&[], &[("s", &["docs/"], None)]), None).unwrap();
    assert_eq!(facts.incomplete_at(HEAD), Some(3_500));
    // A scoped gate-out, then the gate is removed on the same revision: the
    // earlier verdict never stands in for a complete list.
    let stub = Stub::new(vec![])
        .incomplete(3_500)
        .tree(Ok(TreeDiff::Truncated))
        .tree(Ok(TreeDiff::Complete(files(&["docs/a.md"]))));
    let gated_out = read(&stub, &gate, None).unwrap();
    assert!(gated_out.gated_out(&pull()));
    let stub = Stub::new(vec![]).incomplete(3_500);
    let facts = read(&stub, &Plan::default(), Some(&gated_out)).unwrap();
    assert_eq!(facts.incomplete_at(HEAD), Some(3_500));
    let stub = Stub::new(vec![]).incomplete(3_500);
    let ungated = plan(&[], &[("s", &["docs/"], None)]);
    let facts = read(&stub, &ungated, Some(&gated_out)).unwrap();
    assert_eq!(facts.incomplete_at(HEAD), Some(3_500));
    // Step 4 (park): only after every strategy failed.
    let stub = Stub::new(vec![]).incomplete(3_500);
    let facts = read(&stub, &gate, None).unwrap();
    assert_eq!(facts.incomplete_at(HEAD), Some(3_500));
    assert_eq!(
        facts.tried,
        [
            Strategy::Listing,
            Strategy::TreeDiff,
            Strategy::ScopedTreeDiff
        ]
    );
}

#[test]
fn a_revision_that_runs_regardless_of_the_gate_parks_on_a_scoped_negative() {
    // OR mode, rule-admitted (ADR 0029): a scoped walk with no match cannot
    // stand in for the complete list the review reads.
    let or = Plan {
        whole: true,
        ..plan(&["src/"], &[])
    };
    let scoped_negative = || {
        Stub::new(vec![])
            .incomplete(3_500)
            .tree(Ok(TreeDiff::Truncated))
            .tree(Ok(TreeDiff::Complete(files(&["docs/a.md"]))))
    };
    let facts = read(&scoped_negative(), &or, None).unwrap();
    assert_eq!(facts.incomplete_for(&pull()), Some(3_500));
    // A path-only candidate keeps the scoped verdict: it never runs on it.
    let and = plan(&["src/"], &[]);
    let gated_out = read(&scoped_negative(), &and, None).unwrap();
    assert!(gated_out.gated_out(&pull()));
    assert!(gated_out.current(&pull(), &and));
    // A negative cached without `whole` is re-read once the revision may run.
    assert!(!gated_out.current(&pull(), &or));
    let complete = read(&Stub::new(files(&["docs/a.md"])), &or, None).unwrap();
    assert!(complete.whole && !complete.admitted && complete.incomplete.is_none());
    assert!(complete.current(&pull(), &or) && complete.current(&pull(), &and));
}

#[test]
fn without_globs_only_completeness_is_checked() {
    // A complete tree diff proves every file is visible.
    let stub = Stub::new(vec![])
        .incomplete(4_000)
        .tree(Ok(TreeDiff::Complete(files(&["a"]))));
    let facts = read(&stub, &Plan::default(), None).unwrap();
    assert!(facts.incomplete.is_none());
    assert_eq!(
        stub.calls(),
        ["listing:false", "merge_base", "tree:9..b:[]"]
    );
    // A scoped walk proves nothing without globs: parked after the tree diff.
    let stub = Stub::new(vec![]).incomplete(4_000);
    let facts = read(&stub, &Plan::default(), None).unwrap();
    assert_eq!(facts.incomplete_at(HEAD), Some(4_000));
    assert_eq!(facts.tried, [Strategy::Listing, Strategy::TreeDiff]);
}

#[test]
fn capped_compare_climbs_the_ladder_then_uses_the_whole_pull_request() {
    let p = plan(&[], &[("s", &["src/"], Some(OLD))]);
    let capped = |stub: Stub| Stub {
        since: Ok(Since::Capped),
        ..stub
    };
    let stub =
        capped(Stub::new(files(&["src/a.rs"]))).tree(Ok(TreeDiff::Complete(files(&["docs/a.md"]))));
    assert!(read(&stub, &p, None)
        .unwrap()
        .skips(&pull(), "s", Some(OLD), &strings(&["src/"])));
    assert_eq!(stub.calls()[2], "tree:a..b:[]");
    let stub = capped(Stub::new(files(&["src/a.rs"])))
        .tree(Ok(TreeDiff::Truncated))
        .tree(Ok(TreeDiff::Complete(files(&["src/b.rs"]))));
    assert!(!read(&stub, &p, None)
        .unwrap()
        .skips(&pull(), "s", Some(OLD), &strings(&["src/"])));
    assert_eq!(stub.calls()[3], "tree:a..b:[\"src/\"]");
    // Both cut off: the complete whole-PR list decides, conservatively.
    let stub = capped(Stub::new(files(&["src/a.rs"])));
    assert!(!read(&stub, &p, None)
        .unwrap()
        .skips(&pull(), "s", Some(OLD), &strings(&["src/"])));
}

#[test]
fn incomplete_listing_parks_with_or_without_globs() {
    for plan in [Plan::default(), plan(&["src/"], &[("s", &["docs/"], None)])] {
        let stub = Stub::new(vec![]).incomplete(3_001);
        let facts = read(&stub, &plan, None).unwrap();
        assert_eq!(facts.incomplete_at(HEAD), Some(3_001));
        assert_eq!(facts.incomplete_at(OLD), None);
        assert!(!facts.gated_out(&pull()), "never decided on a partial list");
        assert!(!facts.skips(&pull(), "s", None, &strings(&["docs/"])));
        assert!(facts.current(&pull(), &plan));
    }
}

#[test]
fn gate_admits_matching_and_keeps_out_others() {
    let gate = plan(&["src/", "!src/gen/"], &[]);
    let matching = read(&Stub::new(files(&["src/a.rs"])), &gate, None).unwrap();
    assert!(!matching.gated_out(&pull()));
    let other = read(&Stub::new(files(&["docs/a.md"])), &gate, None).unwrap();
    assert!(other.gated_out(&pull()));
    assert!(
        !other.gated_out(&at(OLD, BASE)),
        "only for the revision it was read at"
    );
    let excluded = read(&Stub::new(files(&["src/gen/a.rs"])), &gate, None).unwrap();
    assert!(excluded.gated_out(&pull()));
    // A changed gate is re-evaluated on the same head.
    let widened = plan(&["docs/"], &[]);
    assert!(!other.current(&pull(), &widened));
    let stub = Stub::new(files(&["docs/a.md"]));
    assert!(!read(&stub, &widened, Some(&other))
        .unwrap()
        .gated_out(&pull()));
    assert_eq!(stub.calls(), ["listing:true"]);
}

#[test]
fn first_secondary_review_matches_the_whole_pull_request() {
    let stub = Stub::new(files(&["docs/a.md", "src/a.rs"]));
    let p = plan(&[], &[("docs", &["docs/"], None), ("api", &["api/"], None)]);
    let facts = read(&stub, &p, None).unwrap();
    assert_eq!(
        stub.calls(),
        ["listing:true"],
        "one whole-PR list for every glob"
    );
    assert!(!facts.skips(&pull(), "docs", None, &strings(&["docs/"])));
    assert!(facts.skips(&pull(), "api", None, &strings(&["api/"])));
    assert!(!facts.skips(&pull(), "unglobbed", None, &strings(&["docs/"])));
}

#[test]
fn secondary_matches_incrementally_since_its_last_review() {
    let mut stub = Stub::new(files(&["src/a.rs"]));
    stub.since = Ok(Since::Files(files(&["README.md"])));
    let p = plan(&[], &[("s", &["src/"], Some(OLD))]);
    let facts = read(&stub, &p, None).unwrap();
    // The whole PR touches src/, but nothing since its review does.
    assert!(facts.skips(&pull(), "s", Some(OLD), &strings(&["src/"])));
    assert_eq!(
        stub.calls(),
        ["listing:true".to_string(), format!("since:{OLD}")]
    );
    // Its inputs moved (another review completed): these facts never skip.
    assert!(!facts.skips(
        &pull(),
        "s",
        Some("e".repeat(40).as_str()),
        &strings(&["src/"])
    ));

    stub.since = Ok(Since::Files(files(&["src/b.rs"])));
    assert!(!read(&stub, &p, None)
        .unwrap()
        .skips(&pull(), "s", Some(OLD), &strings(&["src/"])));
}

#[test]
fn rebase_or_capped_compare_falls_back_to_the_whole_pull_request() {
    let mut stub = Stub::new(files(&["src/a.rs"]));
    stub.since = Ok(Since::WholePullRequest);
    let p = plan(&[], &[("s", &["src/"], Some(OLD))]);
    assert!(!read(&stub, &p, None)
        .unwrap()
        .skips(&pull(), "s", Some(OLD), &strings(&["src/"])));
    let p = plan(&[], &[("s", &["lib/"], Some(OLD))]);
    assert!(read(&stub, &p, None)
        .unwrap()
        .skips(&pull(), "s", Some(OLD), &strings(&["lib/"])));
}

#[test]
fn reviewed_head_has_no_new_files() {
    let stub = Stub::new(files(&["src/a.rs"]));
    let p = plan(&[], &[("s", &["src/"], Some(HEAD))]);
    let previous = read(&Stub::new(files(&["src/a.rs"])), &Plan::default(), None).unwrap();
    let facts = read(&stub, &p, Some(&previous)).unwrap();
    assert!(facts.skips(&pull(), "s", Some(HEAD), &strings(&["src/"])));
    assert!(stub.calls().is_empty(), "no provider read decides it");
}

#[test]
fn retries_never_decide() {
    for error in [
        ConnectionError::RateLimited,
        ConnectionError::Network,
        ConnectionError::IncompleteRead,
        ConnectionError::RevisionChanged,
    ] {
        let mut stub = Stub::new(vec![]);
        stub.whole = Err(error);
        assert_eq!(read(&stub, &plan(&["src/"], &[]), None), Err(error));
        assert_eq!(read(&stub, &Plan::default(), None), Err(error));
        let mut stub = Stub::new(files(&["src/a.rs"]));
        stub.since = Err(error);
        assert_eq!(
            read(&stub, &plan(&[], &[("s", &["src/"], Some(OLD))]), None),
            Err(error)
        );
        // Nor does a failing tree diff park.
        let stub = Stub::new(vec![]).incomplete(5_000).tree(Err(error));
        assert_eq!(read(&stub, &plan(&["src/"], &[]), None), Err(error));
        let stub = Stub::new(vec![])
            .incomplete(5_000)
            .tree(Ok(TreeDiff::Truncated))
            .tree(Err(error));
        assert_eq!(read(&stub, &plan(&["src/"], &[]), None), Err(error));
    }
}

#[test]
fn facts_round_trip_and_reject_unknown_fields() {
    let facts = read(
        &Stub::new(files(&["src/a.rs"])),
        &plan(&["src/"], &[("s", &["docs/"], Some(OLD))]),
        None,
    )
    .unwrap();
    let json = serde_json::to_value(&facts).unwrap();
    assert_eq!(
        serde_json::from_value::<PathFacts>(json.clone()).unwrap(),
        facts
    );
    let mut extra = json;
    extra["surprise"] = true.into();
    assert!(serde_json::from_value::<PathFacts>(extra).is_err());
}

#[test]
fn a_retargeted_base_or_changed_patterns_never_reuse_facts() {
    let gate = plan(&["src/"], &[("s", &["docs/"], None)]);
    let facts = read(&Stub::new(files(&["docs/a.md"])), &gate, None).unwrap();
    assert!(facts.gated_out(&pull()));
    // Same head, new base: another diff, so the facts are stale and re-read.
    let moved = at(HEAD, OLD);
    assert!(!facts.current(&moved, &gate));
    assert!(!facts.gated_out(&moved));
    let stub = Stub::new(files(&["src/a.rs"]));
    let fresh = read_facts(&stub, &repository(), &moved, &gate, Some(&facts)).unwrap();
    assert!(!fresh.gated_out(&moved));
    assert_eq!(stub.calls()[0], "listing:true");
    // Legacy facts without a base are stale too.
    let mut legacy = serde_json::to_value(&facts).unwrap();
    legacy.as_object_mut().unwrap().remove("base_sha");
    let legacy: PathFacts = serde_json::from_value(legacy).unwrap();
    assert!(!legacy.current(&pull(), &gate));
    // A verdict taken for other secondary patterns never skips.
    let skipping = read(&Stub::new(files(&["src/a.rs"])), &gate, None).unwrap();
    assert!(skipping.skips(&pull(), "s", None, &strings(&["docs/"])));
    assert!(!skipping.skips(&pull(), "s", None, &strings(&["src/"])));
}

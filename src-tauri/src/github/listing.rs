//! GitHub's batched, narrowed listing of open pull requests (ADR 0028).

use super::{
    metadata::{array, polling_pull_request},
    provider::{parse_response, GithubClient, Transport},
    verify_identity,
};
use crate::provider::{
    listing::{self, Batch},
    ConnectionError, Lifecycle, ListingCursor, ListingScope, PolledPullRequests, PullRequest,
    PullRequestReader, RemoteRepository,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const PAGE_SIZE: u64 = 100;
/// Open-listing pages one poll reads; the pass resumes on the next poll.
const LISTING_PAGES: u64 = 10;
/// Search pages one poll reads: search allows 30 requests a minute, and each
/// hit costs one pull request read.
const SEARCH_PAGES: u64 = 3;
/// Pages are numbered: a resumed pass re-reads one page, which covers up to
/// a page of closures since the last poll.
const RESUME_OVERLAP: u64 = PAGE_SIZE;
/// GitHub search returns at most this many results per query.
const SEARCH_CAP: u64 = 1_000;
/// Qualifiers OR-ed in one search query, well inside search's limit of five
/// operators.
const SEARCH_TERMS: usize = 3;
/// GitHub detects mentions in top comments (`CommentReader::top_comments`),
/// so Mentioning me can admit any open pull request. A `mentions:` search
/// cannot stand in for the listing: the first observation of a pull request
/// baselines its comments (a mention already there never admits), so a pull
/// request first listed because of a mention would never be admitted by it;
/// and the mention match is textual (`@login` in any comment, edits too),
/// which GitHub's mention index does not promise to cover.
const DETECTS_MENTIONS: bool = true;

/// One qualifier of a narrowed search: a GitHub user ID.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    Author(String),
    Reviewer(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Query {
    /// Every open pull request, newest first.
    All,
    /// Open pull requests matching any of these terms.
    Search(Vec<Term>),
}

/// The queries whose union holds every open pull request `scope` may admit.
/// `decide` admits through Include users (the author: an `author:` term),
/// Assigned (the account is a requested reviewer: `review-requested:` also
/// covers its teams) or Mentioned (every open pull request, see
/// `DETECTS_MENTIONS`). `wide`: a search could not be served whole this pass.
fn queries(scope: &ListingScope, wide: bool) -> Vec<Query> {
    match scope {
        ListingScope::All => vec![Query::All],
        ListingScope::Narrowed { mentions: true, .. } if DETECTS_MENTIONS => vec![Query::All],
        ListingScope::Narrowed { .. } if wide => vec![Query::All],
        ListingScope::Narrowed {
            authors, reviewer, ..
        } => {
            // GitHub user IDs are decimal; others match no pull request.
            let user = |id: &&String| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit());
            let authors: BTreeSet<&String> = authors.iter().filter(user).collect();
            let terms: Vec<Term> = authors
                .into_iter()
                .map(|id| Term::Author(id.clone()))
                .chain(reviewer.iter().filter(user).cloned().map(Term::Reviewer))
                .collect();
            terms
                .chunks(SEARCH_TERMS)
                .map(|chunk| Query::Search(chunk.to_vec()))
                .collect()
        }
    }
}

/// Adds one listed open pull request; one that does not parse is skipped,
/// not fatal (ADR 0028).
fn collect(
    repository: &RemoteRepository,
    value: &Value,
    pulls: &mut BTreeMap<u64, PullRequest>,
) -> Result<(), ConnectionError> {
    let Ok(pull) = polling_pull_request(value) else {
        return Ok(());
    };
    if pull.base_repository_id != repository.id {
        return Err(ConnectionError::RepositoryChanged);
    }
    if pull.state == Lifecycle::Open {
        pulls.entry(pull.number).or_insert(pull);
    }
    Ok(())
}

/// A failure of one pull request's own read: that pull request is skipped,
/// the listing goes on (ADR 0028).
fn pull_local(error: ConnectionError) -> bool {
    error.is_invalid_response() || error == ConnectionError::IncompleteRead
}

fn valid_login(login: &str) -> bool {
    let name = login.strip_suffix("[bot]").unwrap_or(login);
    !name.is_empty()
        && name.len() <= 39
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

impl<T: Transport> GithubClient<T> {
    /// A read whose 404 or 422 means "this query cannot be served" (`None`).
    fn soft_read(&self, path: &str) -> Result<Option<Value>, ConnectionError> {
        let response = self.transport.get(path)?;
        if matches!(response.status, 404 | 422) {
            return Ok(None);
        }
        parse_response(response)
            .map(|(value, _)| Some(value))
            .inspect_err(|error| self.publication_provider_error(*error))
    }

    /// The search qualifier of one term, by the user's current login (logins
    /// change; IDs do not). `None`: the user cannot be searched.
    fn qualifier(&self, term: &Term) -> Result<Option<String>, ConnectionError> {
        let (Term::Author(id) | Term::Reviewer(id)) = term;
        let Some(user) = self.soft_read(&format!("/user/{id}"))? else {
            return Ok(None);
        };
        let person = verify_identity(&user, None)?;
        if &person.id != id || !valid_login(&person.login) {
            return Ok(None);
        }
        Ok(Some(match (term, person.login.strip_suffix("[bot]")) {
            (Term::Author(_), Some(app)) => format!("author:app/{app}"),
            (Term::Author(_), None) => format!("author:{}", person.login),
            (Term::Reviewer(_), _) => format!("review-requested:{}", person.login),
        }))
    }

    /// One page of the open listing, newest first: the count it held.
    fn open_page(
        &self,
        name: &str,
        repository: &RemoteRepository,
        skip: u64,
        pulls: &mut BTreeMap<u64, PullRequest>,
    ) -> Result<u64, ConnectionError> {
        let page = skip / PAGE_SIZE + 1;
        let (value, _) = self.read(&format!(
            "/repos/{name}/pulls?state=open&sort=created&direction=desc&per_page={PAGE_SIZE}&page={page}"
        ))?;
        let entries = array(&value)?;
        if entries.len() as u64 > PAGE_SIZE {
            return Err(ConnectionError::InvalidResponse);
        }
        for entry in entries {
            collect(repository, entry, pulls)?;
        }
        Ok(entries.len() as u64)
    }

    /// One page of a narrowed search, newest first: the count it held, each
    /// hit read as a pull request. `None`: the search cannot be served whole
    /// (past the search cap, incomplete, or refused), so the pass lists
    /// every open pull request instead.
    fn search_page(
        &self,
        name: &str,
        repository: &RemoteRepository,
        terms: &[Term],
        skip: u64,
        pulls: &mut BTreeMap<u64, PullRequest>,
    ) -> Result<Option<u64>, ConnectionError> {
        if skip + PAGE_SIZE > SEARCH_CAP {
            return Ok(None);
        }
        let mut qualifiers = Vec::new();
        for term in terms {
            match self.qualifier(term)? {
                Some(qualifier) => qualifiers.push(qualifier),
                None => return Ok(None),
            }
        }
        let advanced = qualifiers.len() > 1;
        let any = if advanced {
            format!("({})", qualifiers.join(" OR "))
        } else {
            qualifiers.concat()
        };
        let query: String = url::form_urlencoded::byte_serialize(
            format!("repo:{name} is:pr is:open {any}").as_bytes(),
        )
        .collect();
        let page = skip / PAGE_SIZE + 1;
        let Some(value) =
            self.soft_read(&format!(
            "/search/issues?q={query}{}&sort=created&order=desc&per_page={PAGE_SIZE}&page={page}",
            if advanced { "&advanced_search=true" } else { "" }
        ))?
        else {
            return Ok(None);
        };
        let total = value["total_count"]
            .as_u64()
            .ok_or(ConnectionError::InvalidResponse)?;
        if total > SEARCH_CAP || value["incomplete_results"].as_bool() != Some(false) {
            return Ok(None);
        }
        let items = array(&value["items"])?;
        if items.len() as u64 > PAGE_SIZE {
            return Err(ConnectionError::InvalidResponse);
        }
        for item in items {
            let Some(number) = item["number"]
                .as_u64()
                .filter(|number| *number > 0 && item["pull_request"].is_object())
            else {
                continue;
            };
            if pulls.contains_key(&number) {
                continue;
            }
            match self.pull_request(repository, number) {
                Ok(pull) if pull.state == Lifecycle::Open => {
                    pulls.insert(number, pull);
                }
                Ok(_) => {}
                Err(error) if pull_local(error) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Some(items.len() as u64))
    }

    /// One poll's batch (ADR 0028): pages of the scope's queries from
    /// `cursor`, then tracked pull requests the batch missed and a slice of
    /// closed ones (a reopen the scope may not list). Repository size is
    /// never a failure; only access, identity and repository failures
    /// surface, and throttling is the poll's rate limit.
    pub(super) fn listing_batch(
        &self,
        repository: &RemoteRepository,
        tracked: &[(String, u64)],
        closed: &[(String, u64)],
        scope: &ListingScope,
        cursor: Option<&ListingCursor>,
        now: i64,
    ) -> Result<PolledPullRequests, ConnectionError> {
        let name = crate::storage::canonical_repository(&repository.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        let wide = cursor.is_some_and(|cursor| cursor.wide);
        let queries = queries(scope, wide);
        let mut next = cursor
            .filter(|cursor| cursor.query < queries.len().max(1))
            .cloned()
            .unwrap_or_default();
        let mut pulls = BTreeMap::new();
        if next.retry_at.is_none_or(|at| now >= at) {
            next.retry_at = None;
            self.list_batch(&name, repository, queries, &mut next, &mut pulls, now)?;
        }
        let (open, closed) = listing::tracked_reads(
            |id| pulls.values().any(|pull| pull.id == id),
            tracked,
            closed,
            &mut next,
        );
        for ((id, number), reopen) in open
            .iter()
            .map(|pull| (pull, false))
            .chain(closed.iter().map(|pull| (pull, true)))
        {
            let pull = match self.pull_request(repository, *number) {
                Ok(pull) => pull,
                Err(error) if pull_local(error) => continue,
                Err(error) => return Err(error),
            };
            if &pull.id != id {
                return Err(ConnectionError::RepositoryChanged);
            }
            // A closed probe matters only when it reopened.
            if !reopen || pull.state == Lifecycle::Open {
                pulls.insert(pull.number, pull);
            }
        }
        Ok(PolledPullRequests {
            pulls: pulls.into_values().collect(),
            cursor: (next != ListingCursor::default()).then_some(next),
        })
    }

    fn list_batch(
        &self,
        name: &str,
        repository: &RemoteRepository,
        mut queries: Vec<Query>,
        next: &mut ListingCursor,
        pulls: &mut BTreeMap<u64, PullRequest>,
        now: i64,
    ) -> Result<(), ConnectionError> {
        let seed = format!("github:{}", repository.id);
        let mut batch = Batch::start(next, queries.len(), RESUME_OVERLAP);
        while let Some((index, skip)) = batch.next(LISTING_PAGES) {
            let read = match &queries[index] {
                Query::All => self.open_page(name, repository, skip, pulls).map(Some),
                Query::Search(_) if batch.pages() >= SEARCH_PAGES => break,
                Query::Search(terms) => self.search_page(name, repository, terms, skip, pulls),
            };
            match read {
                Ok(Some(count)) => batch.read(count, PAGE_SIZE),
                Ok(None) => {
                    batch.widen();
                    queries = vec![Query::All];
                }
                Err(error) => return batch.failed(error, now, &seed),
            }
        }
        batch.finish();
        Ok(())
    }
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;

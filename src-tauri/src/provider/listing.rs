//! Provider-neutral stepping of a batched, resumable listing pass (ADR 0028).
//! Providers read pages; this decides which page comes next, when a batch
//! ends, how a failed read stalls the pass and which tracked pull requests
//! are read on their own.

use super::{ConnectionError, ListingCursor};

/// Tracked pull requests missing from a batch read on their own per poll.
pub const TRACKED_BATCH: usize = 200;
/// Closed tracked pull requests probed for a reopen per poll.
pub const CLOSED_BATCH: usize = 20;

/// Pull requests by ID and number.
pub type Numbered = Vec<(String, u64)>;

/// One poll's walk over the scope's queries from a cursor.
pub struct Batch<'a> {
    cursor: &'a mut ListingCursor,
    queries: usize,
    /// The next read is the head page (newest first): pull requests opened
    /// since the pass began.
    head: bool,
    pages: u64,
}

impl<'a> Batch<'a> {
    /// Resumes `cursor`, backing off `overlap` places: closures since the
    /// last poll move later pull requests forward.
    pub fn start(cursor: &'a mut ListingCursor, queries: usize, overlap: u64) -> Self {
        let head = cursor.skip > 0;
        cursor.skip = cursor.skip.saturating_sub(overlap);
        Self {
            cursor,
            queries,
            head,
            pages: 0,
        }
    }

    /// The query index and skip of the next page, or `None` when the pass is
    /// done or `budget` pages were read this poll.
    pub fn next(&self, budget: u64) -> Option<(usize, u64)> {
        (self.cursor.query < self.queries && self.pages < budget).then_some((
            self.cursor.query,
            if self.head { 0 } else { self.cursor.skip },
        ))
    }

    /// A page of `count` entries was read; fewer than `page_size` ends the
    /// query.
    pub fn read(&mut self, count: u64, page_size: u64) {
        self.pages += 1;
        if std::mem::take(&mut self.head) {
            return;
        }
        if count < page_size {
            self.cursor.query += 1;
            self.cursor.skip = 0;
        } else {
            self.cursor.skip += count;
        }
    }

    /// Pages read this poll.
    pub fn pages(&self) -> u64 {
        self.pages
    }

    /// The rest of this pass lists every open pull request (one query): a
    /// narrowed query could not be served whole.
    pub fn widen(&mut self) {
        self.cursor.wide = true;
        self.queries = 1;
        self.cursor.query = 0;
        self.cursor.skip = 0;
        self.head = false;
    }

    /// A page read failed. Throttling is the poll's rate limit (shared
    /// backpressure, nothing committed): the error is returned. A failure a
    /// later poll can clear ends the batch where it stopped; with no page
    /// read, the listing waits under the shared transient backoff while
    /// tracked reads go on. Anything else surfaces.
    pub fn failed(
        self,
        error: ConnectionError,
        now: i64,
        seed: &str,
    ) -> Result<(), ConnectionError> {
        if throttled(error) || !resumable(error) {
            return Err(error);
        }
        if self.pages == 0 {
            self.cursor.stalls = self.cursor.stalls.saturating_add(1);
            self.cursor.retry_at = Some(now.saturating_add(crate::retry::TRANSIENT.wait(
                seed,
                self.cursor.stalls.saturating_sub(1),
                stated_wait(error),
            )));
        } else {
            self.cursor.stalls = 0;
        }
        Ok(())
    }

    /// The batch read every page it could: a finished pass starts over.
    pub fn finish(self) {
        self.cursor.stalls = 0;
        if self.cursor.query >= self.queries {
            self.cursor.query = 0;
            self.cursor.skip = 0;
            self.cursor.passes += 1;
            self.cursor.wide = false;
        }
    }
}

/// Tracked pull requests not `listed` in this batch, read on their own: up
/// to `TRACKED_BATCH` open and `CLOSED_BATCH` closed (probed for a reopen),
/// each rotating by number across polls.
pub fn tracked_reads(
    listed: impl Fn(&str) -> bool,
    tracked: &[(String, u64)],
    closed: &[(String, u64)],
    cursor: &mut ListingCursor,
) -> (Numbered, Numbered) {
    let missing = |ids: &[(String, u64)]| {
        let mut ids: Vec<(String, u64)> =
            ids.iter().filter(|(id, _)| !listed(id)).cloned().collect();
        ids.sort_by_key(|(_, number)| *number);
        ids.dedup();
        ids
    };
    let (open, tracked_after) = rotate(&missing(tracked), cursor.tracked_after, TRACKED_BATCH);
    let (closed, closed_after) = rotate(&missing(closed), cursor.closed_after, CLOSED_BATCH);
    (cursor.tracked_after, cursor.closed_after) = (tracked_after, closed_after);
    (open, closed)
}

/// Up to `budget` of `ids` (sorted by number), resuming after number
/// `after` and wrapping; with the number to resume after next time (0: all
/// were read).
pub fn rotate(ids: &[(String, u64)], after: u64, budget: usize) -> (Vec<(String, u64)>, u64) {
    let start = ids.partition_point(|(_, number)| *number <= after);
    let chosen: Vec<_> = ids[start..]
        .iter()
        .chain(&ids[..start])
        .take(budget)
        .cloned()
        .collect();
    let next = if ids.len() > budget {
        chosen.last().map_or(0, |(_, number)| *number)
    } else {
        0
    };
    (chosen, next)
}

/// Whether a listing read failed for a reason a later poll can clear.
/// Access, identity and repository failures, and a refused query, surface.
pub fn resumable(error: ConnectionError) -> bool {
    !matches!(
        error,
        ConnectionError::WrongIdentity
            | ConnectionError::MissingCli
            | ConnectionError::BrokenCli
            | ConnectionError::SignedOut
            | ConnectionError::MissingReadPermission
            | ConnectionError::RepositoryUnavailable
            | ConnectionError::MissingScope
            | ConnectionError::ScopeUnverified
            | ConnectionError::OrganizationPolicyDenied
            | ConnectionError::OrganizationPolicyDeniedWithMissingScope
            | ConnectionError::ProviderRejected
            | ConnectionError::InvalidRepository
            | ConnectionError::RepositoryChanged
            | ConnectionError::Configuration
    ) && !matches!(error, ConnectionError::ProviderRejectedStatus(status) if status != 408)
}

pub fn throttled(error: ConnectionError) -> bool {
    matches!(
        error,
        ConnectionError::RateLimited
            | ConnectionError::RateLimitedAfter(_)
            | ConnectionError::RateLimitedWithContext { .. }
    )
}

pub fn stated_wait(error: ConnectionError) -> Option<i64> {
    match error {
        ConnectionError::RateLimitedAfter(seconds)
        | ConnectionError::ProviderFailureAfter(seconds) => Some(seconds),
        ConnectionError::RateLimitedWithContext {
            retry_after_seconds,
            ..
        } => retry_after_seconds,
        _ => None,
    }
}
